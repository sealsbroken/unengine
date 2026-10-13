//! Where the speech models live and which files make them up.

use std::path::{Path, PathBuf};

use un_core::Language;
use un_engine::SpeechError;

/// The layout written by `tools/fetch_models.py --speech`:
///
/// ```text
/// <root>/
///   silero_vad.onnx
///   sherpa-onnx-zipformer-small-en-2023-06-26/{encoder…int8.onnx, decoder….onnx, joiner…int8.onnx, tokens.txt}
///   sherpa-onnx-zipformer-ru-int8-2025-04-20/{encoder.int8.onnx, decoder.onnx, joiner.int8.onnx, tokens.txt}
///   vits-piper-en_US-lessac-medium/{en_US-lessac-medium.onnx, tokens.txt, espeak-ng-data/}
///   vits-piper-ru_RU-irina-medium/{ru_RU-irina-medium.onnx, tokens.txt, espeak-ng-data/}
/// ```
#[derive(Debug, Clone)]
pub struct SpeechModels {
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RecognizerFiles {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

#[derive(Debug, Clone)]
pub struct VoiceFiles {
    pub model: PathBuf,
    pub tokens: PathBuf,
    pub data_dir: PathBuf,
}

impl SpeechModels {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn recognizer_dir(language: Language) -> &'static str {
        match language {
            Language::English => "sherpa-onnx-zipformer-small-en-2023-06-26",
            Language::Russian => "sherpa-onnx-zipformer-ru-int8-2025-04-20",
        }
    }

    pub fn voice_name(language: Language) -> &'static str {
        match language {
            Language::English => "en_US-lessac-medium",
            Language::Russian => "ru_RU-irina-medium",
        }
    }

    pub fn vad(&self) -> PathBuf {
        self.root.join("silero_vad.onnx")
    }

    pub fn recognizer(&self, language: Language) -> RecognizerFiles {
        let dir = self.root.join(Self::recognizer_dir(language));
        match language {
            Language::English => RecognizerFiles {
                encoder: dir.join("encoder-epoch-99-avg-1.int8.onnx"),
                decoder: dir.join("decoder-epoch-99-avg-1.onnx"),
                joiner: dir.join("joiner-epoch-99-avg-1.int8.onnx"),
                tokens: dir.join("tokens.txt"),
            },
            Language::Russian => RecognizerFiles {
                encoder: dir.join("encoder.int8.onnx"),
                decoder: dir.join("decoder.onnx"),
                joiner: dir.join("joiner.int8.onnx"),
                tokens: dir.join("tokens.txt"),
            },
        }
    }

    pub fn voice(&self, language: Language) -> VoiceFiles {
        let name = Self::voice_name(language);
        let dir = self.root.join(format!("vits-piper-{name}"));
        VoiceFiles {
            model: dir.join(format!("{name}.onnx")),
            tokens: dir.join("tokens.txt"),
            data_dir: dir.join("espeak-ng-data"),
        }
    }

    /// The first missing file for the language, if any.
    pub fn missing(&self, language: Language) -> Option<PathBuf> {
        let r = self.recognizer(language);
        let v = self.voice(language);
        [self.vad(), r.encoder, r.decoder, r.joiner, r.tokens, v.model, v.tokens, v.data_dir].into_iter().find(|p| !p.exists())
    }

    pub fn recognizer_installed(&self, language: Language) -> bool {
        let r = self.recognizer(language);
        self.vad().exists() && [r.encoder, r.decoder, r.joiner, r.tokens].iter().all(|p| p.exists())
    }

    pub fn voice_installed(&self, language: Language) -> bool {
        let v = self.voice(language);
        v.model.exists() && v.tokens.exists() && v.data_dir.exists()
    }

    pub fn any_voice_installed(&self) -> bool {
        Language::ALL.iter().any(|l| self.voice_installed(*l))
    }
}

pub(crate) fn require(path: &Path) -> Result<String, SpeechError> {
    if path.exists() {
        Ok(path.to_string_lossy().into_owned())
    } else {
        Err(SpeechError::AssetsNotInstalled(format!("missing {}; run: python tools/fetch_models.py --speech", path.display())))
    }
}
