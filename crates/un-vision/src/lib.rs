//! Un's vision engine: an ONNX Runtime object detector.
//!
//! * [`Detector`]: an exported object detector (Ultralytics raw heads, end-to-end
//!   YOLO26/YOLOv10 rows, or DETR-style boxes) plus its `.json` sidecar with labels.
//!
//! Frames are plain RGB8 buffers ([`RgbFrame`]); every platform can produce one.

pub mod detector;
pub mod frame;
pub mod runtime;

pub use detector::{Detector, DetectorConfig, Letterbox, RawDecoder};
pub use frame::RgbFrame;
pub use runtime::{CoreMlFormat, Provider, VisionError};
