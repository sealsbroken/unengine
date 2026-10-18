//! The Un engine for Swift and Kotlin, through UniFFI.
//!
//! Phones keep their own camera and speech APIs and hand the engine what it needs:
//!
//! * perception: either call [`UnSession::observe`] with the platform's own counts, or run the
//!   shared detector with [`UnPerception`] and pass frames;
//! * speech: implement [`SpeechInputCallback`] / [`SpeechOutputCallback`] over the platform's
//!   recogniser and synthesiser (sherpa-onnx's own Swift/Kotlin packages work too).
//!
//! Build the library for a target and generate bindings in library mode:
//!
//! ```text
//! cargo build -p un-ffi --release --target aarch64-apple-ios
//! cargo run -p un-ffi --bin uniffi-bindgen -- generate --library target/aarch64-apple-ios/release/libun_ffi.a \
//!     --language swift --out-dir apps/ios/Generated
//! ```
//!
//! A product that adds proprietary strategies links this crate plus the strategy crates into its
//! own `cdylib`/`staticlib` (see the `unmethod` repository's `apps/mobile`); the strategies register
//! themselves at link time.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use un_core::strategy::{SessionConfig, StrategyInfo, StrategyRegistry, TableObservation};
use un_core::{EstimatorConfig, Language};
use un_engine::{CancelToken, Engine, EngineOptions, NumberHeard, SpeechError, SpeechInput, SpeechOutput};
use un_vision::RgbFrame;

uniffi::setup_scaffolding!();

/// The field is `reason`, not `message`: in Kotlin the error is a `Throwable`, which has a
/// `message` of its own.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum UnError {
    #[error("{reason}")]
    Failed { reason: String },
}

