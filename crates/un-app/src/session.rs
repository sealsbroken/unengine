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
    /// Scripted lines the child says (demos and tests); replaces the microphone.
    pub answers: Option<Vec<String>>,
    pub chime: bool,
    /// Keep the microphone open for the whole lesson (see `EngineOptions::always_listen`).
    pub always_listen: bool,
    /// Answer free speech with the language model in `models/llm` when one is installed.
    pub chat: bool,
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
            chime: false,
            always_listen: true,
            chat: true,
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

/// Names of the microphones the system offers, in system order.
#[cfg(feature = "speech")]
pub fn list_microphones() -> Vec<String> {
    un_speech::MicrophoneInput::list_devices()
}

/// The microphone to start with when the user has not chosen one: a dedicated microphone (a
/// USB or XLR mic by name) beats the laptop's built-in one, which beats webcams and virtual
/// devices. `None` means the system default is as good as any.
pub fn pick_microphone(names: &[String]) -> Option<String> {
    const GOOD: &[&str] = &[
        "yeti",
        "blue ",
        "rode",
        "røde",
        "shure",
        "audio-technica",
        "at20",
        "samson",
        "fifine",
        "hyperx",
        "elgato",
        "wave",
        "microphone",
        "mic",
    ];
    const BAD: &[&str] = &[
        "macbook",
        "built-in",
        "imac",
        "internal",
        "cam",
        "display",
        "monitor",
        "teams",
        "zoom",
        "virtual",
        "blackhole",
        "loopback",
        "aggregate",
        "soundflower",
        "obs",
    ];
    let score = |name: &str| -> i32 {
        let n = name.to_lowercase();
        let good = GOOD.iter().any(|k| n.contains(k));
        let bad = BAD.iter().any(|k| n.contains(k));
        match (good, bad) {
            (true, false) => 2,
            (true, true) => -1,
            (false, false) => 0,
            (false, true) => -2,
        }
    };
    names.iter().map(|n| (score(n), n)).filter(|(s, _)| *s > 0).max_by_key(|(s, _)| *s).map(|(_, n)| n.clone())
}

/// The microphone and the voice for `language`, per the choices and what is installed.
pub fn speech_setup(
    paths: &ModelPaths,
    language: Language,
    mic: MicChoice,
    mic_device: Option<&str>,
    voice: VoiceChoice,
    answers: Option<Vec<String>>,
) -> SpeechPair {
    let speech_out: Arc<dyn SpeechOutput> = match voice {
        VoiceChoice::Off => Arc::new(SilentSpeechOutput::new(false)),
        VoiceChoice::Console => Arc::new(SilentSpeechOutput::paced(true)),
        VoiceChoice::Piper | VoiceChoice::Auto => {
            engine_voice(paths, language, voice).unwrap_or_else(|| Arc::new(SilentSpeechOutput::paced(true)))
        }
    };
    let speech_in: Option<Arc<dyn SpeechInput>> = if let Some(answers) = answers {
        Some(Arc::new(ScriptedSpeechInput::saying(answers, std::time::Duration::from_millis(500))))
    } else {
        match mic {
            MicChoice::Off => None,
            _ => engine_microphone(paths, language, mic, mic_device),
        }
    };
    (speech_in, speech_out)
}

/// The engine's own voice (Piper): always when asked for, otherwise only when installed.
#[cfg(feature = "speech")]
fn engine_voice(paths: &ModelPaths, language: Language, voice: VoiceChoice) -> Option<Arc<dyn SpeechOutput>> {
    let models = paths.speech();
    (matches!(voice, VoiceChoice::Piper) || models.voice_installed(language))
        .then(|| Arc::new(un_speech::SpeakerOutput::new(models)) as Arc<dyn SpeechOutput>)
}

#[cfg(not(feature = "speech"))]
fn engine_voice(_paths: &ModelPaths, _language: Language, _voice: VoiceChoice) -> Option<Arc<dyn SpeechOutput>> {
    None
}

/// The engine's own recogniser on a microphone: when asked for, or by default when installed.
#[cfg(feature = "speech")]
fn engine_microphone(
    paths: &ModelPaths,
    language: Language,
    mic: MicChoice,
    mic_device: Option<&str>,
) -> Option<Arc<dyn SpeechInput>> {
    let models = paths.speech();
    if matches!(mic, MicChoice::Auto) && !models.recognizer_installed(language) {
        return None;
    }
    let device = match mic_device {
        Some(name) => un_speech::audio::MicDevice::Named(name.to_string()),
        None => un_speech::audio::MicDevice::Default,
    };
    Some(Arc::new(un_speech::MicrophoneInput::new(models, device)))
}

#[cfg(not(feature = "speech"))]
fn engine_microphone(
    _paths: &ModelPaths,
    _language: Language,
    _mic: MicChoice,
    _mic_device: Option<&str>,
) -> Option<Arc<dyn SpeechInput>> {
    None
}

/// The language model in `models/llm`, loaded once per process.
#[cfg(feature = "llm")]
pub fn chat_model(paths: &ModelPaths, log: Option<&un_engine::Logger>) -> Option<std::sync::Arc<dyn un_engine::ChatModel>> {
    let path = paths.chat_model()?;
    if let Some(log) = log {
        log(&format!("chat model: {}", path.display()));
    }
    Some(load_chat_model(&path))
}

/// The language model at `path`, loading in the background (see `un_llm::Background`).
#[cfg(feature = "llm")]
pub fn load_chat_model(path: &std::path::Path) -> Arc<dyn un_engine::ChatModel> {
    un_llm::Background::load(path)
}

#[cfg(not(feature = "llm"))]
pub fn chat_model(_paths: &ModelPaths, _log: Option<&un_engine::Logger>) -> Option<std::sync::Arc<dyn un_engine::ChatModel>> {
    None
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
    let chat = if options.chat { chat_model(paths, log.as_ref()) } else { None };
    // Scripted answers are consumed one per listen; an always-open microphone would read them all at once.
    let always_listen = options.always_listen && options.answers.is_none();
    let engine = Engine::new(
        strategy,
        options.config.clone(),
        speech_in,
        speech_out,
        EngineOptions { chime: options.chime, log, always_listen, chat, ..Default::default() },
    );
    Ok(Session { engine, options, speech_available })
}

#[cfg(test)]
mod microphone_tests {
    use super::pick_microphone;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_usb_microphone_beats_the_built_in_one_and_webcams() {
        let mics = names(&["Samsung SlimFit Cam", "Yeti X", "MacBook Air Microphone", "Microsoft Teams Audio"]);
        assert_eq!(pick_microphone(&mics).as_deref(), Some("Yeti X"));
        assert_eq!(pick_microphone(&names(&["MacBook Air Microphone"])), None);
        assert_eq!(pick_microphone(&names(&["Samsung SlimFit Cam", "MacBook Pro Microphone"])), None);
        assert_eq!(pick_microphone(&names(&["Logitech Webcam Microphone", "Shure MV7"])).as_deref(), Some("Shure MV7"));
    }
}
