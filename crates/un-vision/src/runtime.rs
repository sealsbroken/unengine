//! ONNX Runtime environment and execution providers.

use std::path::Path;
use std::sync::OnceLock;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VisionError {
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("onnx runtime: {0}")]
    Runtime(String),
    #[error("unexpected model layout: {0}")]
    Layout(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl From<ort::Error> for VisionError {
    fn from(error: ort::Error) -> Self {
        VisionError::Runtime(error.to_string())
    }
}

/// Which hardware runs the models. Providers that were not compiled in fall back to the CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Cpu,
    /// Apple Neural Engine / GPU (feature `coreml`).
    CoreMl,
    /// NVIDIA (feature `cuda`).
    Cuda,
    /// Any DirectX 12 GPU on Windows (feature `directml`).
    DirectMl,
    /// XNNPACK CPU kernels, useful on Android (feature `xnnpack`).
    Xnnpack,
    /// Android NNAPI (feature `nnapi`).
    Nnapi,
    /// The best compiled-in accelerator, else CPU.
    Auto,
}

impl Provider {
    pub fn parse(text: &str) -> Option<Provider> {
        Some(match text.to_lowercase().as_str() {
            "cpu" => Provider::Cpu,
            "coreml" => Provider::CoreMl,
            "cuda" => Provider::Cuda,
            "directml" | "dml" => Provider::DirectMl,
            "xnnpack" => Provider::Xnnpack,
            "nnapi" => Provider::Nnapi,
            "auto" => Provider::Auto,
            _ => return None,
        })
    }

    /// Providers compiled into this build.
    pub fn available() -> Vec<Provider> {
        let mut list = vec![Provider::Cpu];
        if cfg!(feature = "coreml") {
            list.push(Provider::CoreMl);
        }
        if cfg!(feature = "cuda") {
            list.push(Provider::Cuda);
        }
        if cfg!(feature = "directml") {
            list.push(Provider::DirectMl);
        }
        if cfg!(feature = "xnnpack") {
            list.push(Provider::Xnnpack);
        }
        if cfg!(feature = "nnapi") {
            list.push(Provider::Nnapi);
        }
        list
    }
}

fn environment() -> Result<&'static ort::environment::Environment, VisionError> {
    static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();
    match ENV.get_or_init(|| ort::init().with_name("un").build().map_err(|e| e.to_string())) {
        Ok(env) => Ok(env),
        Err(error) => Err(VisionError::Runtime(error.clone())),
    }
}

/// Which CoreML model format ONNX Runtime builds for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoreMlFormat {
    /// The older NeuralNetwork format: compiles quickly; enough for convolutional detectors.
    #[default]
    NeuralNetwork,
    /// MLProgram: more operators (GridSample, the attention of DETR-style detectors), so the whole
    /// model runs on the GPU / Neural Engine; compiled once and cached (see [`coreml_cache_dir`]).
    MlProgram,
}

