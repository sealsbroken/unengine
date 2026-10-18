//! The lesson runtime. A [`Strategy`] is a pure reducer; this crate owns the threads:
//! an effect queue that speaks and listens in order, a nudge timer and a listening
//! thread. The app calls [`Engine::observe`] once per frame and
//! reads [`Engine::snapshot`] to draw the HUD.
//!
//! Speech is abstracted by [`SpeechInput`] and [`SpeechOutput`]; `un-speech` provides
//! sherpa-onnx implementations, and the test doubles in [`doubles`] drive a lesson
//! without audio.

pub mod doubles;
pub mod speech;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::{Condvar, Mutex, RwLock};
use serde::{Deserialize, Serialize};
use un_core::strategy::{
    Effect, Event, ListenMode, PhaseKind, SessionConfig, SessionSummary, Strategy, StrategySnapshot, TableObservation,
};
use un_core::{NumberParser, TableState};

pub use speech::{CancelToken, NumberHeard, SpeechError, SpeechInput, SpeechOutput};

/// How long a child counting aloud may be quiet before the sequence is taken as finished
/// ([`ListenMode::Counting`]), counted from the last number heard or from the end of the
/// tutor's last word, whichever is later: a recogniser hands a word over a second or so
/// after it ends, and the tutor's "good" in between takes another, so the child's own pause
/// is shorter than this.
pub const COUNTING_GAP: Duration = Duration::from_secs(5);
/// The quiet that ends the count once it has reached the expected answer: a beat, so a child
/// who goes on ("...four, five") is still heard, without the tutor jumping in on the last word.
pub const COUNTING_DONE_GAP: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Idle,
    Starting,
    Watching,
    Speaking,
    Listening,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub kind: StatusKind,
    pub listen_mode: Option<ListenMode>,
}

impl Status {
    fn new(kind: StatusKind) -> Self {
        Self { kind, listen_mode: None }
    }
}

/// Everything the HUD needs, copied under the lock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub status: Status,
    pub strategy: StrategySnapshot,
    pub last_spoken: String,
    /// Count-along animation in progress: number the objects 1...n on screen.
    pub highlight_count: Option<u32>,
    /// Seconds since the last correct answer, for a celebration animation.
    pub celebrated_seconds_ago: Option<f64>,
    pub summary: Option<SessionSummary>,
    pub warning: Option<String>,
    pub show_number_pad: bool,
    pub speech_available: bool,
    pub mic_name: String,
    pub mic_level: f32,
    /// The latest transcript from the microphone, empty after a few seconds.
    pub heard_text: String,
}

/// Receives one line per thing the engine does.
pub type Logger = Arc<dyn Fn(&str) + Send + Sync>;

/// Options that are not part of the strategy's [`SessionConfig`].
#[derive(Default)]
pub struct EngineOptions {
    /// Play the "listen now" chime before each listen.
    pub chime: bool,
    /// Always show the number pad, not only as a fallback.
    pub always_show_number_pad: bool,
    /// Receives a one-line transcript of everything the engine does.
    pub log: Option<Logger>,
    /// Keep the microphone open for the whole session, not only while the strategy listens:
    /// whatever the child says outside an answer window reaches the strategy as `Said`. It is
    /// closed only while the tutor speaks, so the tutor never hears itself.
    pub always_listen: bool,
}

/// The always-on microphone's state (see [`EngineOptions::always_listen`]).
#[derive(Default)]
struct Mic {
    /// The tutor is speaking: the microphone is closed.
    speaking: bool,
    /// The listen in flight, cancelled when the tutor starts to speak.
    token: Option<Arc<CancelToken>>,
    /// The tutor's line spoken with the microphone open (during a count), and when it ended:
    /// its words are taken out of what the microphone hears for a moment.
    echo: Option<(String, Option<Instant>)>,
    /// The tutor's recent lines and when each ended (`None` while it is being said): a
    /// transcript that is mostly one of them is the tutor heard through the microphone.
    lines: Vec<(String, Option<Instant>)>,
}

/// How long after the tutor's line a transcript of it may still come back from the microphone.
const ECHO_WINDOW: Duration = Duration::from_secs(8);

/// Whether `heard` is mostly the tutor's own words: at least two of them, and seven in ten of
/// what was heard. "Готов" or "да, готов" after "...Готов?" is the child; "Хорошо до встречи"
/// after "Хорошо, Вася, до встречи." is the tutor.
fn is_echo(heard: &str, tutor: &str) -> bool {
    let word = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let tutor: Vec<String> = tutor.split_whitespace().map(word).filter(|w| !w.is_empty()).collect();
    let heard: Vec<String> = heard.split_whitespace().map(word).filter(|w| !w.is_empty()).collect();
    let matched = heard.iter().filter(|w| tutor.contains(w)).count();
    !heard.is_empty() && matched >= 2 && matched * 10 >= heard.len() * 7
}

/// How long after the tutor's line its words may still turn up in a transcript: a recogniser
/// hands an utterance over a second or so after it ends.
const ECHO_TAIL: Duration = Duration::from_secs(3);

