//! Speech doubles for tests, demos and headless runs.

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use un_core::Language;

use crate::speech::{CancelToken, NumberHeard, SpeechError, SpeechInput, SpeechOutput};

/// Each listen consumes the next scripted answer; an `Err` entry is raised. When the script is
/// exhausted, listening times out.
pub struct ScriptedSpeechInput {
    answers: Mutex<Vec<Result<u32, SpeechError>>>,
    delay: Duration,
    prepared: Mutex<Vec<Language>>,
    cancel: Mutex<Option<u64>>,
    fail_prepare: Option<String>,
}

impl ScriptedSpeechInput {
    pub fn new(answers: Vec<u32>, delay: Duration) -> Self {
        Self {
            answers: Mutex::new(answers.into_iter().map(Ok).collect()),
            delay,
            prepared: Mutex::new(Vec::new()),
            cancel: Mutex::new(None),
            fail_prepare: None,
        }
    }

    /// A full script: `Ok(n)` is heard, `Err(e)` is raised at once (for example a `Timeout`).
    pub fn with_script(script: Vec<Result<u32, SpeechError>>, delay: Duration) -> Self {
        Self { answers: Mutex::new(script), ..Self::new(vec![], delay) }
    }

    /// A double whose `prepare` fails, like a missing microphone.
    pub fn broken(reason: &str) -> Self {
        let mut s = Self::new(vec![], Duration::ZERO);
        s.fail_prepare = Some(reason.to_string());
        s
    }

    pub fn push(&self, answer: u32) {
        self.answers.lock().push(Ok(answer));
    }

    pub fn prepared(&self) -> Vec<Language> {
        self.prepared.lock().clone()
    }

    fn wait(&self, duration: Duration, cancel: &CancelToken) -> Result<(), SpeechError> {
        if cancel.wait(duration) {
            Err(SpeechError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl SpeechInput for ScriptedSpeechInput {
    fn prepare(&self, language: Language) -> Result<(), SpeechError> {
        if let Some(reason) = &self.fail_prepare {
            return Err(SpeechError::AudioEngineFailed(reason.clone()));
        }
        self.prepared.lock().push(language);
        Ok(())
    }

    fn listen_for_number(
        &self,
        _language: Language,
        timeout: Duration,
        cancel: &CancelToken,
    ) -> Result<NumberHeard, SpeechError> {
        *self.cancel.lock() = Some(cancel.generation);
        if !self.delay.is_zero() {
            self.wait(self.delay, cancel)?;
        }
        let next = {
            let mut answers = self.answers.lock();
            if answers.is_empty() {
                None
            } else {
                Some(answers.remove(0))
            }
        };
        match next {
            None => {
                // Script exhausted: behave like a quiet room and honour the timeout.
                self.wait(timeout, cancel)?;
                Err(SpeechError::Timeout { last_transcript: None })
            }
            Some(Err(error)) => Err(error),
            Some(Ok(value)) => Ok(NumberHeard { value, transcript: value.to_string() }),
        }
    }

    fn listen_for_text(
        &self,
        language: Language,
        timeout: Duration,
        cancel: &CancelToken,
        _accept: &dyn Fn(&str) -> bool,
    ) -> Result<String, SpeechError> {
        self.listen_for_number(language, timeout, cancel).map(|h| h.transcript)
    }

    fn stop(&self) {}
}

/// Records what the tutor says, optionally pacing it like real speech.
pub struct SilentSpeechOutput {
    pub spoken: Mutex<Vec<String>>,
    pub seconds_per_char: f64,
    echo: bool,
    stop: CancelToken,
}

impl SilentSpeechOutput {
    /// `echo` prints the lines to stderr.
    pub fn new(echo: bool) -> Self {
        Self { spoken: Mutex::new(Vec::new()), seconds_per_char: 0.0, echo, stop: CancelToken::default() }
    }

    /// Like [`SilentSpeechOutput::new`] but takes as long as speaking would (0.045 s per character).
    pub fn paced(echo: bool) -> Self {
        Self { seconds_per_char: 0.045, ..Self::new(echo) }
    }

    pub fn spoken_texts(&self) -> Vec<String> {
        self.spoken.lock().clone()
    }

    pub fn spoken_joined(&self) -> String {
        self.spoken.lock().join(" ")
    }
}

impl SpeechOutput for SilentSpeechOutput {
    fn prepare(&self, _language: Language) -> Result<(), SpeechError> {
        Ok(())
    }

    fn speak(&self, text: &str, _language: Language) -> Result<(), SpeechError> {
        self.spoken.lock().push(text.to_string());
        if self.echo {
            eprintln!("[tutor] {text}");
        }
        let pace = Duration::from_secs_f64(self.seconds_per_char * text.chars().count() as f64);
        if !pace.is_zero() {
            let started = Instant::now();
            let token = CancelToken::default();
            while started.elapsed() < pace {
                if self.stop.is_cancelled() {
                    break;
                }
                token.wait(Duration::from_millis(20).min(pace - started.elapsed()));
            }
        }
        Ok(())
    }

    fn stop(&self) {
        self.stop.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_answers_then_timeout() {
        let input = ScriptedSpeechInput::new(vec![3], Duration::ZERO);
        let cancel = CancelToken::default();
        assert_eq!(input.listen_for_number(Language::English, Duration::from_millis(10), &cancel).unwrap().value, 3);
        assert!(matches!(
            input.listen_for_number(Language::English, Duration::from_millis(10), &cancel),
            Err(SpeechError::Timeout { .. })
        ));
        cancel.cancel();
        assert!(matches!(
            input.listen_for_number(Language::English, Duration::from_secs(5), &cancel),
            Err(SpeechError::Cancelled)
        ));
    }
}