impl From<String> for UnError {
    fn from(reason: String) -> Self {
        UnError::Failed { reason }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UnLanguage {
    English,
    Russian,
}

impl From<UnLanguage> for Language {
    fn from(l: UnLanguage) -> Self {
        match l {
            UnLanguage::English => Language::English,
            UnLanguage::Russian => Language::Russian,
        }
    }
}

impl From<Language> for UnLanguage {
    fn from(l: Language) -> Self {
        match l {
            Language::English => UnLanguage::English,
            Language::Russian => UnLanguage::Russian,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnStrategyInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<UnLanguage>,
    pub license: String,
}

impl From<StrategyInfo> for UnStrategyInfo {
    fn from(i: StrategyInfo) -> Self {
        Self {
            id: i.id,
            title: i.title,
            description: i.description,
            languages: i.languages.into_iter().map(Into::into).collect(),
            license: i.license,
        }
    }
}

type Registration = fn(&mut StrategyRegistry);

static REGISTRATIONS: Mutex<Vec<Registration>> = Mutex::new(Vec::new());

/// Adds a strategy crate's `register` function to every registry this library builds.
///
/// Strategies also register themselves at link time, but a static library only keeps the
/// objects something refers to, so a product library calls this from a function the app
/// calls at startup (`unmethod_mobile_init`, say). Registering twice is harmless.
pub fn add_strategies(register: Registration) {
    let mut registrations = REGISTRATIONS.lock();
    if !registrations.contains(&register) {
        registrations.push(register);
    }
}

/// Makes `lessons` the only strategies this library offers (no linked strategies, no demo): a
/// product library calls it from its startup function, before listing or creating sessions.
pub fn set_registry(lessons: fn() -> StrategyRegistry) {
    un_app::set_registry(lessons);
}

/// Every strategy linked into this library (or the product's list, see [`set_registry`]) plus
/// those added with [`add_strategies`].
pub fn registry() -> StrategyRegistry {
    let mut registry = un_app::registry();
    for register in REGISTRATIONS.lock().iter() {
        register(&mut registry);
    }
    registry
}

/// Every strategy this library knows.
#[uniffi::export]
pub fn available_strategies() -> Vec<UnStrategyInfo> {
    registry().infos().into_iter().map(Into::into).collect()
}

#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnSessionConfig {
    pub language: UnLanguage,
    pub child_name: Option<String>,
    pub total_objects: u32,
    pub max_tasks: u32,
    pub start_level: u32,
    pub answer_timeout: f64,
    pub perform_timeout: f64,
    pub nudge_interval: f64,
    pub seed: u64,
    pub noun: String,
    /// Strategy-specific settings, including what an earlier session asked to `remember`.
    pub extra: HashMap<String, String>,
}

#[uniffi::export]
pub fn default_session_config() -> UnSessionConfig {
    SessionConfig::default().into()
}

impl From<UnSessionConfig> for SessionConfig {
    fn from(c: UnSessionConfig) -> Self {
        SessionConfig {
            language: c.language.into(),
            child_name: c.child_name,
            total_objects: c.total_objects,
            max_tasks: c.max_tasks,
            start_level: c.start_level,
            answer_timeout: c.answer_timeout,
            perform_timeout: c.perform_timeout,
            nudge_interval: c.nudge_interval,
            seed: c.seed,
            noun: c.noun,
            extra: c.extra,
        }
    }
}

impl From<SessionConfig> for UnSessionConfig {
    fn from(c: SessionConfig) -> Self {
        Self {
            language: c.language.into(),
            child_name: c.child_name,
            total_objects: c.total_objects,
            max_tasks: c.max_tasks,
            start_level: c.start_level,
            answer_timeout: c.answer_timeout,
            perform_timeout: c.perform_timeout,
            nudge_interval: c.nudge_interval,
            seed: c.seed,
            noun: c.noun,
            extra: c.extra,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UnListenMode {
    Voice,
    NumberPad,
    Counting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UnStatus {
    Idle,
    Starting,
    Watching,
    Speaking,
    Listening,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UnPhase {
    Idle,
    Meeting,
    AwaitingObjects,
    Performing,
    Asking,
    Paused,
    Finished,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnTaskRecord {
    pub task: String,
    pub answers: Vec<u32>,
    pub expected: Option<u32>,
    pub correct: bool,
    pub skipped: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnSummary {
    pub tasks_completed: u32,
    pub correct: u32,
    pub final_level: u32,
    pub records: Vec<UnTaskRecord>,
    /// Stopped early; `remember` holds what the next session needs to continue.
    pub paused: bool,
    /// Store it and pass it back as `UnSessionConfig::extra` (an empty value forgets the key).
    pub remember: HashMap<String, String>,
}

/// Everything the HUD needs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct UnSnapshot {
    pub status: UnStatus,
    pub listen_mode: Option<UnListenMode>,
    pub phase: UnPhase,
    pub level: u32,
    pub level_count: u32,
    pub tasks_completed: u32,
    pub correct: u32,
    pub task: Option<String>,
    pub target_count: Option<u32>,
    pub last_spoken: String,
    pub highlight_count: Option<u32>,
    pub celebrated_seconds_ago: Option<f64>,
    pub summary: Option<UnSummary>,
    pub warning: Option<String>,
    pub show_number_pad: bool,
    pub speech_available: bool,
    pub mic_level: f32,
    /// The lesson is over and the child said yes to "Продолжим?": start the next lesson.
    pub next_lesson_requested: bool,
    /// The child said "пауза" (or "хватит", "enough"...): go back to the lessons once the
    /// tutor has said goodbye (`status` finished).
    pub pause_requested: bool,
    /// The child said no to "Продолжим?" (or it was tapped): show the goodbye.
    pub next_lesson_declined: bool,
    /// What the microphone heard most recently, for a few seconds ("Heard: «три!»").
    pub heard_text: String,
}

impl From<un_engine::EngineSnapshot> for UnSnapshot {
    fn from(s: un_engine::EngineSnapshot) -> Self {
        use un_core::strategy::{ListenMode, PhaseKind};
        use un_engine::StatusKind;
        Self {
            status: match s.status.kind {
                StatusKind::Idle => UnStatus::Idle,
                StatusKind::Starting => UnStatus::Starting,
                StatusKind::Watching => UnStatus::Watching,
                StatusKind::Speaking => UnStatus::Speaking,
                StatusKind::Listening => UnStatus::Listening,
                StatusKind::Finished => UnStatus::Finished,
            },
            listen_mode: s.status.listen_mode.map(|m| match m {
                ListenMode::Voice => UnListenMode::Voice,
                ListenMode::NumberPad => UnListenMode::NumberPad,
                ListenMode::Counting => UnListenMode::Counting,
            }),
            phase: match s.strategy.phase {
                PhaseKind::Idle => UnPhase::Idle,
                PhaseKind::Meeting => UnPhase::Meeting,
                PhaseKind::AwaitingObjects => UnPhase::AwaitingObjects,
                PhaseKind::Performing => UnPhase::Performing,
                PhaseKind::Asking => UnPhase::Asking,
                PhaseKind::Paused => UnPhase::Paused,
                PhaseKind::Finished => UnPhase::Finished,
            },
            level: s.strategy.level,
            level_count: s.strategy.level_count,
            tasks_completed: s.strategy.tasks_completed,
            correct: s.strategy.correct,
            task: s.strategy.task,
            target_count: s.strategy.target_count,
            last_spoken: s.last_spoken,
            highlight_count: s.highlight_count,
            celebrated_seconds_ago: s.celebrated_seconds_ago,
            summary: s.summary.map(|m| UnSummary {
                tasks_completed: m.tasks_completed,
                correct: m.correct,
                final_level: m.final_level,
                records: m
                    .records
                    .into_iter()
                    .map(|r| UnTaskRecord {
                        task: r.task,
                        answers: r.answers,
                        expected: r.expected,
                        correct: r.correct,
                        skipped: r.skipped,
                    })
                    .collect(),
                paused: m.paused,
                remember: m.remember,
            }),
            warning: s.warning,
            show_number_pad: s.show_number_pad,
            speech_available: s.speech_available,
            mic_level: s.mic_level,
            next_lesson_requested: s.next_lesson_requested,
            pause_requested: s.pause_requested,
            next_lesson_declined: s.next_lesson_declined,
            heard_text: s.heard_text,
        }
    }
}

// ------------------------------------------------------------------ platform speech

/// Implemented by the platform over its recogniser. `listen_for_number` must block until it has
/// a number, the timeout passes (return `None`), or `cancel_listening` was called. The engine
/// parses the transcript itself, so a platform may return any words with `value` 0: the
/// strategy then hears them as speech ("yes", a name) rather than a number.
#[uniffi::export(with_foreign)]
pub trait SpeechInputCallback: Send + Sync {
    fn prepare(&self, language: UnLanguage) -> Result<(), UnError>;
    /// Listens until a number is heard; `None` on timeout or cancellation.
    fn listen_for_number(&self, language: UnLanguage, timeout_seconds: f64) -> Option<UnHeard>;
    /// Listens for whatever the child says (a name, "yes", "I don't understand"), returning the
    /// transcript once the child has finished the utterance; `None` when nothing was said
    /// before the timeout or listening was cancelled.
    fn listen_for_text(&self, language: UnLanguage, timeout_seconds: f64) -> Option<String>;
    fn cancel_listening(&self);
    /// The microphone stays open while the tutor speaks, the tutor's voice taken out of it
    /// (echo cancellation): the child may answer over a question (iOS).
    fn barge_in(&self) -> bool;
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnHeard {
    pub value: u32,
    pub transcript: String,
}

/// Implemented by the platform over its synthesiser. `speak` must block until the utterance has
/// been played or `stop_speaking` was called.
#[uniffi::export(with_foreign)]
pub trait SpeechOutputCallback: Send + Sync {
    fn prepare(&self, language: UnLanguage) -> Result<(), UnError>;
    fn speak(&self, text: String, language: UnLanguage) -> Result<(), UnError>;
    fn stop_speaking(&self);
    fn play_chime(&self);
}

struct CallbackInput(Arc<dyn SpeechInputCallback>);

impl SpeechInput for CallbackInput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        self.0.prepare(language.into()).map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))
    }

    fn listen_for_number(&self, language: Language, timeout: Duration, cancel: &CancelToken) -> Result<NumberHeard, SpeechError> {
        match self.0.listen_for_number(language.into(), timeout.as_secs_f64()) {
            Some(heard) if !cancel.is_cancelled() => Ok(NumberHeard { value: heard.value, transcript: heard.transcript }),
            Some(_) => Err(SpeechError::Cancelled),
            None if cancel.is_cancelled() => Err(SpeechError::Cancelled),
            None => Err(SpeechError::Timeout { last_transcript: None }),
        }
    }

    fn listen_for_text(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<String, SpeechError> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(SpeechError::Timeout { last_transcript: None });
            }
            let heard = self.0.listen_for_text(language.into(), remaining.as_secs_f64());
            if cancel.is_cancelled() {
                return Err(SpeechError::Cancelled);
            }
            match heard {
                Some(text) if accept(&text) => return Ok(text),
                Some(_) => continue,
                None => return Err(SpeechError::Timeout { last_transcript: None }),
            }
        }
    }

    fn stop(&self) {
        self.0.cancel_listening();
    }

    /// The iOS and Android inputs recognise nothing while the tutor speaks (the engine stops
    /// them for every line, and iOS drops the microphone's sound until just after it).
    fn hears_tutor(&self) -> bool {
        false
    }

    fn barge_in(&self) -> bool {
        self.0.barge_in()
    }
}

struct CallbackOutput(Arc<dyn SpeechOutputCallback>);

impl SpeechOutput for CallbackOutput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        self.0.prepare(language.into()).map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))
    }

