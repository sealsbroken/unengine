//! MediaPipe-style hand landmarks on ONNX Runtime: a region of interest, then 21 landmarks per hand.
//!
//! Uses PINTO0309's ONNX conversion of MediaPipe Hands (`hand_landmark_sparse_Nx3x224x224.onnx`,
//! and optionally `palm_detection_full_inf_post_192x192.onnx`). A new hand's region comes from
//! one of two places:
//!
//! * **Seeds** (preferred): hand and finger boxes from the object detector, which runs on every
//!   frame anyway. A seed is turned so the fingers point up (they point away from the frame edge
//!   the hand comes in from; the other three turns are tried if that one does not fit), then
//!   refined once from its own landmarks. Unlike the palm detector this finds a hand whose palm
//!   is out of the picture, which is how a child's hand reaches over a table filmed from above.
//! * **Palm detection**, when no seeds are given and the palm model is loaded.
//!
//! Once a hand is found, the next frame's region comes from its landmarks, as in MediaPipe.

use std::path::Path;

use ort::session::Session;
use ort::value::Tensor;
use un_core::geometry::{Box2, Point};
use un_core::{Chirality, HandObservation};

use crate::frame::RgbFrame;
use crate::runtime::{load_session, Provider, VisionError};

#[derive(Debug, Clone)]
pub struct HandLandmarkerConfig {
    pub max_hands: usize,
    pub palm_threshold: f32,
    pub landmark_threshold: f32,
    /// True for a selfie-style mirrored image; MediaPipe's handedness assumes one.
    pub mirrored: bool,
    /// Run palm detection at least every N frames to find new hands.
    pub redetect_every: u32,
    /// With no hand in view, look for a new one (palm detector, or the seeds' landmark search,
    /// four turns of each seed) only every N frames: a hand coming in is picked up a few frames
    /// later anyway, and a weak false seed cannot cost a landmark search on every frame.
    pub idle_every: u32,
    /// A seed box is grown to this many times its longer side for the landmark crop.
    pub seed_scale: f32,
    pub provider: Provider,
    pub threads: usize,
}

impl Default for HandLandmarkerConfig {
    fn default() -> Self {
        Self {
            max_hands: 2,
            palm_threshold: 0.6,
            landmark_threshold: 0.5,
            mirrored: false,
            redetect_every: 15,
            idle_every: 3,
            seed_scale: 1.8,
            provider: Provider::Cpu,
            threads: 2,
        }
    }
}

/// A rotated square region in frame pixels: `size` is the side, `rotation` in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roi {
    pub cx: f32,
    pub cy: f32,
    pub size: f32,
    pub rotation: f32,
}

impl Roi {
    /// Crop-normalized (0...1) coordinates -> frame pixels.
    pub fn to_frame(&self, u: f32, v: f32) -> (f32, f32) {
        let (s, c) = self.rotation.sin_cos();
        let (du, dv) = ((u - 0.5) * self.size, (v - 0.5) * self.size);
        (self.cx + du * c - dv * s, self.cy + du * s + dv * c)
    }

    fn overlaps(&self, other: &Roi) -> bool {
        let d = ((self.cx - other.cx).powi(2) + (self.cy - other.cy).powi(2)).sqrt();
        d < 0.5 * self.size.min(other.size)
    }
}

/// The turn that brings a hand reaching in from the nearest frame edge upright: fingers point
/// away from that edge, and a region's rotation turns "up" in the crop into that direction.
pub(crate) fn entry_rotation(seed: &Box2) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let gaps = [(seed.x1, FRAC_PI_2), (1.0 - seed.x2, -FRAC_PI_2), (seed.y1, PI), (1.0 - seed.y2, 0.0)];
    gaps.iter().min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)).map(|g| g.1).unwrap_or(0.0)
}

pub(crate) fn normalize_radians(angle: f32) -> f32 {
    angle - 2.0 * std::f32::consts::PI * ((angle + std::f32::consts::PI) / (2.0 * std::f32::consts::PI)).floor()
}

