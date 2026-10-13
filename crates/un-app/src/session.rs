//! Building a session: strategy, speech and engine from plain options.

use std::sync::Arc;

use thiserror::Error;
use un_core::strategy::{SessionConfig, StrategyRegistry};
use un_core::Language;
use un_engine::doubles::{ScriptedSpeechInput, SilentSpeechOutput};
use un_engine::{Engine, EngineOptions, SpeechInput, SpeechOutput};

use crate::paths::ModelPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MicChoice {
    /// The microphone when speech models are installed, else the number pad only.
    #[default]
    Auto,
    Mic,
    Off,
}

impl MicChoice {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "auto" => Self::Auto,
            "mic" | "on" => Self::Mic,
            "off" | "none" => Self::Off,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceChoice {
    /// Piper voices when installed, else the console.
    #[default]
    Auto,
    Piper,
    /// Print the tutor's lines, paced like speech.
    Console,
    Off,
}

impl VoiceChoice {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "auto" => Self::Auto,
            "piper" => Self::Piper,
            "console" => Self::Console,
            "off" | "none" => Self::Off,
            _ => return None,
        })
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("unknown strategy {0:?}; available: {1}")]
    UnknownStrategy(String, String),
    #[error("{0}")]
    Speech(#[from] un_engine::SpeechError),
}

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub strategy: String,
    pub config: SessionConfig,
    pub mic: MicChoice,
    pub mic_device: Option<String>,
    pub voice: VoiceChoice,
    /// Scripted spoken answers (demos and tests); replaces the microphone.
    pub answers: Option<Vec<u32>>,
    pub chime: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            strategy: un_strategy_demo::ID.to_string(),
            config: SessionConfig::default(),
            mic: MicChoice::Auto,
            mic_device: None,
            voice: VoiceChoice::Auto,
            answers: None,
            chime: true,
        }
    }
}

/// Every strategy linked into this binary, the demo included.
pub fn registry() -> StrategyRegistry {
    let mut registry = StrategyRegistry::linked();
    un_strategy_demo::register(&mut registry);
    registry
}

/// A speech input (or none: number pad only) and a speech output.
pub type SpeechPair = (Option<Arc<dyn SpeechInput>>, Arc<dyn SpeechOutput>);

/// The microphone and the voice for `language`, per the choices and what is installed.
pub fn speech_setup(
    paths: &ModelPaths,
    language: Language,
    mic: MicChoice,
    mic_device: Option<&str>,
    voice: VoiceChoice,
    answers: Option<Vec<u32>>,
) -> SpeechPair {
    let models = paths.speech();
    let speech_out: Arc<dyn SpeechOutput> = match voice {
        VoiceChoice::Off => Arc::new(SilentSpeechOutput::new(false)),
        VoiceChoice::Console => Arc::new(SilentSpeechOutput::paced(true)),
        VoiceChoice::Piper => Arc::new(un_speech::SpeakerOutput::new(models.clone())),
        VoiceChoice::Auto => {
            if models.voice_installed(language) {
                Arc::new(un_speech::SpeakerOutput::new(models.clone()))
            } else {
                Arc::new(SilentSpeechOutput::paced(true))
            }
        }
    };
    let speech_in: Option<Arc<dyn SpeechInput>> = if let Some(answers) = answers {
        Some(Arc::new(ScriptedSpeechInput::new(answers, std::time::Duration::from_millis(500))))
    } else {
        match mic {
            MicChoice::Off => None,
            MicChoice::Auto if !models.recognizer_installed(language) => None,
            _ => {
                let device = match mic_device {
                    Some(name) => un_speech::audio::MicDevice::Named(name.to_string()),
                    None => un_speech::audio::MicDevice::Default,
                };
                Some(Arc::new(un_speech::MicrophoneInput::new(models, device)))
            }
        }
    };
    (speech_in, speech_out)
}

/// A built session: the engine plus what it was built from.
pub struct Session {
    pub engine: Engine,
    pub options: SessionOptions,
    pub speech_available: bool,
}

pub fn build_session(
    paths: &ModelPaths,
    options: SessionOptions,
    log: Option<un_engine::Logger>,
) -> Result<Session, SessionError> {
    let registry = registry();
    let strategy = registry.create(&options.strategy, &options.config).ok_or_else(|| {
        SessionError::UnknownStrategy(
            options.strategy.clone(),
            registry.infos().iter().map(|i| i.id.clone()).collect::<Vec<_>>().join(", "),
        )
    })?;
    let (speech_in, speech_out) = speech_setup(
        paths,
        options.config.language,
        options.mic,
        options.mic_device.as_deref(),
        options.voice,
        options.answers.clone(),
    );
    let speech_available = speech_in.is_some();
    let engine = Engine::new(
        strategy,
        options.config.clone(),
        speech_in,
        speech_out,
        EngineOptions { chime: options.chime, always_show_number_pad: false, log },
    );
    Ok(Session { engine, options, speech_available })
}
