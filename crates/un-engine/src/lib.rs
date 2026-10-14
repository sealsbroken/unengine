//! The lesson runtime. A [`Strategy`] is a pure reducer; this crate owns the threads:
//! an effect queue that speaks and listens in order, a nudge timer, a listening
//! thread and a finger watcher. The app calls [`Engine::observe`] once per frame and
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
    finger_count: Option<u32>,
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
                finger_count: None,
                sound: None,
                sound_heard_at: None,
                timers: HashMap::new(),
                cancelled_early: Vec::new(),
                listen: None,
                closed: false,
            }),
            effects: tx,
            listen_generation: AtomicU64::new(0),
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

    /// A number arrived from any other input (a platform's own hand tracker, a test).
    pub fn heard(&self, number: u32) {
        self.inner.cancel_listening();
        self.inner.dispatch(Event::Heard(number));
    }

    // ------------------------------------------------------------------ perception

    pub fn observe(&self, state: &TableState) {
        let observation = TableObservation {
            count: state.cube_count,
            hands_present: state.hands_present,
            is_stable: state.is_stable,
            finger_count: state.finger_count,
            groups: state.groups,
        };
        self.observe_table(observation);
    }

    pub fn observe_table(&self, observation: TableObservation) {
        self.inner.sense_sound();
        {
            let mut state = self.inner.state.lock();
            state.finger_count = observation.finger_count;
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
            let effects = state.strategy.handle(event);
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
                if let Some(speech_in) = &self.speech_in {
                    speech_in.tutor_speaking(&text, self.config.language);
                }
                let out = Arc::clone(&self.speech_out.read());
                if let Err(error) = out.speak(&text, self.config.language) {
                    self.log(&format!("voice failed: {error}"));
                }
                if let Some(speech_in) = &self.speech_in {
                    speech_in.tutor_silent();
                }
                let mut state = self.state.lock();
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

    // ------------------------------------------------------------------ listening

    fn begin_listening(self: &Inner, mode: ListenMode, timeout: f64) {
        self.cancel_listening();
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
                    .spawn(move || this.listen(speech_in, timeout, token))
                    .expect("spawn");
            }
        }
        if mode == ListenMode::VoiceOrFingers {
            let token = Arc::clone(&token);
            thread::Builder::new().name("un-fingers".into()).spawn(move || this.watch_fingers(token)).expect("spawn");
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
    /// first number ends it as `Heard`.
    fn listen(&self, speech_in: Arc<dyn SpeechInput>, timeout: f64, token: Arc<CancelToken>) {
        if self.options.chime {
            self.speech_out.read().play_chime();
        }
        let language = self.config.language;
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        loop {
            if token.is_cancelled() {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let result = if remaining.is_zero() {
                Err(SpeechError::Timeout { last_transcript: None })
            } else {
                speech_in.listen_for_text(language, remaining, &token, &|_| true)
            };
            match result {
                Ok(text) => {
                    if token.is_cancelled() {
                        return;
                    }
                    let numbers = NumberParser::all(&text, language);
                    match numbers.last().copied() {
                        Some(value) => {
                            self.log(&format!("heard {value} ({text:?})"));
                            self.finish_listening(Some(&token), false);
                            self.dispatch(Event::Numbers(numbers));
                            self.dispatch(Event::Heard(value));
                            return;
                        }
                        None => {
                            self.log(&format!("child: {text}"));
                            self.dispatch(Event::Said(text));
                        }
                    }
                }
                Err(SpeechError::Cancelled) => return,
                Err(error) => {
                    if token.is_cancelled() {
                        return;
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

    fn watch_fingers(&self, token: Arc<CancelToken>) {
        let mut candidate: Option<u32> = None;
        let mut since = Instant::now();
        while !token.wait(Duration::from_millis(100)) {
            let fingers = self.state.lock().finger_count;
            if fingers != candidate {
                candidate = fingers;
                since = Instant::now();
            } else if let Some(fingers) = fingers {
                if since.elapsed() > Duration::from_millis(1500) {
                    self.log(&format!("fingers: {fingers}"));
                    self.finish_listening(Some(&token), false);
                    self.dispatch(Event::Heard(fingers));
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
        if let Some(speech_in) = &self.speech_in {
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