/// MediaPipe's palm-detection-to-ROI: scale 2.6, shifted half a box towards the fingers.
pub(crate) fn palm_roi(cx: f32, cy: f32, size: f32, kp0: Point, kp2: Point) -> Roi {
    let rotation = normalize_radians(std::f32::consts::FRAC_PI_2 - (-(kp2.1 - kp0.1)).atan2(kp2.0 - kp0.0));
    let (s, c) = rotation.sin_cos();
    Roi { cx: cx + 0.5 * size * s, cy: cy - 0.5 * size * c, size: 2.6 * size, rotation }
}

/// MediaPipe's landmarks-to-ROI: rotation from wrist to middle MCP, 2x the hand's extent.
pub(crate) fn landmarks_roi(landmarks_px: &[Point]) -> Roi {
    let wrist = landmarks_px[0];
    let middle = landmarks_px[9];
    let rotation = normalize_radians(std::f32::consts::FRAC_PI_2 - (-(middle.1 - wrist.1)).atan2(middle.0 - wrist.0));
    let (s, c) = rotation.sin_cos();
    // Extent along the rotated axes around the centroid of the palm joints.
    let palm = [0usize, 1, 2, 5, 9, 13, 17];
    let (cx, cy) = palm.iter().fold((0.0, 0.0), |acc, &i| (acc.0 + landmarks_px[i].0, acc.1 + landmarks_px[i].1));
    let (cx, cy) = (cx / palm.len() as f32, cy / palm.len() as f32);
    let mut max_u: f32 = 0.0;
    let mut max_v: f32 = 0.0;
    for &(x, y) in landmarks_px {
        let (dx, dy) = (x - cx, y - cy);
        max_u = max_u.max((dx * c + dy * s).abs());
        max_v = max_v.max((-dx * s + dy * c).abs());
    }
    let size = 2.0 * max_u.max(max_v).max(1.0) * 1.1;
    // Shift the centre towards the fingers so the whole hand fits, like MediaPipe's -0.1 shift.
    Roi { cx: cx + 0.1 * size * s, cy: cy - 0.1 * size * c, size, rotation }
}

pub struct HandLandmarker {
    palm: Option<Session>,
    palm_input: String,
    palm_size: u32,
    landmark: Session,
    landmark_input: String,
    landmark_size: u32,
    pub config: HandLandmarkerConfig,
    rois: Vec<Roi>,
    frames_since_detection: u32,
    palm_buffer: Vec<f32>,
}

impl HandLandmarker {
    /// `palm_model` is optional: without it new hands come only from the seeds passed to [`track`].
    ///
    /// [`track`]: HandLandmarker::track
    pub fn load(palm_model: Option<&Path>, landmark_model: &Path, config: HandLandmarkerConfig) -> Result<Self, VisionError> {
        let palm = palm_model.map(|p| load_session(p, config.provider, config.threads)).transpose()?;
        let landmark = load_session(landmark_model, config.provider, config.threads)?;
        let input_of = |s: &Session| -> Result<(String, u32), VisionError> {
            let input = s.inputs().first().ok_or_else(|| VisionError::Layout("model has no inputs".into()))?;
            let size = input.dtype().tensor_shape().and_then(|d| d.get(2).copied()).filter(|d| *d > 0).unwrap_or(0) as u32;
            Ok((input.name().to_string(), size))
        };
        let (palm_input, palm_size) = match &palm {
            Some(palm) => input_of(palm)?,
            None => (String::new(), 0),
        };
        let (landmark_input, landmark_size) = input_of(&landmark)?;
        let palm_size = if palm_size == 0 { 192 } else { palm_size };
        let landmark_size = if landmark_size == 0 { 224 } else { landmark_size };
        Ok(Self {
            palm,
            palm_input,
            palm_size,
            landmark,
            landmark_input,
            landmark_size,
            config,
            rois: Vec::new(),
            frames_since_detection: 0,
            palm_buffer: vec![0.0; (palm_size * palm_size * 3) as usize],
        })
    }

