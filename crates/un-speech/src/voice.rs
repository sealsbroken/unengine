//! Piper voices through sherpa-onnx's VITS runtime.

use parking_lot::Mutex;
use sherpa_rs::tts::{VitsTts, VitsTtsConfig};
use sherpa_rs::OnnxConfig;
use un_core::Language;
use un_engine::SpeechError;

use crate::models::{require, SpeechModels};

pub struct Synthesized {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

pub struct Voice {
    pub language: Language,
    tts: Mutex<VitsTts>,
}

impl Voice {
    pub fn load(models: &SpeechModels, language: Language) -> Result<Self, SpeechError> {
        let files = models.voice(language);
        let config = VitsTtsConfig {
            model: require(&files.model)?,
            tokens: require(&files.tokens)?,
            data_dir: require(&files.data_dir)?,
            lexicon: String::new(),
            dict_dir: String::new(),
            length_scale: 1.0,
            noise_scale: 0.667,
            noise_scale_w: 0.8,
            silence_scale: 1.0,
            onnx_config: OnnxConfig { provider: "cpu".into(), debug: false, num_threads: 2 },
            tts_config: Default::default(),
        };
        Ok(Self { language, tts: Mutex::new(VitsTts::new(config)) })
    }

    /// Synthesises `text`; `speed` 1.0 is the voice's natural pace, below 1 is slower.
    pub fn synthesize(&self, text: &str, speed: f32) -> Result<Synthesized, SpeechError> {
        let audio = self.tts.lock().create(text, 0, speed).map_err(|e| SpeechError::AudioEngineFailed(format!("voice: {e}")))?;
        if audio.samples.is_empty() {
            return Err(SpeechError::AudioEngineFailed("voice produced no audio".into()));
        }
        Ok(Synthesized { samples: audio.samples, sample_rate: audio.sample_rate })
    }
}
