//! Un's voice engine, on sherpa-onnx.
//!
//! * [`Recognizer`]: offline Zipformer transducers (EN, RU) cut into utterances by Silero VAD.
//! * [`Voice`]: Piper voices through sherpa-onnx's VITS runtime (no Python, no espeak binary).
//! * [`chime`]: the "listen now" tone, synthesised.
//! * With the `audio` feature: [`MicrophoneInput`] and [`SpeakerOutput`], the
//!   [`un_engine::SpeechInput`] / [`un_engine::SpeechOutput`] implementations for desktops.
//! * [`check`]: a round trip without any audio device (the voice says a number, the recogniser
//!   hears it), for installers and smoke tests.
//!
//! Models are laid out by `tools/fetch_models.py`; see [`SpeechModels`].

pub mod chime;
pub mod models;
pub mod recognizer;
pub mod voice;

#[cfg(feature = "audio")]
pub mod audio;

pub use models::SpeechModels;
pub use recognizer::Recognizer;
pub use voice::Voice;

#[cfg(feature = "audio")]
pub use audio::{MicrophoneInput, SpeakerOutput};

use un_core::{Language, NumberParser};
use un_engine::SpeechError;

pub const SAMPLE_RATE: u32 = 16_000;

/// What `check_speech` says, per language: a number the lesson would accept.
pub fn check_phrase(language: Language) -> &'static str {
    match language {
        Language::English => "three",
        Language::Russian => "три",
    }
}

/// Round trip without any audio device: the voice says a number, VAD and the recogniser hear it.
/// Returns `(said, heard)`; errors name whatever model or library is missing.
pub fn check(models: &SpeechModels, language: Language) -> Result<(String, String), SpeechError> {
    let said = check_phrase(language);
    let voice = Voice::load(models, language)?;
    let audio = voice.synthesize(said, 1.0)?;
    let samples = resample(&audio.samples, audio.sample_rate, SAMPLE_RATE);
    let recognizer = Recognizer::load(models, language)?;
    let mut padded = vec![0.0f32; SAMPLE_RATE as usize / 2];
    padded.extend_from_slice(&samples);
    padded.extend(std::iter::repeat_n(0.0, SAMPLE_RATE as usize));
    let segments = recognizer.segments(&padded)?;
    let heard = if segments.is_empty() { recognizer.decode(&samples)? } else { segments.join(" ") };
    Ok((said.to_string(), heard))
}

/// `check` plus the verdict: did the recogniser hear a three? The voice is random (VITS noise), and
/// a lone word is sometimes misheard ("three" as "read"), so it gets three tries.
pub fn check_ok(models: &SpeechModels, language: Language) -> Result<(String, String, bool), SpeechError> {
    let mut last = check(models, language)?;
    for _ in 0..2 {
        if NumberParser::parse(&last.1, language) == Some(3) {
            break;
        }
        last = check(models, language)?;
    }
    let ok = NumberParser::parse(&last.1, language) == Some(3);
    Ok((last.0, last.1, ok))
}

/// Linear resampling; good enough for speech going into a recogniser.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let count = (samples.len() as f64 / ratio).floor() as usize;
    (0..count)
        .map(|i| {
            let pos = i as f64 * ratio;
            let index = pos.floor() as usize;
            let frac = (pos - index as f64) as f32;
            let a = samples[index.min(samples.len() - 1)];
            let b = samples[(index + 1).min(samples.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_halves() {
        let s: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = resample(&s, 2, 1);
        assert_eq!(out.len(), 5);
        assert_eq!(out[1], 2.0);
    }
}