    pub fn reset(&mut self) {
        self.rois.clear();
        self.frames_since_detection = 0;
    }

    /// Tracks the hands; `seeds` are normalized hand or finger boxes from the object detector,
    /// best first (empty to rely on the palm detector).
    pub fn track(&mut self, frame: &RgbFrame, seeds: &[Box2]) -> Result<Vec<HandObservation>, VisionError> {
        let seeded = !seeds.is_empty();
        let need_detection = self.rois.len() < self.config.max_hands
            && if self.rois.is_empty() {
                self.frames_since_detection >= self.config.idle_every.max(1) - 1
            } else {
                self.frames_since_detection >= self.config.redetect_every
            };
        let mut rois = self.rois.clone();
        if need_detection {
            self.frames_since_detection = 0;
            let found = if seeded {
                self.seed_rois(frame, seeds, &rois)?
            } else if self.palm.is_some() {
                self.detect_palms(frame)?
            } else {
                vec![]
            };
            for roi in found {
                if !rois.iter().any(|r| r.overlaps(&roi)) && rois.len() < self.config.max_hands {
                    rois.push(roi);
                }
            }
        } else {
            self.frames_since_detection += 1;
        }
        if rois.is_empty() {
            self.rois.clear();
            return Ok(vec![]);
        }
        if self.rois.is_empty() {
            // A hand just came in: track it from the next frame on, re-detect on the usual cadence.
            self.frames_since_detection = 0;
        }
        let results = self.landmarks(frame, &rois)?;
        let mut hands = Vec::new();
        let mut next_rois = Vec::new();
        for (landmarks_px, score, right) in results {
            if score < self.config.landmark_threshold {
                continue;
            }
            let roi = landmarks_roi(&landmarks_px);
            if next_rois.iter().any(|r: &Roi| r.overlaps(&roi)) {
                continue; // the same hand found twice
            }
            next_rois.push(roi);
            let normalized: Vec<Point> =
                landmarks_px.iter().map(|(x, y)| (x / frame.width as f32, y / frame.height as f32)).collect();
            // The model reports handedness for a mirrored (selfie) image.
            let mut chirality = if right { Chirality::Right } else { Chirality::Left };
            if !self.config.mirrored {
                chirality = match chirality {
                    Chirality::Left => Chirality::Right,
                    Chirality::Right => Chirality::Left,
                    Chirality::Unknown => Chirality::Unknown,
                };
            }
            hands.push(HandObservation::from_landmarks(chirality, &normalized, score));
        }
        self.rois = next_rois;
        Ok(hands)
    }

    /// Regions for new hands from the detector's boxes: each seed not already tracked is turned so
    /// the fingers point up, the best-fitting turn is kept and refined once from its own landmarks.
    fn seed_rois(&mut self, frame: &RgbFrame, seeds: &[Box2], tracked: &[Roi]) -> Result<Vec<Roi>, VisionError> {
        let (w, h) = (frame.width as f32, frame.height as f32);
        let mut found: Vec<Roi> = Vec::new();
        for seed in seeds {
            if tracked.len() + found.len() >= self.config.max_hands {
                break;
            }
            let (cx, cy) = (seed.cx() * w, seed.cy() * h);
            let size = self.config.seed_scale * (seed.width() * w).max(seed.height() * h);
            let candidate = Roi { cx, cy, size, rotation: 0.0 };
            if tracked.iter().chain(&found).any(|r| r.overlaps(&candidate)) {
                continue;
            }
            // The fingers point away from the edge the hand comes in from: try that turn first,
            // then the other three, in one batch.
            let first = entry_rotation(seed);
            let turns: Vec<Roi> = (0..4)
                .map(|k| Roi { rotation: normalize_radians(first + k as f32 * std::f32::consts::FRAC_PI_2), ..candidate })
                .collect();
            let results = self.landmarks(frame, &turns)?;
            let Some((landmarks, _, _)) =
                results.into_iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            else {
                continue;
            };
            // Refined from its own landmarks; the tracking pass that follows checks the score.
            found.push(landmarks_roi(&landmarks));
        }
        Ok(found)
    }

