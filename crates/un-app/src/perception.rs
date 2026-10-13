//! Frame in, table state out: detector, hand landmarks, estimator and tracker in one call.

use std::path::Path;

use thiserror::Error;
use un_core::{
    DetectionTracker, EstimatorConfig, HandObservation, TableState, TableStateEstimator, TrackedDetection, TrackerConfig,
};
use un_vision::{Detector, DetectorConfig, HandLandmarker, HandLandmarkerConfig, Provider, RgbFrame, VisionError};

use crate::paths::ModelPaths;

#[derive(Debug, Error)]
pub enum PerceptionError {
    #[error("{0}")]
    Vision(#[from] VisionError),
    #[error("detector not found: {0}. Build one with tools/export_detector.py")]
    NoDetector(String),
}

#[derive(Debug, Clone)]
pub struct PerceptionConfig {
    /// Detector name (`detector`, `coco`, `table`) or a path to an `.onnx`.
    pub detector: String,
    pub confidence: f32,
    pub provider: Provider,
    pub hands: bool,
    /// Count only this label.
    pub count_label: Option<String>,
    pub estimator: EstimatorConfig,
    pub mirrored: bool,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            detector: "detector".into(),
            confidence: 0.25,
            provider: Provider::Cpu,
            hands: true,
            count_label: None,
            estimator: EstimatorConfig::default(),
            mirrored: false,
        }
    }
}

pub struct PerceptionOutput {
    pub state: TableState,
    pub tracked: Vec<TrackedDetection>,
    pub hands: Vec<HandObservation>,
}

pub struct Perception {
    pub detector: Detector,
    pub hands: Option<HandLandmarker>,
    pub estimator: TableStateEstimator,
    pub tracker: DetectionTracker,
    pub count_label: Option<String>,
    pub labels: Vec<String>,
}

impl Perception {
    pub fn load(paths: &ModelPaths, config: &PerceptionConfig) -> Result<Self, PerceptionError> {
        let detector_path = paths.detector(&config.detector);
        if !detector_path.exists() {
            return Err(PerceptionError::NoDetector(detector_path.display().to_string()));
        }
        let detector = Detector::load(
            &detector_path,
            DetectorConfig { confidence: config.confidence, provider: config.provider, ..Default::default() },
        )?;
        let hands = if config.hands && paths.hands_installed() {
            Some(HandLandmarker::load(
                &paths.palm_detector(),
                &paths.hand_landmark(),
                HandLandmarkerConfig { mirrored: config.mirrored, provider: config.provider, ..Default::default() },
            )?)
        } else {
            if config.hands {
                log::warn!("hand models missing in {}; hand tracking off (run tools/fetch_models.py)", paths.root.display());
            }
            None
        };
        let labels = detector.object_labels();
        Ok(Self {
            detector,
            hands,
            estimator: TableStateEstimator::new(config.estimator.clone()),
            tracker: DetectionTracker::new(TrackerConfig::default()),
            count_label: config.count_label.clone(),
            labels,
        })
    }

    pub fn detector_path(&self) -> &Path {
        &self.detector.path
    }

    pub fn process(&mut self, frame: &RgbFrame, timestamp: f64) -> Result<PerceptionOutput, PerceptionError> {
        let mut detections = self.detector.detect(frame)?;
        if let Some(label) = &self.count_label {
            detections.retain(|d| d.label.as_deref() == Some(label));
        }
        let hands = match &mut self.hands {
            Some(tracker) => tracker.track(frame)?,
            None => vec![],
        };
        let state = self.estimator.ingest(&detections, &hands, timestamp);
        let tracked = self.tracker.update(&state.detections);
        Ok(PerceptionOutput { state, tracked, hands })
    }

    /// Perception without a camera: the simulated table's detections through the same state machines.
    pub fn process_detections(
        estimator: &mut TableStateEstimator,
        tracker: &mut DetectionTracker,
        detections: &[un_core::Detection],
        hands: &[HandObservation],
        timestamp: f64,
    ) -> PerceptionOutput {
        let state = estimator.ingest(detections, hands, timestamp);
        let tracked = tracker.update(&state.detections);
        PerceptionOutput { state, tracked, hands: hands.to_vec() }
    }
}
