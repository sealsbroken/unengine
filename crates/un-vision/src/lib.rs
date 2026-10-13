//! Un's vision engine: ONNX Runtime sessions behind two small APIs.
//!
//! * [`Detector`]: an exported object detector (Ultralytics raw heads, end-to-end
//!   YOLO26/YOLOv10 rows, or DETR-style boxes) plus its `.json` sidecar with labels.
//! * [`HandLandmarker`]: MediaPipe's palm detection + hand landmark models converted to
//!   ONNX (see `tools/fetch_models.py`), giving the same 21 joints as MediaPipe and
//!   Apple Vision.
//!
//! Frames are plain RGB8 buffers ([`RgbFrame`]); every platform can produce one.

pub mod detector;
pub mod frame;
pub mod hands;
pub mod runtime;

pub use detector::{Detector, DetectorConfig, Letterbox, RawDecoder};
pub use frame::RgbFrame;
pub use hands::{HandLandmarker, HandLandmarkerConfig};
pub use runtime::{Provider, VisionError};