    fn speak(&self, text: &str, language: Language) -> Result<(), SpeechError> {
        self.0.speak(text.to_string(), language.into()).map_err(|e| SpeechError::AudioEngineFailed(e.to_string()))
    }

    fn stop(&self) {
        self.0.stop_speaking();
    }

    fn play_chime(&self) {
        self.0.play_chime();
    }
}

// ------------------------------------------------------------------ session

/// One lesson: a strategy driven by the engine.
#[derive(uniffi::Object)]
pub struct UnSession {
    engine: Engine,
}

#[uniffi::export]
impl UnSession {
    /// `speech_input` may be `None`: the lesson then uses the number pad only. `always_listen`
    /// keeps the microphone open for the whole lesson (closed only while the tutor speaks), so
    /// the child can talk at any time.
    #[uniffi::constructor]
    pub fn new(
        strategy_id: String,
        config: UnSessionConfig,
        speech_input: Option<Arc<dyn SpeechInputCallback>>,
        speech_output: Arc<dyn SpeechOutputCallback>,
        chime: bool,
        always_listen: bool,
    ) -> Result<Arc<Self>, UnError> {
        let registry = registry();
        let config: SessionConfig = config.into();
        let strategy = registry.create(&strategy_id, &config).ok_or_else(|| format!("unknown strategy {strategy_id:?}"))?;
        let speech_in: Option<Arc<dyn SpeechInput>> = speech_input.map(|cb| Arc::new(CallbackInput(cb)) as Arc<dyn SpeechInput>);
        let speech_out: Arc<dyn SpeechOutput> = Arc::new(CallbackOutput(speech_output));
        let engine = Engine::new(
            strategy,
            config,
            speech_in,
            speech_out,
            EngineOptions {
                chime,
                always_listen,
                // The engine's one-line log (what the tutor said, what it heard) on stderr: the Xcode console, `devicectl ... --console`, logcat.
                log: Some(Arc::new(|line: &str| eprintln!("un: {line}"))),
                ..Default::default()
            },
        );
        Ok(Arc::new(Self { engine }))
    }