    fn detect_palms(&mut self, frame: &RgbFrame) -> Result<Vec<Roi>, VisionError> {
        let Some(palm) = self.palm.as_mut() else { return Ok(vec![]) };
        let size = self.palm_size;
        let scale = size as f32 / frame.width.max(frame.height) as f32;
        let pad_x = (size as f32 - frame.width as f32 * scale) / 2.0;
        let pad_y = (size as f32 - frame.height as f32 * scale) / 2.0;
        frame.write_planar(&mut self.palm_buffer, size, size, scale, pad_x, pad_y, [0, 0, 0]);
        let tensor = Tensor::from_array((vec![1i64, 3, size as i64, size as i64], self.palm_buffer.clone()))?;
        let outputs = palm.run(ort::inputs![self.palm_input.as_str() => tensor])?;
        let (shape, data) = outputs[0].try_extract_tensor::<f32>()?;
        let cols = *shape.last().unwrap_or(&8) as usize;
        if cols < 8 {
            return Err(VisionError::Layout(format!("palm detector output has {cols} columns, expected 8")));
        }
        let to_px = |x: f32, y: f32| ((x * size as f32 - pad_x) / scale, (y * size as f32 - pad_y) / scale);
        let mut rois = Vec::new();
        for row in data.chunks_exact(cols) {
            let score = row[0];
            if score <= self.config.palm_threshold || row[3] <= 0.0 {
                continue;
            }
            let (cx, cy) = to_px(row[1], row[2]);
            let box_size = row[3] * size as f32 / scale;
            let kp0 = to_px(row[4], row[5]);
            let kp2 = to_px(row[6], row[7]);
            rois.push((score, palm_roi(cx, cy, box_size, kp0, kp2)));
        }
        rois.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut kept: Vec<Roi> = Vec::new();
        for (_, roi) in rois {
            if !kept.iter().any(|k| k.overlaps(&roi)) {
                kept.push(roi);
            }
        }
        Ok(kept)
    }