/// Where compiled CoreML models are kept between launches: `UN_CACHE_DIR`, else the system's
/// temporary directory (an app's own on iOS). A first compile of a DETR-style model takes about a
/// minute; a cached one loads in seconds.
pub fn coreml_cache_dir(model: &Path) -> std::path::PathBuf {
    let base = std::env::var_os("UN_CACHE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let root = base.join("un-coreml");
    let dir = root.join(format!("{stem}-{}", model_stamp(model)));
    // A compiled model is as large as the model: drop the ones of earlier exports.
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            // `<stem>-<size>-<stamp>`; another model's name (`toys-yoloe-...`) has more parts.
            let earlier = name.strip_prefix(&format!("{stem}-")).is_some_and(|rest| {
                rest.split_once('-').is_some_and(|(size, stamp)| size.bytes().all(|b| b.is_ascii_digit()) && !stamp.contains('-'))
            });
            if path != dir && earlier {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }
    dir
}

/// The model's size and a hash of its first and last megabyte: the same model keeps its
/// compiled copy across reinstalls and app updates (which give the file a new date; keyed on
/// the date, every install compiled again, 45 s on an iPad mini), a new export gets a new one.
fn model_stamp(model: &Path) -> String {
    use std::hash::Hasher;
    use std::io::{Read, Seek, SeekFrom};
    const PART: u64 = 1 << 20;
    let Ok(mut file) = std::fs::File::open(model) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = Vec::new();
    let _ = file.by_ref().take(PART).read_to_end(&mut buffer);
    if len > 2 * PART && file.seek(SeekFrom::End(-(PART as i64))).is_ok() {
        let _ = file.read_to_end(&mut buffer);
    }
    hasher.write(&buffer);
    format!("{len}-{:016x}", hasher.finish())
}

/// Loads a session with graph optimisation and the requested provider.
pub fn load_session(path: &Path, provider: Provider, threads: usize) -> Result<Session, VisionError> {
    load_session_with(path, provider, threads, CoreMlFormat::default())
}

/// [`load_session`] with the CoreML format the model needs.
pub fn load_session_with(path: &Path, provider: Provider, threads: usize, coreml: CoreMlFormat) -> Result<Session, VisionError> {
    if !path.exists() {
        return Err(VisionError::ModelNotFound(path.display().to_string()));
    }
    let env = environment()?;
    let mut builder = Session::builder(env)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(runtime_error)?
        .with_intra_threads(threads.max(1))
        .map_err(runtime_error)?;
    builder = register_provider(builder, provider, path, coreml)?;
    Ok(builder.commit_from_file(path)?)
}

fn runtime_error<T>(error: ort::Error<T>) -> VisionError {
    VisionError::Runtime(error.to_string())
}

#[allow(unused_mut, unused_variables, unused_imports)]
fn register_provider(
    mut builder: ort::session::builder::SessionBuilder,
    provider: Provider,
    model: &Path,
    format: CoreMlFormat,
) -> Result<ort::session::builder::SessionBuilder, VisionError> {
    use ort::ep;
    let wanted: Vec<Provider> = match provider {
        Provider::Auto => Provider::available().into_iter().filter(|p| *p != Provider::Cpu).collect(),
        Provider::Cpu => vec![],
        other => vec![other],
    };
    for p in wanted {
        match p {
            #[cfg(feature = "coreml")]
            Provider::CoreMl => {
                let coreml = match format {
                    CoreMlFormat::NeuralNetwork => ep::CoreML::default(),
                    CoreMlFormat::MlProgram => {
                        let cache = coreml_cache_dir(model);
                        let _ = std::fs::create_dir_all(&cache);
                        ep::CoreML::default()
                            .with_model_format(ep::coreml::ModelFormat::MLProgram)
                            .with_model_cache_dir(cache.display())
                    }
                };
                builder = builder.with_execution_providers([coreml.build()]).map_err(runtime_error)?
            }
            #[cfg(feature = "cuda")]
            Provider::Cuda => builder = builder.with_execution_providers([ep::CUDA::default().build()]).map_err(runtime_error)?,
            #[cfg(feature = "directml")]
            Provider::DirectMl => {
                builder = builder.with_execution_providers([ep::DirectML::default().build()]).map_err(runtime_error)?
            }
            #[cfg(feature = "xnnpack")]
            Provider::Xnnpack => {
                builder = builder.with_execution_providers([ep::XNNPACK::default().build()]).map_err(runtime_error)?
            }
            #[cfg(feature = "nnapi")]
            Provider::Nnapi => {
                builder = builder.with_execution_providers([ep::NNAPI::default().build()]).map_err(runtime_error)?
            }
            other => log::warn!("execution provider {other:?} is not compiled in; using the CPU"),
        }
    }
    Ok(builder)
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn the_compiled_model_follows_the_content_not_the_file_date() {
        let dir = std::env::temp_dir().join(format!("un-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("toys.onnx");
        std::fs::write(&model, vec![7u8; 3 << 20]).unwrap();
        let first = model_stamp(&model);
        // A reinstall: same bytes, new date.
        std::fs::write(&model, vec![7u8; 3 << 20]).unwrap();
        assert_eq!(model_stamp(&model), first);
        let mut changed = vec![7u8; 3 << 20];
        changed[10] = 8;
        std::fs::write(&model, changed).unwrap();
        assert_ne!(model_stamp(&model), first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn earlier_compiles_of_the_same_model_are_dropped_and_others_kept() {
        let base = std::env::temp_dir().join(format!("un-cache-prune-{}", std::process::id()));
        let root = base.join("un-coreml");
        for name in ["toys-100-1700000000", "toys-yoloe-40-1700000000", "caps-5-abc"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let model = base.join("toys.onnx");
        std::fs::write(&model, b"model").unwrap();
        std::env::set_var("UN_CACHE_DIR", &base);
        let dir = coreml_cache_dir(&model);
        std::env::remove_var("UN_CACHE_DIR");
        std::fs::create_dir_all(&dir).unwrap();
        let mut left: Vec<String> =
            std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left.len(), 3);
        assert!(!left.contains(&"toys-100-1700000000".to_string()));
        assert!(left.contains(&"toys-yoloe-40-1700000000".to_string()) && left.contains(&"caps-5-abc".to_string()));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
