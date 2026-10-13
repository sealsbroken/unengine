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

/// Loads a session with graph optimisation and the requested provider.
pub fn load_session(path: &Path, provider: Provider, threads: usize) -> Result<Session, VisionError> {
    if !path.exists() {
        return Err(VisionError::ModelNotFound(path.display().to_string()));
    }
    let env = environment()?;
    let mut builder = Session::builder(env)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(runtime_error)?
        .with_intra_threads(threads.max(1))
        .map_err(runtime_error)?;
    builder = register_provider(builder, provider)?;
    Ok(builder.commit_from_file(path)?)
}

fn runtime_error<T>(error: ort::Error<T>) -> VisionError {
    VisionError::Runtime(error.to_string())
}

#[allow(unused_mut, unused_variables, unused_imports)]
fn register_provider(
    mut builder: ort::session::builder::SessionBuilder,
    provider: Provider,
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
                builder = builder.with_execution_providers([ep::CoreML::default().build()]).map_err(runtime_error)?
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
