//! Speech abstractions the engine drives. Implementations live in `un-speech` (sherpa-onnx)
//! and in platform shells (Apple Speech, Android SpeechRecognizer...).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use thiserror::Error;
use un_core::Language;

#[derive(Debug, Error)]
pub enum SpeechError {
    #[error("no recogniser for {0}")]
    UnsupportedLanguage(String),
    #[error("speech models are missing: {0}")]
    AssetsNotInstalled(String),
    #[error("audio engine failed: {0}")]
    AudioEngineFailed(String),
    #[error("timeout{}", .last_transcript.as_ref().map(|t| format!(" (last heard: {t:?})")).unwrap_or_default())]
    Timeout { last_transcript: Option<String> },
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberHeard {
    pub value: u32,
    pub transcript: String,
}

/// Cancels one listen. Also usable as a sleep that wakes up on cancellation.
#[derive(Debug)]
pub struct CancelToken {
    pub generation: u64,
    cancelled: AtomicBool,
    lock: Mutex<()>,
    cv: Condvar,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new(0)
    }
}

impl CancelToken {
    pub fn new(generation: u64) -> Self {
        Self { generation, cancelled: AtomicBool::new(false), lock: Mutex::new(()), cv: Condvar::new() }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let _guard = self.lock.lock();
        self.cv.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Waits up to `timeout`; returns true when cancelled.
    pub fn wait(&self, timeout: Duration) -> bool {
        let mut guard = self.lock.lock();
        if self.is_cancelled() {
            return true;
        }
        self.cv.wait_for(&mut guard, timeout);
        self.is_cancelled()
    }
}

/// Speech recognition. Methods take `&self` because `stop` races `listen` from another thread.
pub trait SpeechInput: Send + Sync {
    /// Loads models and checks the microphone. Safe to call repeatedly.
    fn prepare(&self, language: Language) -> Result<(), SpeechError>;

    /// Listens until a number (0...20) is heard, `timeout` elapses, or `cancel` fires.
    fn listen_for_number(&self, language: Language, timeout: Duration, cancel: &CancelToken) -> Result<NumberHeard, SpeechError>;

    /// Listens for any transcript matching `accept`, for questions outside a lesson.
    fn listen_for_text(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        accept: &dyn Fn(&str) -> bool,
    ) -> Result<String, SpeechError>;

    /// Cancels any in-flight listen and releases the microphone.
    fn stop(&self);

    /// The tutor is about to say `text`: an always-on microphone uses it to tell the tutor's own
    /// voice from the child's.
    fn tutor_speaking(&self, _text: &str, _language: Language) {}

    /// Name of the microphone in use, for the HUD.
    fn device_name(&self) -> String {
        String::new()
    }

    /// Smoothed input level 0...1 while listening, for the HUD.
    fn level(&self) -> f32 {
        0.0
    }

    /// What the recogniser heard most recently, for a few seconds, so the HUD can show why an
    /// answer was not taken.
    fn last_transcript(&self) -> String {
        String::new()
    }
}

/// Speech synthesis. `speak` blocks until the utterance has been played or stopped.
pub trait SpeechOutput: Send + Sync {
    fn prepare(&self, language: Language) -> Result<(), SpeechError>;
    fn speak(&self, text: &str, language: Language) -> Result<(), SpeechError>;
    /// Interrupts the current utterance.
    fn stop(&self);
    /// The short "listen now" chime; silent by default.
    fn play_chime(&self) {}
}