    /// Runs the landmark model on every ROI; returns landmarks in frame pixels, score and "is right hand".
    fn landmarks(&mut self, frame: &RgbFrame, rois: &[Roi]) -> Result<Vec<(Vec<Point>, f32, bool)>, VisionError> {
        let size = self.landmark_size;
        let plane = (size * size) as usize;
        let mut batch = vec![0.0f32; rois.len() * plane * 3];
        for (n, roi) in rois.iter().enumerate() {
            let out = &mut batch[n * plane * 3..(n + 1) * plane * 3];
            for oy in 0..size {
                for ox in 0..size {
                    let (sx, sy) = roi.to_frame((ox as f32 + 0.5) / size as f32, (oy as f32 + 0.5) / size as f32);
                    let px = frame.sample(sx - 0.5, sy - 0.5, [0, 0, 0]);
                    let i = (oy * size + ox) as usize;
                    out[i] = px[0] as f32 / 255.0;
                    out[plane + i] = px[1] as f32 / 255.0;
                    out[2 * plane + i] = px[2] as f32 / 255.0;
                }
            }
        }
        let tensor = Tensor::from_array((vec![rois.len() as i64, 3, size as i64, size as i64], batch))?;
        let outputs = self.landmark.run(ort::inputs![self.landmark_input.as_str() => tensor])?;
        let (xyz_shape, xyz) = outputs[0].try_extract_tensor::<f32>()?;
        let (_, scores) = outputs[1].try_extract_tensor::<f32>()?;
        let (_, handedness) = outputs[2].try_extract_tensor::<f32>()?;
        let stride = *xyz_shape.last().unwrap_or(&63) as usize;
        if stride < 63 {
            return Err(VisionError::Layout(format!("landmark output has {stride} values per hand, expected 63")));
        }
        let mut results = Vec::new();
        for (n, roi) in rois.iter().enumerate() {
            let values = &xyz[n * stride..(n + 1) * stride];
            let landmarks: Vec<Point> =
                (0..21).map(|i| roi.to_frame(values[i * 3] / size as f32, values[i * 3 + 1] / size as f32)).collect();
            let score = scores.get(n).copied().unwrap_or(0.0);
            let right = handedness.get(n).copied().unwrap_or(0.0) > 0.5;
            results.push((landmarks, score, right));
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roi_mapping_round_trips_without_rotation() {
        let roi = Roi { cx: 100.0, cy: 50.0, size: 20.0, rotation: 0.0 };
        assert_eq!(roi.to_frame(0.5, 0.5), (100.0, 50.0));
        assert_eq!(roi.to_frame(0.0, 0.0), (90.0, 40.0));
        let turned = Roi { rotation: std::f32::consts::FRAC_PI_2, ..roi };
        let (x, y) = turned.to_frame(1.0, 0.5);
        assert!((x - 100.0).abs() < 1e-4 && (y - 60.0).abs() < 1e-4);
    }

    #[test]
    fn a_hand_from_the_left_edge_is_turned_to_point_right() {
        // Crop "up" (u 0.5, v 0) maps to the right of the centre for a hand coming in from the left.
        let from_left = Box2::new(0.0, 0.3, 0.35, 0.45);
        let roi = Roi { cx: 100.0, cy: 100.0, size: 20.0, rotation: entry_rotation(&from_left) };
        let (x, y) = roi.to_frame(0.5, 0.0);
        assert!((x - 110.0).abs() < 1e-4 && (y - 100.0).abs() < 1e-4, "{x} {y}");
        // From the bottom edge the fingers already point up: no turn.
        assert_eq!(entry_rotation(&Box2::new(0.4, 0.7, 0.6, 1.0)), 0.0);
        // From the top they point down.
        let from_top = Roi { rotation: entry_rotation(&Box2::new(0.4, 0.0, 0.6, 0.2)), ..roi };
        let (_, y) = from_top.to_frame(0.5, 0.0);
        assert!((y - 110.0).abs() < 1e-4, "{y}");
    }

    #[test]
    fn upright_palm_has_zero_rotation() {
        // Wrist below the middle finger base: the hand points up, no rotation needed.
        let roi = palm_roi(100.0, 100.0, 40.0, (100.0, 120.0), (100.0, 80.0));
        assert!(roi.rotation.abs() < 1e-5, "{}", roi.rotation);
        assert!((roi.size - 104.0).abs() < 1e-4);
        assert!((roi.cy - 80.0).abs() < 1e-4); // shifted half a box towards the fingers
        assert!((normalize_radians(4.0) - (4.0 - 2.0 * std::f32::consts::PI)).abs() < 1e-5);
    }

    #[test]
    fn landmark_roi_covers_the_hand() {
        let mut pts: Vec<Point> = (0..21).map(|i| (100.0 + (i % 5) as f32 * 10.0, 200.0 - (i / 5) as f32 * 20.0)).collect();
        pts[0] = (120.0, 220.0);
        pts[9] = (120.0, 140.0);
        let roi = landmarks_roi(&pts);
        assert!(roi.rotation.abs() < 1e-5);
        for (x, y) in &pts {
            let (s, c) = roi.rotation.sin_cos();
            let (dx, dy) = (x - roi.cx, y - roi.cy);
            let (u, v) = ((dx * c + dy * s) / roi.size + 0.5, (-dx * s + dy * c) / roi.size + 0.5);
            assert!((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v), "{u} {v}");
        }
    }
}