    pub fn start(&self) {
        self.engine.start();
    }

    pub fn stop(&self) {
        self.engine.stop();
    }

    /// Tears the session down. Not `close`: a Kotlin object has an `AutoCloseable.close()` of
    /// its own that frees the handle.
    pub fn shutdown(&self) {
        self.engine.close();
    }

    pub fn repeat(&self) {
        self.engine.repeat();
    }

    pub fn skip(&self) {
        self.engine.skip();
    }

    pub fn tap(&self, number: u32) {
        self.engine.tap(number);
    }

    /// What the platform's own perception sees. `count` is the committed count, `None` while
    /// unsettled.
    pub fn observe(&self, count: Option<u32>, is_stable: bool, groups: u32) {
        self.engine.observe_table(TableObservation { count, is_stable, groups });
    }

    /// There is a lesson after this one: once this one is completed the tutor asks
    /// "Продолжим?", and a yes sets `next_lesson_requested` in the snapshot.
    pub fn offer_next_lesson(&self, offer: bool) {
        self.engine.offer_next_lesson(offer);
    }

    /// The yes/no to "Продолжим?" tapped on screen instead of said.
    pub fn answer_next_lesson(&self, yes: bool) {
        self.engine.answer_next_lesson(yes);
    }

    pub fn snapshot(&self) -> UnSnapshot {
        self.engine.snapshot().into()
    }
}