/// The line as the voice should read it: without quotation marks, backslashes and markup, which
/// a synthesiser reads out as symbols ("скажи «два»" came out as "скажи бэкслеш два бэкслеш").
/// The HUD keeps the written line.
fn for_voice(text: &str) -> String {
    let kept: String =
        text.chars().filter(|c| !matches!(c, '«' | '»' | '"' | '“' | '”' | '„' | '\\' | '*' | '_' | '`' | '#')).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "1234" as "1 2 3 4": a recogniser writes a fast "one two three four" as one number, and a
/// wrong count too ("1235"). Spread: runs of consecutive digits going up ("12", "234"), and any
/// longer run of digits going up from 1 that is too big to be an answer ("1235"); "10", "15",
/// "25" stay what they are.
fn spread_digits(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            let digits: Vec<u32> = word.chars().filter_map(|c| c.to_digit(10)).collect();
            let all_digits =
                digits.len() >= 2 && digits.len() == word.trim_matches(|c: char| !c.is_alphanumeric()).chars().count();
            let consecutive = digits.windows(2).all(|w| w[1] == w[0] + 1);
            let rising_from_one = digits.first() == Some(&1) && digits.windows(2).all(|w| w[1] > w[0]) && digits.len() >= 3;
            let run = all_digits && (consecutive || rising_from_one);
            if run {
                digits.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(" ")
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Preserve explicit zero (and invalid signed/out-of-range count tokens) for the
/// strategy to judge. Zero-like conversational phrases remain ordinary speech.
fn counting_numbers(text: &str, language: un_core::Language) -> Vec<u32> {
    let spread = spread_digits(text);
    let tokens = un_core::numbers::tokenize(&spread);
    let mut words = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let word = tokens[i].as_str();
        let pair = tokens.get(i + 1).map(|next| (word, next.as_str()));
        if matches!(pair, Some(("no", "cube" | "cubes") | ("not", "any") | ("ни", "одного" | "одной"))) {
            i += 2;
        } else if matches!(word, "none" | "nothing" | "nope" | "нет" | "ничего" | "нисколько") {
            i += 1;
        } else {
            words.push(word);
            i += 1;
        }
    }
    let mut numbers = NumberParser::all(&words.join(" "), language);
    let invalid = invalid_numeric_input(text, &tokens, !numbers.is_empty());
    // u32 cannot represent a negative or overflowing number. An invalid marker
    // prevents silently dropping it and turning a wrong count into a valid one.
    if invalid {
        numbers.push(u32::MAX);
    }
    numbers
}

fn invalid_numeric_input(text: &str, tokens: &[String], has_numbers: bool) -> bool {
    let out_of_range = tokens
        .iter()
        .any(|t| t.bytes().all(|b| b.is_ascii_digit()) && t.parse::<u32>().ok().is_none_or(|n| n > NumberParser::MAX));
    let signed = tokens.iter().any(|t| matches!(t.as_str(), "minus" | "negative" | "минус"))
        || text.split_whitespace().any(|word| {
            (word.starts_with('-') || word.starts_with('−'))
                && word.trim_start_matches(['-', '−']).starts_with(|c: char| c.is_ascii_digit())
        });
    out_of_range || (signed && has_numbers)
}

fn answer_numbers(text: &str, language: un_core::Language) -> Vec<u32> {
    let mut numbers = NumberParser::all(text, language);
    if invalid_numeric_input(text, &un_core::numbers::tokenize(text), !numbers.is_empty()) {
        numbers.push(u32::MAX);
    }
    numbers
}

/// A count said aloud, right or wrong ("one, two, three", "1235", "один два три пять"): at least
/// two numbers, including counts starting at zero. The strategy judges it; a wrong count is to be corrected, not
/// taken for chatter.
fn spoken_count(text: &str, language: un_core::Language) -> Option<Vec<u32>> {
    let numbers = counting_numbers(text, language);
    (numbers.len() >= 2 && numbers.first().is_some_and(|n| *n <= 1)).then_some(numbers)
}

/// `heard` without the words of `tutor` (case and punctuation aside), in order.
fn without_echo(heard: &str, tutor: &str) -> String {
    let word = |w: &str| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    let tutor: Vec<String> = tutor.split_whitespace().map(word).filter(|w| !w.is_empty()).collect();
    heard.split_whitespace().filter(|w| !tutor.contains(&word(w))).collect::<Vec<_>>().join(" ")
}

struct State {
    strategy: Box<dyn Strategy>,
    status: Status,
    last_spoken: String,
    highlight: Option<u32>,
    celebrated_at: Option<Instant>,
    summary: Option<SessionSummary>,
    warning: Option<String>,
    speech_available: bool,
    show_number_pad: bool,
    always_show_number_pad: bool,
    last_observation: Option<TableObservation>,
    /// The table as the strategy last saw it. While it asks (the child counts or answers) the
    /// table is held back: the child points at the toys while counting, and a hand moving over
    /// them must not cancel the question. The latest table follows once the question is over.
    table_seen: Option<TableObservation>,
    /// When the tutor last finished a line: a count's quiet window starts after it.
    tutor_silent_at: Option<Instant>,
    /// Whether the microphone currently picks up sound, as last told to the strategy.
    sound: Option<bool>,
    sound_heard_at: Option<Instant>,
    timers: HashMap<u64, Arc<TimerHandle>>,
    /// Timers cancelled before their (queued) start ran.
    cancelled_early: Vec<u64>,
    listen: Option<Arc<CancelToken>>,
    closed: bool,
}

struct TimerHandle {
    cancelled: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

impl TimerHandle {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _guard = self.lock.lock();
        self.cv.notify_all();
    }
}

struct Inner {
    self_arc: Weak<Inner>,
    config: SessionConfig,
    options: EngineOptions,
    speech_in: Option<Arc<dyn SpeechInput>>,
    speech_out: RwLock<Arc<dyn SpeechOutput>>,
    state: Mutex<State>,
    effects: Sender<Option<Effect>>,
    listen_generation: AtomicU64,
    mic: Mutex<Mic>,
    mic_cv: Condvar,
    /// Utterances from the always-on microphone for the strategy's listen in progress.
    utterances: (Sender<String>, Receiver<String>),
}

/// Runs one lesson session.
pub struct Engine {
    inner: Arc<Inner>,
    worker: Mutex<Option<JoinHandle<()>>>,
    effects_rx: Mutex<Option<Receiver<Option<Effect>>>>,
}

impl Engine {
    pub fn new(
        strategy: Box<dyn Strategy>,
        config: SessionConfig,
        speech_in: Option<Arc<dyn SpeechInput>>,
        speech_out: Arc<dyn SpeechOutput>,
        options: EngineOptions,
    ) -> Self {
        let (tx, rx) = unbounded();
        let always = options.always_show_number_pad;
        let inner = Arc::new_cyclic(|weak| Inner {
            self_arc: weak.clone(),
            config,
            options,
            speech_in,
            speech_out: RwLock::new(speech_out),
            state: Mutex::new(State {
                strategy,
                status: Status::new(StatusKind::Idle),
                last_spoken: String::new(),
                highlight: None,
                celebrated_at: None,
                summary: None,
                warning: None,
                speech_available: false,
                show_number_pad: false,
                always_show_number_pad: always,
                last_observation: None,
                table_seen: None,
                tutor_silent_at: None,
                sound: None,
                sound_heard_at: None,
                timers: HashMap::new(),
                cancelled_early: Vec::new(),
                listen: None,
                closed: false,
            }),
            effects: tx,
            listen_generation: AtomicU64::new(0),
            mic: Mutex::new(Mic::default()),
            mic_cv: Condvar::new(),
            utterances: unbounded(),
        });
        Self { inner, worker: Mutex::new(None), effects_rx: Mutex::new(Some(rx)) }
    }

    pub fn config(&self) -> &SessionConfig {
        &self.inner.config
    }

    // ------------------------------------------------------------------ lifecycle

    /// Prepares speech, starts the effect worker and sends `Start` to the strategy.
    pub fn start(&self) {
        {
            let mut state = self.inner.state.lock();
            if state.status.kind != StatusKind::Idle {
                return;
            }
            state.status = Status::new(StatusKind::Starting);
        }
        let language = self.inner.config.language;
        let out_ok = self.inner.speech_out.read().prepare(language);
        if let Err(error) = out_ok {
            self.inner.log(&format!("warning: voice output unavailable ({error}); tutor lines go to the log"));
            *self.inner.speech_out.write() = Arc::new(doubles::SilentSpeechOutput::new(true));
        }
        match &self.inner.speech_in {
            Some(speech_in) => match speech_in.prepare(language) {
                Ok(()) => self.inner.state.lock().speech_available = true,
                Err(error) => {
                    let mut state = self.inner.state.lock();
                    state.speech_available = false;
                    state.always_show_number_pad = true;
                    let warning = format!("Voice answers unavailable ({error}). Using the number pad.");
                    self.inner.log(&format!("warning: {warning}"));
                    state.warning = Some(warning);
                }
            },
            None => self.inner.state.lock().always_show_number_pad = true,
        }
        if let Some(rx) = self.effects_rx.lock().take() {
            let inner = Arc::clone(&self.inner);
            *self.worker.lock() =
                Some(thread::Builder::new().name("un-effects".into()).spawn(move || inner.run_effects(rx)).expect("spawn"));
        }
        self.inner.state.lock().status = Status::new(StatusKind::Watching);
        let speech_available = self.inner.state.lock().speech_available;
        if let (true, true, Some(speech_in)) = (self.inner.options.always_listen, speech_available, self.inner.speech_in.clone())
        {
            let inner = Arc::clone(&self.inner);
            thread::Builder::new().name("un-mic".into()).spawn(move || inner.run_mic(speech_in)).expect("spawn");
        }
        self.inner.dispatch(Event::Start);
        self.inner.sense_sound();
    }

    /// Ends the lesson politely: the tutor says goodbye, then the summary appears.
    pub fn stop(&self) {
        self.inner.dispatch(Event::Stop);
    }

    /// Tears everything down immediately (quitting the app).
    pub fn close(&self) {
        {
            let mut state = self.inner.state.lock();
            if state.closed {
                return;
            }
            state.closed = true;
            for timer in state.timers.values() {
                timer.cancel();
            }
            state.timers.clear();
        }
        self.inner.cancel_listening();
        if let Some(token) = self.inner.mic.lock().token.take() {
            token.cancel();
        }
        self.inner.mic_cv.notify_all();
        if let Some(speech_in) = &self.inner.speech_in {
            speech_in.stop();
        }
        self.inner.speech_out.read().stop();
        let _ = self.inner.effects.send(None);
        if let Some(worker) = self.worker.lock().take() {
            if thread::current().id() != worker.thread().id() {
                let _ = worker.join();
            }
        }
    }

    pub fn repeat(&self) {
        self.inner.dispatch(Event::RepeatRequested);
    }

    pub fn skip(&self) {
        self.inner.dispatch(Event::SkipRequested);
    }

    /// Number pad answer.
    pub fn tap(&self, number: u32) {
        self.inner.state.lock().show_number_pad = false;
        self.inner.cancel_listening();
        self.inner.log(&format!("tapped {number}"));
        self.inner.dispatch(Event::Heard(number));
    }

    /// A number arrived from any other input (a test).
    pub fn heard(&self, number: u32) {
        self.inner.cancel_listening();
        self.inner.dispatch(Event::Heard(number));
    }

    // ------------------------------------------------------------------ perception

    pub fn observe(&self, state: &TableState) {
        let observation = TableObservation { count: state.cube_count, is_stable: state.is_stable, groups: state.groups };
        self.observe_table(observation);
    }

    pub fn observe_table(&self, observation: TableObservation) {
        self.inner.sense_sound();
        {
            let mut state = self.inner.state.lock();
            if state.last_observation == Some(observation) {
                return;
            }
            state.last_observation = Some(observation);
        }
        self.inner.dispatch(Event::Table(observation));
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let state = self.inner.state.lock();
        let finished = matches!(state.status.kind, StatusKind::Finished | StatusKind::Idle);
        EngineSnapshot {
            status: state.status.clone(),
            strategy: state.strategy.snapshot(),
            last_spoken: state.last_spoken.clone(),
            highlight_count: state.highlight,
            celebrated_seconds_ago: state.celebrated_at.map(|t| t.elapsed().as_secs_f64()),
            summary: state.summary.clone(),
            warning: state.warning.clone(),
            show_number_pad: state.show_number_pad || (state.always_show_number_pad && !finished),
            speech_available: state.speech_available,
            mic_name: self.inner.speech_in.as_ref().map(|s| s.device_name()).unwrap_or_default(),
            mic_level: self.inner.speech_in.as_ref().map(|s| s.level()).unwrap_or(0.0),
            heard_text: self.inner.speech_in.as_ref().map(|s| s.last_transcript()).unwrap_or_default(),
        }
    }

    /// The strategy's own view, for tests and logs.
    pub fn strategy_snapshot(&self) -> StrategySnapshot {
        self.inner.state.lock().strategy.snapshot()
    }

    pub fn phase(&self) -> PhaseKind {
        self.strategy_snapshot().phase
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

impl Inner {
    fn log(&self, text: &str) {
        match &self.options.log {
            Some(log) => log(text),
            None => log::info!("{text}"),
        }
    }

    fn dispatch(&self, event: Event) {
        let queued: Vec<Effect> = {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            if state.strategy.snapshot().phase == PhaseKind::Idle && event != Event::Start {
                return;
            }
            let starting = event == Event::Start;
            if let Event::Table(observation) = event {
                if state.strategy.snapshot().phase == PhaseKind::Asking {
                    return;
                }
                state.table_seen = Some(observation);
            }
            let mut effects = state.strategy.handle(event);
            // Replay pre-start perception under the same strategy lock, so a
            // concurrent newer table cannot be overwritten by an older replay.
            if starting {
                if let Some(observation) = state.last_observation {
                    state.table_seen = Some(observation);
                    effects.extend(state.strategy.handle(Event::Table(observation)));
                }
            }
            // A table held back during a question reaches the strategy once the question is over.
            if state.strategy.snapshot().phase != PhaseKind::Asking && state.last_observation != state.table_seen {
                if let Some(observation) = state.last_observation {
                    state.table_seen = Some(observation);
                    effects.extend(state.strategy.handle(Event::Table(observation)));
                }
            }
            let mut queued = Vec::new();
            for effect in effects {
                // Cancelling a timer or a listen must not wait behind speech; starting a timer
                // must (a waiting window counts from the end of the tutor's line).
                match effect {
                    Effect::CancelTimer(token) => match state.timers.remove(&token) {
                        Some(timer) => timer.cancel(),
                        None => state.cancelled_early.push(token),
                    },
                    Effect::StopListening => {
                        let token = state.listen.take();
                        drop(state);
                        self.finish_listening(token.as_deref(), true);
                        state = self.state.lock();
                    }
                    other => queued.push(other),
                }
            }
            queued
        };
        for effect in queued {
            let _ = self.effects.send(Some(effect));
        }
    }

    fn run_effects(self: Arc<Self>, rx: Receiver<Option<Effect>>) {
        while let Ok(Some(effect)) = rx.recv() {
            self.run(effect);
        }
    }

    fn run(&self, effect: Effect) {
        match effect {
            Effect::Say(text) => {
                {
                    let mut state = self.state.lock();
                    state.last_spoken = text.clone();
                    state.status = Status::new(StatusKind::Speaking);
                }
                self.log(&format!("tutor: {text}"));
                {
                    let mut mic = self.mic.lock();
                    mic.lines.retain(|(_, ended)| ended.is_none_or(|t| t.elapsed() < ECHO_WINDOW));
                    mic.lines.push((text.clone(), None));
                }
                // During a count the microphone stays open: the tutor's "good" overlaps the child's
                // next number. Its words are taken out of the transcript instead.
                let counting = {
                    let state = self.state.lock();
                    state.listen.is_some() && state.status.listen_mode == Some(ListenMode::Counting)
                };
                if self.options.always_listen && counting {
                    self.mic.lock().echo = Some((text.clone(), None));
                } else if self.options.always_listen {
                    // Otherwise the microphone closes while the tutor speaks, so the tutor never
                    // hears itself (and never answers its own words).
                    let token = {
                        let mut mic = self.mic.lock();
                        mic.speaking = true;
                        mic.token.take()
                    };
                    if let Some(token) = token {
                        token.cancel();
                        if let Some(speech_in) = &self.speech_in {
                            speech_in.stop();
                        }
                    }
                }
                if let Some(speech_in) = &self.speech_in {
                    speech_in.tutor_speaking(&text, self.config.language);
                }
                let out = Arc::clone(&self.speech_out.read());
                if let Err(error) = out.speak(&for_voice(&text), self.config.language) {
                    self.log(&format!("voice failed: {error}"));
                }
                if let Some(speech_in) = &self.speech_in {
                    speech_in.tutor_silent();
                }
                {
                    let mut mic = self.mic.lock();
                    if let Some(line) = mic.lines.iter_mut().rev().find(|(_, ended)| ended.is_none()) {
                        line.1 = Some(Instant::now());
                    }
                }
                if self.options.always_listen {
                    let mut mic = self.mic.lock();
                    mic.speaking = false;
                    if let Some((_, ended)) = mic.echo.as_mut() {
                        *ended = Some(Instant::now());
                    }
                    drop(mic);
                    self.mic_cv.notify_all();
                }
                let mut state = self.state.lock();
                state.tutor_silent_at = Some(Instant::now());
                if state.status.kind == StatusKind::Speaking {
                    state.status = Status::new(StatusKind::Watching);
                }
            }
            Effect::Listen { mode, timeout } => self.begin_listening(mode, timeout),
            Effect::HighlightCount(count) => {
                self.state.lock().highlight = Some(count);
                thread::sleep(Duration::from_secs_f64(count as f64 * 0.6 + 0.5));
                self.state.lock().highlight = None;
            }
            Effect::Celebrate => self.state.lock().celebrated_at = Some(Instant::now()),
            Effect::SessionEnded(summary) => {
                {
                    let mut state = self.state.lock();
                    state.summary = Some(summary.clone());
                    state.status = Status::new(StatusKind::Finished);
                    state.show_number_pad = false;
                }
                self.cancel_listening();
                self.log(&format!(
                    "session ended: {}/{} correct, level {}",
                    summary.correct,
                    summary.tasks_completed,
                    summary.final_level + 1
                ));
            }
            Effect::StartTimer { token, seconds } => {
                let mut state = self.state.lock();
                if let Some(i) = state.cancelled_early.iter().position(|t| *t == token) {
                    state.cancelled_early.swap_remove(i);
                } else if !state.closed {
                    self.start_timer(&mut state, token, seconds);
                }
            }
            Effect::CancelTimer(_) | Effect::StopListening => {}
        }
    }

    // ------------------------------------------------------------------ the always-on microphone

    fn closed(&self) -> bool {
        self.state.lock().closed
    }

    /// Listens for the whole session, one utterance after another, except while the tutor
    /// speaks; each utterance goes to the strategy's listen in progress, or as `Said`.
    fn run_mic(self: Arc<Self>, speech_in: Arc<dyn SpeechInput>) {
        let language = self.config.language;
        let mut failures = 0u32;
        loop {
            if self.closed() {
                return;
            }
            let token = {
                let mut mic = self.mic.lock();
                if mic.speaking {
                    self.mic_cv.wait_for(&mut mic, Duration::from_millis(200));
                    continue;
                }
                let token = Arc::new(CancelToken::new(0));
                mic.token = Some(Arc::clone(&token));
                token
            };
            let result = speech_in.listen_for_text(language, Duration::from_secs(30), &token, &|_| true);
            {
                let mut mic = self.mic.lock();
                if mic.token.as_ref().is_some_and(|t| Arc::ptr_eq(t, &token)) {
                    mic.token = None;
                }
            }
            if self.closed() {
                return;
            }
            match result {
                Ok(text) => {
                    failures = 0;
                    if !token.is_cancelled() {
                        self.route_utterance(text);
                    }
                }
                Err(SpeechError::Timeout { .. }) | Err(SpeechError::Cancelled) => failures = 0,
                Err(error) => {
                    failures += 1;
                    if failures == 1 {
                        self.log(&format!("microphone failed ({error}); retrying"));
                    }
                    thread::sleep(Duration::from_millis(500 * failures.min(10) as u64));
                }
            }
        }
    }

    /// An utterance from the always-on microphone: the strategy's listen takes it when one is
    /// open (numbers, a count), otherwise it is speech the strategy hears as `Said`.
    fn route_utterance(&self, text: String) {
        let mut text = text.trim().to_string();
        {
            let mut mic = self.mic.lock();
            if mic.speaking {
                drop(mic);
                self.log(&format!("ignored while the tutor speaks: {text}"));
                return;
            }
            let echo = mic
                .lines
                .iter()
                .filter(|(_, ended)| ended.is_none_or(|t| t.elapsed() < ECHO_WINDOW))
                .any(|(line, _)| is_echo(&text, line));
            if echo {
                drop(mic);
                self.log(&format!("the tutor's own line came back, ignored: {text}"));
                return;
            }
            match &mic.echo {
                Some((_, Some(ended))) if ended.elapsed() > ECHO_TAIL => mic.echo = None,
                Some((line, _)) => {
                    let kept = without_echo(&text, line);
                    if kept != text {
                        self.log(&format!("tutor's own words taken out: {text:?} -> {kept:?}"));
                        text = kept;
                    }
                }
                None => {}
            }
        }
        if text.is_empty() {
            return;
        }
        let listening = {
            let state = self.state.lock();
            state.listen.is_some() && state.status.listen_mode.is_some_and(|m| m != ListenMode::NumberPad)
        };
        if listening {
            let _ = self.utterances.0.send(text);
        } else if let Some(count) = spoken_count(&text, self.config.language) {
            // A count with no answer window open (it ran out a moment before): still a count.
            self.log(&format!("child counted: {text}"));
            let last = *count.last().expect("two numbers at least");
            self.dispatch(Event::Numbers(count));
            self.dispatch(Event::Heard(last));
        } else {
            self.log(&format!("child: {text}"));
            self.dispatch(Event::Said(text));
        }
    }

    /// The next utterance for the listen `token` belongs to, from the always-on microphone.
    fn next_utterance(&self, timeout: Duration, token: &CancelToken) -> Result<String, SpeechError> {
        let deadline = Instant::now() + timeout;
        loop {
            if token.is_cancelled() {
                return Err(SpeechError::Cancelled);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(SpeechError::Timeout { last_transcript: None });
            }
            match self.utterances.1.recv_timeout(left.min(Duration::from_millis(100))) {
                Ok(text) => return Ok(text),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Err(SpeechError::Cancelled),
            }
        }
    }

    // ------------------------------------------------------------------ listening

    fn begin_listening(self: &Inner, mode: ListenMode, timeout: f64) {
        self.cancel_listening();
        while self.utterances.1.try_recv().is_ok() {}
        let token = Arc::new(CancelToken::new(self.listen_generation.fetch_add(1, Ordering::SeqCst) + 1));
        let speech_available = {
            let mut state = self.state.lock();
            state.status = Status { kind: StatusKind::Listening, listen_mode: Some(mode) };
            state.show_number_pad = mode == ListenMode::NumberPad || state.always_show_number_pad;
            state.listen = Some(Arc::clone(&token));
            state.speech_available
        };
        let this = self.arc();
        if mode != ListenMode::NumberPad && speech_available {
            if let Some(speech_in) = self.speech_in.clone() {
                let token = Arc::clone(&token);
                let this = Arc::clone(&this);
                thread::Builder::new()
                    .name("un-listen".into())
                    .spawn(move || this.listen(speech_in, mode, timeout, token))
                    .expect("spawn");
            }
        }
    }

    /// Tells the strategy when the microphone starts or stops picking up sound. Called on the
    /// perception heartbeat; only a metered microphone (see [`SpeechInput::meters_level`]) counts.
    fn sense_sound(&self) {
        const THRESHOLD: f32 = 0.1;
        const QUIET_AFTER: Duration = Duration::from_secs(2);
        let Some(speech_in) = &self.speech_in else { return };
        if !speech_in.meters_level() {
            return;
        }
        let level = speech_in.level();
        let event = {
            let mut state = self.state.lock();
            if !state.speech_available {
                return;
            }
            let now = Instant::now();
            if level >= THRESHOLD {
                state.sound_heard_at = Some(now);
            }
            let hears = state.sound_heard_at.is_some_and(|t| now.duration_since(t) < QUIET_AFTER);
            if state.sound == Some(hears) {
                return;
            }
            state.sound = Some(hears);
            hears
        };
        self.log(if event { "microphone: sound" } else { "microphone: quiet" });
        self.dispatch(Event::Sound(event));
    }

    /// One listen: every utterance until the timeout is reported, words as `Said`, and the
    /// first number ends it as `Heard`. In [`ListenMode::Counting`] the numbers are collected
    /// across utterances instead (a child counting pauses between the words, and every
    /// recogniser cuts an utterance at a pause): the listen ends when the child has been quiet
    /// for [`COUNTING_GAP`] after the last number, or for [`COUNTING_DONE_GAP`] once the
    /// sequence has reached the expected answer. The child is never cut off on the last word,
    /// and `timeout` is the time allowed before the first number and after each one, not for
    /// the whole count: four objects with a finger on each take longer than an answer.
    fn listen(&self, speech_in: Arc<dyn SpeechInput>, mode: ListenMode, timeout: f64, token: Arc<CancelToken>) {
        if self.options.chime {
            self.speech_out.read().play_chime();
        }
        let language = self.config.language;
        let counting = mode == ListenMode::Counting;
        let expected = if counting { self.state.lock().strategy.snapshot().expected_answer } else { None };
        let mut deadline = Instant::now() + Duration::from_secs_f64(timeout);
        let mut sequence: Vec<u32> = Vec::new();
        let mut last_number_at: Option<Instant> = None;
        let finish = |sequence: Vec<u32>, text: &str| {
            let value = *sequence.last().expect("a number");
            self.log(&format!("heard {sequence:?} ({text:?})"));
            self.finish_listening(Some(&token), false);
            self.dispatch(Event::Numbers(sequence));
            self.dispatch(Event::Heard(value));
        };
        loop {
            if token.is_cancelled() {
                return;
            }
            let now = Instant::now();
            let mut remaining = deadline.saturating_duration_since(now);
            if let Some(at) = last_number_at {
                // Quiet is counted from the later of the last number and the tutor's last word.
                let silent = self.state.lock().tutor_silent_at.filter(|t| *t > at);
                let since = silent.unwrap_or(at);
                let done = expected.is_some_and(|e| sequence.last() == Some(&e));
                let gap = if done { COUNTING_DONE_GAP } else { COUNTING_GAP };
                remaining = remaining.min(gap.saturating_sub(now.duration_since(since)));
            }
            let result = if remaining.is_zero() {
                Err(SpeechError::Timeout { last_transcript: None })
            } else if self.options.always_listen {
                self.next_utterance(remaining, &token)
            } else {
                speech_in.listen_for_text(language, remaining, &token, &|_| true)
            };
            match result {
                Ok(text) => {
                    if token.is_cancelled() {
                        return;
                    }
                    let mut numbers = if counting { counting_numbers(&text, language) } else { answer_numbers(&text, language) };
                    if !counting && numbers.iter().all(|n| *n == 0) && text.split_whitespace().count() >= 3 {
                        // "Нет, объясни, зачем мне учиться считать" is talk, not the answer zero.
                        numbers.clear();
                    }
                    if numbers.is_empty() {
                        self.log(&format!("child: {text}"));
                        self.dispatch(Event::Said(text));
                    } else if !counting {
                        return finish(numbers, &text);
                    } else {
                        self.log(&format!("counting: {text}"));
                        sequence.extend(numbers.iter().copied());
                        last_number_at = Some(Instant::now());
                        deadline = Instant::now() + Duration::from_secs_f64(timeout);
                        self.dispatch(Event::Counted(numbers));
                    }
                }
                Err(SpeechError::Cancelled) => return,
                Err(error) => {
                    if token.is_cancelled() {
                        return;
                    }
                    if !sequence.is_empty() && matches!(error, SpeechError::Timeout { .. }) {
                        // The child stopped counting: the sequence so far is the answer.
                        return finish(sequence, "");
                    }
                    let kind = if matches!(error, SpeechError::Timeout { .. }) { "heard nothing" } else { "listening failed" };
                    self.log(&format!("{kind} ({error})"));
                    self.finish_listening(Some(&token), false);
                    self.dispatch(Event::NothingHeard);
                    return;
                }
            }
        }
    }

    /// Ends the listen `token` belongs to (or the current one when `None`).
    fn finish_listening(&self, token: Option<&CancelToken>, force_current: bool) {
        {
            let mut state = self.state.lock();
            let current = state.listen.as_ref().map(|t| t.generation);
            if let Some(token) = token {
                if !force_current && current != Some(token.generation) {
                    return; // a newer listen already replaced this one
                }
                token.cancel();
            }
            if let Some(current) = state.listen.take() {
                current.cancel();
            }
            if state.status.kind == StatusKind::Listening {
                state.status = Status::new(StatusKind::Watching);
            }
            state.show_number_pad = state.always_show_number_pad && state.status.kind != StatusKind::Finished;
        }
        // The always-on microphone stays open: a listen ending is not a reason to stop hearing.
        if let (Some(speech_in), false) = (&self.speech_in, self.options.always_listen) {
            speech_in.stop();
        }
    }

    fn cancel_listening(&self) {
        self.finish_listening(None, true);
    }

    // ------------------------------------------------------------------ timers

    fn start_timer(&self, state: &mut State, token: u64, seconds: f64) {
        if let Some(old) = state.timers.remove(&token) {
            old.cancel();
        }
        let handle = Arc::new(TimerHandle { cancelled: AtomicBool::new(false), lock: Mutex::new(()), cv: Condvar::new() });
        state.timers.insert(token, Arc::clone(&handle));
        let this = self.arc();
        thread::Builder::new()
            .name("un-timer".into())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs_f64(seconds);
                {
                    let mut guard = handle.lock.lock();
                    while !handle.cancelled.load(Ordering::SeqCst) {
                        let now = Instant::now();
                        if now >= deadline {
                            break;
                        }
                        handle.cv.wait_for(&mut guard, deadline - now);
                    }
                }
                if handle.cancelled.load(Ordering::SeqCst) {
                    return;
                }
                let fired = {
                    let mut state = this.state.lock();
                    state.timers.remove(&token).is_some()
                };
                if fired {
                    this.dispatch(Event::TimerFired(token));
                }
            })
            .expect("spawn");
    }

    fn arc(&self) -> Arc<Inner> {
        self.self_arc.upgrade().expect("engine alive")
    }
}

#[cfg(test)]
mod echo_tests {
    use super::without_echo;

    #[test]
    fn a_fast_count_written_as_one_number_is_spread() {
        use un_core::Language::Russian;
        assert_eq!(super::spread_digits("1234"), "1 2 3 4");
        assert_eq!(super::spread_digits("10 25 12"), "10 25 1 2");
        assert_eq!(super::spread_digits("1235"), "1 2 3 5");
        assert_eq!(super::spread_digits("15"), "15");
        assert_eq!(super::spoken_count("один два три пять", Russian), Some(vec![1, 2, 3, 5]));
        assert_eq!(super::spoken_count("1235", Russian), Some(vec![1, 2, 3, 5]));
        assert_eq!(super::spoken_count("1234", Russian), Some(vec![1, 2, 3, 4]));
        assert_eq!(super::spoken_count("один два три", Russian), Some(vec![1, 2, 3]));
        assert_eq!(super::spoken_count("мне пять лет", Russian), None);
        assert_eq!(super::spoken_count("два три", Russian), None);
        assert_eq!(super::spoken_count("мне пять или шесть лет", Russian), None);
    }

    #[test]
    fn quotes_and_backslashes_are_not_read_out() {
        assert_eq!(
            super::for_voice("Поставь палец на одну игрушку, скажи «один». Скажи \\\"два\\\"."),
            "Поставь палец на одну игрушку, скажи один. Скажи два."
        );
        assert_eq!(super::for_voice("Say \"one\"."), "Say one.");
    }

    #[test]
    fn the_tutor_heard_through_the_microphone_is_told_from_the_child() {
        use super::is_echo;
        assert!(is_echo("Хорошо до встречи", "Хорошо, Вася, до встречи."));
        assert!(is_echo(
            "Привет Вася сегодня будем считать с помощью игрушек готов",
            "Привет, Вася! Сегодня будем считать с помощью игрушек. Готов?"
        ));
        assert!(!is_echo("готов", "Привет, Вася! Сегодня будем считать с помощью игрушек. Готов?"));
        assert!(!is_echo("да готов", "Привет, Вася! Сегодня будем считать с помощью игрушек. Готов?"));
        assert!(!is_echo("один два", "Теперь покажи пальцем на каждую игрушку и посчитай вслух."));
    }

    #[test]
    fn the_tutors_words_are_taken_out_of_the_childs() {
        assert_eq!(without_echo("Хорошо два", "Хорошо."), "два");
        assert_eq!(without_echo("хорошо", "Хорошо."), "");
        assert_eq!(without_echo("три четыре", "Так."), "три четыре");
    }
}

#[cfg(test)]
mod adversarial_count_tests {
    use super::*;
    use un_core::Language::{English, Russian};
    #[test]
    fn explicit_zero_is_never_erased_from_a_count() {
        for (text, language) in [
            ("zero one two three", English),
            ("0 1 2 3", English),
            ("0123", English),
            ("ноль один два три", Russian),
            ("нуль один два три", Russian),
            ("one zero two three", English),
            ("один ноль два три", Russian),
        ] {
            assert!(counting_numbers(text, language).contains(&0), "{text}");
        }
        assert_eq!(spoken_count("zero one two", English), Some(vec![0, 1, 2]));
    }
    #[test]
    fn conversational_negatives_are_not_zero_but_explicit_zero_stays() {
        for (text, language) in [
            ("none", English),
            ("no cubes", English),
            ("not any", English),
            ("нет", Russian),
            ("ничего", Russian),
            ("ни одного", Russian),
        ] {
            assert!(counting_numbers(text, language).is_empty(), "{text}");
        }
        assert_eq!(counting_numbers("one no cubes two", English), vec![1, 2]);
        assert_eq!(counting_numbers("один нет ноль два", Russian), vec![1, 0, 2]);
    }
    #[test]
    fn scalar_answers_do_not_drop_a_minus_or_an_out_of_range_final_number() {
        for text in ["minus two", "-2", "−2", "two or 999"] {
            assert_eq!(answer_numbers(text, English).last(), Some(&u32::MAX), "{text}");
        }
        assert_eq!(answer_numbers("минус два", Russian).last(), Some(&u32::MAX));
        assert_eq!(answer_numbers("zero", English), vec![0]);
        assert_eq!(answer_numbers("нет", Russian), vec![0]);
        assert_eq!(answer_numbers("four no five", English), vec![4, 5]);
    }
    #[test]
    fn impossible_numeric_tokens_cannot_be_silently_dropped() {
        for text in [
            "-1 two three",
            "−1 two three",
            "minus one two three",
            "negative one two three",
            "one two three 999",
            "one two three 99999999999999999999999999999999",
        ] {
            assert!(counting_numbers(text, English).contains(&u32::MAX), "{text}");
        }
        assert_eq!(counting_numbers("1-2-3", English), vec![1, 2, 3]);
    }
}
