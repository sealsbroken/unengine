//! Frame in, table state out: detector, estimator and tracker in one call.

use std::path::Path;

use thiserror::Error;
use un_core::{Detection, DetectionTracker, EstimatorConfig, TableState, TableStateEstimator, TrackedDetection, TrackerConfig};
use un_vision::{Detector, DetectorConfig, Provider, RgbFrame, VisionError};

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
    /// Detector name (`toys`, or one taught with tools/teach.py) or a path to an `.onnx`.
    pub detector: String,
    pub confidence: f32,
    pub provider: Provider,
    /// Count only this label.
    pub count_label: Option<String>,
    pub estimator: EstimatorConfig,
}

impl Default for PerceptionConfig {
    fn default() -> Self {
        Self {
            detector: "toys".into(),
            confidence: 0.25,
            provider: Provider::Cpu,
            count_label: None,
            estimator: EstimatorConfig::default(),
        }
    }
}

pub struct PerceptionOutput {
    pub state: TableState,
    pub tracked: Vec<TrackedDetection>,
}

pub struct Perception {
    pub detector: Detector,
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
        let labels = detector.object_labels();
        Ok(Self {
            detector,
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
        Ok(Self::process_detections(&mut self.estimator, &mut self.tracker, &detections, timestamp))
    }

    /// Perception without a camera: the simulated table's detections through the same state machines.
    pub fn process_detections(
        estimator: &mut TableStateEstimator,
        tracker: &mut DetectionTracker,
        detections: &[Detection],
        timestamp: f64,
    ) -> PerceptionOutput {
        let state = estimator.ingest(detections, timestamp);
        let tracked = tracker.update(&state.detections);
        PerceptionOutput { state, tracked }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use un_core::Box2;

    #[test]
    fn hand_and_finger_decoys_are_never_counted() {
        let mut estimator = TableStateEstimator::default();
        let mut tracker = DetectionTracker::new(TrackerConfig::default());
        let detections = vec![
            Detection::labeled(Box2::new(0.1, 0.1, 0.2, 0.2), 0.9, "car"),
            Detection::labeled(Box2::new(0.5, 0.5, 0.6, 0.6), 0.9, "car"),
            Detection::labeled(Box2::new(0.55, 0.3, 0.6, 0.55), 0.8, "_finger"),
            Detection::labeled(Box2::new(0.4, 0.2, 0.8, 0.6), 0.8, "_hand"),
        ];
        let out = Perception::process_detections(&mut estimator, &mut tracker, &detections, 0.0);
        assert_eq!(out.state.raw_count, 2);
        assert_eq!(out.state.detections.len(), 2);
    }
}
