//! Whisper or a Zipformer transducer + Silero VAD: audio in, utterances out.

use parking_lot::Mutex;
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};
use sherpa_rs::whisper::{WhisperConfig, WhisperRecognizer};
use sherpa_rs::zipformer::{ZipFormer, ZipFormerConfig};
use un_core::Language;
use un_engine::SpeechError;

use crate::models::{require, SpeechModels};
use crate::SAMPLE_RATE;

/// Silero VAD window at 16 kHz.
pub const CHUNK: usize = 512;
/// Shorter than this is a click, and the encoder rejects empty input.
pub const MIN_SEGMENT: usize = (0.2 * SAMPLE_RATE as f32) as usize;

enum Backend {
    Zipformer(Mutex<ZipFormer>),
    Whisper(Mutex<WhisperRecognizer>),
}

/// One language's recogniser, plus a fresh VAD per listen.
pub struct Recognizer {
    pub language: Language,
    backend: Backend,
    /// `whisper-small`, `zipformer`, for logs.
    pub name: String,
    vad_model: String,
    pub min_silence: f32,
}

impl Recognizer {
    pub fn load(models: &SpeechModels, language: Language) -> Result<Self, SpeechError> {
        let vad_model = require(&models.vad())?;
        let (backend, name) = match models.whisper() {
            Some(files) => {
                let config = WhisperConfig {
                    encoder: require(&files.encoder)?,
                    decoder: require(&files.decoder)?,
                    tokens: require(&files.tokens)?,
                    language: language.code().to_string(),
                    bpe_vocab: None,
                    tail_paddings: None,
                    provider: Some("cpu".into()),
                    num_threads: Some(4),
                    debug: false,
                };
                let whisper =
                    WhisperRecognizer::new(config).map_err(|e| SpeechError::AudioEngineFailed(format!("whisper: {e}")))?;
                (Backend::Whisper(Mutex::new(whisper)), format!("whisper-{}", files.size))
            }
            None => {
                let files = models.recognizer(language);
                let config = ZipFormerConfig {
                    encoder: require(&files.encoder)?,
                    decoder: require(&files.decoder)?,
                    joiner: require(&files.joiner)?,
                    tokens: require(&files.tokens)?,
                    num_threads: Some(2),
                    provider: Some("cpu".into()),
                    debug: false,
                };
                let zipformer = ZipFormer::new(config).map_err(|e| SpeechError::AudioEngineFailed(format!("recogniser: {e}")))?;
                (Backend::Zipformer(Mutex::new(zipformer)), "zipformer".to_string())
            }
        };
        Ok(Self { language, backend, name, vad_model, min_silence: 0.35 })
    }

    /// A voice activity detector for one listen (it keeps state).
    pub fn vad(&self) -> Result<SileroVad, SpeechError> {
        let config = SileroVadConfig {
            model: self.vad_model.clone(),
            min_silence_duration: self.min_silence,
            min_speech_duration: 0.15,
            max_speech_duration: 6.0,
            threshold: 0.5,
            sample_rate: SAMPLE_RATE,
            window_size: CHUNK as i32,
            provider: Some("cpu".into()),
            num_threads: Some(1),
            debug: false,
        };
        SileroVad::new(config, 30.0).map_err(|e| SpeechError::AudioEngineFailed(format!("vad: {e}")))
    }

    /// Decodes one utterance (16 kHz mono) to text; empty for clicks.
    pub fn decode(&self, samples: &[f32]) -> Result<String, SpeechError> {
        if samples.len() < MIN_SEGMENT {
            return Ok(String::new());
        }
        let text = match &self.backend {
            Backend::Zipformer(z) => z.lock().decode(SAMPLE_RATE, samples.to_vec()),
            Backend::Whisper(w) => w.lock().transcribe(SAMPLE_RATE, samples).text,
        };
        Ok(text.trim().to_string())
    }

    /// Cuts `samples` into utterances with the VAD and decodes each (for files and tests).
    pub fn segments(&self, samples: &[f32]) -> Result<Vec<String>, SpeechError> {
        let mut vad = self.vad()?;
        let mut texts = Vec::new();
        let drain = |vad: &mut SileroVad, texts: &mut Vec<String>| -> Result<(), SpeechError> {
            while !vad.is_empty() {
                let segment = vad.front();
                let text = self.decode(&segment.samples)?;
                if !text.is_empty() {
                    texts.push(text);
                }
                vad.pop();
            }
            Ok(())
        };
        for chunk in samples.chunks(CHUNK) {
            if chunk.len() < CHUNK {
                break;
            }
            vad.accept_waveform(chunk.to_vec());
            drain(&mut vad, &mut texts)?;
        }
        vad.flush();
        drain(&mut vad, &mut texts)?;
        Ok(texts)
    }
}