// ------------------------------------------------------------------ shared vision

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnBox {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnDetection {
    pub bbox: UnBox,
    pub confidence: f32,
    pub label: Option<String>,
    /// Stable 1-based number, 0 when not yet numbered.
    pub number: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnTableState {
    pub count: Option<u32>,
    pub raw_count: u32,
    pub is_stable: bool,
    /// Separate groups the objects lie in.
    pub groups: u32,
    pub detections: Vec<UnDetection>,
    pub timestamp: f64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UnPerceptionConfig {
    pub models_dir: String,
    /// Detector name in `models_dir` (`toys`) or a path to an `.onnx`.
    pub detector: String,
    pub confidence: f32,
    pub count_label: Option<String>,
    /// Table area as normalized x1,y1,x2,y2; empty means the whole frame.
    pub roi: Option<UnBox>,
}

/// The shared detector, estimator and tracker. Feed it RGB8 frames.
#[derive(uniffi::Object)]
pub struct UnPerception {
    inner: Mutex<un_app::Perception>,
}

#[uniffi::export]
impl UnPerception {
    #[uniffi::constructor]
    pub fn new(config: UnPerceptionConfig) -> Result<Arc<Self>, UnError> {
        let paths = un_app::ModelPaths::new(config.models_dir);
        let estimator =
            EstimatorConfig { roi: config.roi.map(|b| un_core::Box2::new(b.x1, b.y1, b.x2, b.y2)), ..Default::default() };
        let perception = un_app::Perception::load(
            &paths,
            &un_app::PerceptionConfig {
                detector: config.detector,
                confidence: config.confidence,
                provider: un_vision::Provider::Auto,
                count_label: config.count_label,
                estimator,
            },
        )
        .map_err(|e| e.to_string())?;
        Ok(Arc::new(Self { inner: Mutex::new(perception) }))
    }

    pub fn labels(&self) -> Vec<String> {
        self.inner.lock().labels.clone()
    }

    pub fn set_roi(&self, roi: Option<UnBox>) {
        self.inner.lock().estimator.set_roi(roi.map(|b| un_core::Box2::new(b.x1, b.y1, b.x2, b.y2)));
    }

    /// `rgb` is width*height*3 bytes, row-major. `timestamp` in seconds, increasing.
    pub fn process_rgb(&self, rgb: Vec<u8>, width: u32, height: u32, timestamp: f64) -> Result<UnTableState, UnError> {
        if rgb.len() != (width * height * 3) as usize {
            return Err(format!("expected {} bytes of RGB8, got {}", width * height * 3, rgb.len()).into());
        }
        let frame = RgbFrame::new(width, height, rgb);
        self.process(&frame, timestamp)
    }

    /// NV12 (Y plane then interleaved UV), the native format of phone cameras.
    #[allow(clippy::too_many_arguments)]
    pub fn process_nv12(
        &self,
        y: Vec<u8>,
        uv: Vec<u8>,
        width: u32,
        height: u32,
        y_stride: u32,
        uv_stride: u32,
        timestamp: f64,
    ) -> Result<UnTableState, UnError> {
        let frame = RgbFrame::from_nv12(width, height, &y, &uv, y_stride as usize, uv_stride as usize);
        self.process(&frame, timestamp)
    }
}

impl UnPerception {
    fn process(&self, frame: &RgbFrame, timestamp: f64) -> Result<UnTableState, UnError> {
        let mut p = self.inner.lock();
        let out = p.process(frame, timestamp).map_err(|e| e.to_string())?;
        let numbered = |d: &un_core::Detection| out.tracked.iter().find(|t| t.detection == *d).map(|t| t.number).unwrap_or(0);
        Ok(UnTableState {
            count: out.state.cube_count,
            raw_count: out.state.raw_count,
            is_stable: out.state.is_stable,
            groups: out.state.groups,
            detections: out
                .state
                .detections
                .iter()
                .map(|d| UnDetection {
                    bbox: UnBox { x1: d.bbox.x1, y1: d.bbox.y1, x2: d.bbox.x2, y2: d.bbox.y2 },
                    confidence: d.confidence,
                    label: d.label.clone(),
                    number: numbered(d),
                })
                .collect(),
            timestamp: out.state.timestamp,
        })
    }
}

// ------------------------------------------------------------------ children's profiles

/// One child: their name and where they are in the lessons.
#[derive(Debug, Clone, uniffi::Record)]
pub struct UnProfile {
    pub id: String,
    pub name: String,
    /// Lessons the child completed, by id.
    pub completed: Vec<String>,
    /// The lesson they go on with next.
    pub current: Option<String>,
    /// The lesson they stopped halfway ("Paused here" on the trail).
    pub paused: Option<String>,
}

impl From<&un_app::Profile> for UnProfile {
    fn from(p: &un_app::Profile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            completed: p.completed.clone(),
            current: p.current.clone(),
            paused: p.paused.clone(),
        }
    }
}

/// The children who use the app and their progress, kept in one JSON file (`path`), shared by
/// the iOS and Android apps.
#[derive(uniffi::Object)]
pub struct UnProfiles {
    inner: Mutex<un_app::Profiles>,
}

fn lesson_ids() -> Vec<String> {
    registry().infos().into_iter().map(|i| i.id).collect()
}

#[uniffi::export]
impl UnProfiles {
    #[uniffi::constructor]
    pub fn new(path: String) -> Arc<Self> {
        Arc::new(Self { inner: Mutex::new(un_app::Profiles::open(path)) })
    }

    pub fn list(&self) -> Vec<UnProfile> {
        self.inner.lock().list().iter().map(UnProfile::from).collect()
    }

    pub fn add(&self, name: String) -> UnProfile {
        UnProfile::from(&self.inner.lock().add(&name))
    }

    pub fn rename(&self, id: String, name: String) {
        self.inner.lock().rename(&id, &name);
    }

    pub fn remove(&self, id: String) {
        self.inner.lock().remove(&id);
    }

    /// `base` for this child: their name and what their lessons remembered.
    pub fn session_config(&self, id: String, base: UnSessionConfig) -> UnSessionConfig {
        self.inner.lock().config(&id, base.into()).into()
    }

    /// A lesson ended for this child (`snapshot.summary`); recording it twice changes nothing.
    pub fn record(&self, id: String, lesson_id: String, summary: UnSummary) {
        let summary =
            un_core::strategy::SessionSummary { paused: summary.paused, remember: summary.remember, ..Default::default() };
        self.inner.lock().record(&id, &lesson_id, &summary, &lesson_ids());
    }

    /// The lesson this child goes on with.
    pub fn next_lesson(&self, id: String) -> Option<String> {
        self.inner.lock().next_lesson(&id, &lesson_ids())
    }
}
