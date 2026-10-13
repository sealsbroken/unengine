//! Where the models live: `UN_MODELS`, `models/` next to the executable, or `models/` in the working directory.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub root: PathBuf,
}

impl ModelPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The first existing candidate, else `./models`.
    pub fn discover() -> Self {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(env) = std::env::var("UN_MODELS") {
            candidates.push(PathBuf::from(env));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("models"));
                candidates.push(dir.join("../Resources/models"));
                candidates.push(dir.join("../../models"));
                candidates.push(dir.join("../../../models"));
            }
        }
        candidates.push(PathBuf::from("models"));
        for candidate in &candidates {
            if candidate.is_dir() {
                return Self::new(candidate.clone());
            }
        }
        Self::new("models")
    }

    pub fn detector(&self, name: &str) -> PathBuf {
        let path = Path::new(name);
        if path.extension().is_some() || path.components().count() > 1 {
            path.to_path_buf()
        } else {
            self.root.join(format!("{name}.onnx"))
        }
    }

    pub fn palm_detector(&self) -> PathBuf {
        self.root.join("palm_detection.onnx")
    }

    pub fn hand_landmark(&self) -> PathBuf {
        self.root.join("hand_landmark.onnx")
    }

    pub fn hands_installed(&self) -> bool {
        self.palm_detector().exists() && self.hand_landmark().exists()
    }

    pub fn speech(&self) -> un_speech::SpeechModels {
        un_speech::SpeechModels::new(self.root.join("speech"))
    }
}
