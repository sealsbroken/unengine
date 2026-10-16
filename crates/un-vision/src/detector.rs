//! Object detector: an exported ONNX model, letterboxed input, decoded raw head, class-agnostic NMS.
//!
//! The `.json` sidecar next to the model (written by `tools/export_detector.py`) holds
//! `labels` (one per class), `imgsz` (`[height, width]`) and optionally `head`.

use std::path::{Path, PathBuf};

use ort::session::Session;
use ort::value::Tensor;
use serde::{Deserialize, Serialize};
use un_core::{Box2, Detection};

use crate::frame::RgbFrame;
use crate::runtime::{load_session, Provider, VisionError};

pub const PAD_VALUE: u8 = 114;

/// How a frame was fitted into the model input: `model_px = frame_px * scale + pad`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Letterbox {
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub frame_w: u32,
    pub frame_h: u32,
}

impl Letterbox {
    pub fn fit(frame_w: u32, frame_h: u32, input_w: u32, input_h: u32) -> Self {
        let scale = (input_w as f32 / frame_w as f32).min(input_h as f32 / frame_h as f32);
        let (nw, nh) = ((frame_w as f32 * scale).round(), (frame_h as f32 * scale).round());
        let pad_x = ((input_w as f32 - nw) / 2.0 - 0.1).round().max(0.0);
        let pad_y = ((input_h as f32 - nh) / 2.0 - 0.1).round().max(0.0);
        Self { scale, pad_x, pad_y, frame_w, frame_h }
    }

    /// Model-input pixels -> normalized frame coordinates.
    pub fn to_frame(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.pad_x) / self.scale / self.frame_w as f32, (y - self.pad_y) / self.scale / self.frame_h as f32)
    }
}

/// Which raw head the model has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Head {
    /// Guess from the output shape.
    #[default]
    Auto,
    /// Ultralytics one-to-many: `[1, 4 + classes (+ 32 mask coeffs), anchors]`, `cx cy w h` in input pixels.
    Yolo,
    /// End-to-end (YOLO26 / YOLOv10): `[1, K, 6]` rows of `x1 y1 x2 y2 score class` in input pixels.
    E2e,
    /// RT-DETR style: `[1, K, 4 + classes]` rows of normalized `cx cy w h` then class scores.
    Rtdetr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub imgsz: Vec<u32>,
    #[serde(default)]
    pub head: Head,
    #[serde(default)]
    pub prompts: Vec<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct DetectorConfig {
    pub confidence: f32,
    pub iou: f32,
    pub max_detections: usize,
    pub provider: Provider,
    pub threads: usize,
    /// Overrides the sidecar's labels.
    pub labels: Option<Vec<String>>,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self { confidence: 0.25, iou: 0.5, max_detections: 50, provider: Provider::Cpu, threads: 2, labels: None }
    }
}

/// Greedy class-agnostic NMS over x1 y1 x2 y2 boxes; returns kept indices, best first.
pub fn non_max_suppression(boxes: &[[f32; 4]], scores: &[f32], iou: f32, limit: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal));
    let area = |b: &[f32; 4]| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let mut kept = Vec::new();
    let mut suppressed = vec![false; scores.len()];
    for &i in &order {
        if suppressed[i] {
            continue;
        }
        kept.push(i);
        if kept.len() >= limit {
            break;
        }
        for &j in &order {
            if suppressed[j] || j == i {
                continue;
            }
            let (a, b) = (&boxes[i], &boxes[j]);
            let inter = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
            let union = area(a) + area(b) - inter;
            if union > 0.0 && inter / union > iou {
                suppressed[j] = true;
            }
        }
    }
    kept
}

/// Decodes raw detector heads into normalized, NMS-filtered detections.
#[derive(Debug, Clone)]
pub struct RawDecoder {
    pub confidence: f32,
    pub iou: f32,
    pub class_count: Option<usize>,
    pub max_detections: usize,
    pub labels: Vec<String>,
    pub head: Head,
}

impl RawDecoder {
    pub fn decode(&self, output: &[f32], shape: &[i64], fit: &Letterbox, input_w: u32, input_h: u32) -> Vec<Detection> {
        if shape.len() != 3 || shape[0] != 1 {
            return vec![];
        }
        let (d1, d2) = (shape[1] as usize, shape[2] as usize);
        let head = match self.head {
            Head::Auto => {
                if d2 == 6 {
                    Head::E2e
                } else if d1 < d2 {
                    Head::Yolo
                } else {
                    Head::Rtdetr
                }
            }
            other => other,
        };
        let mut boxes: Vec<[f32; 4]> = Vec::new();
        let mut scores: Vec<f32> = Vec::new();
        let mut classes: Vec<usize> = Vec::new();
        match head {
            Head::E2e => {
                for row in output.as_chunks::<6>().0.iter().take(d1) {
                    if row[4] >= self.confidence {
                        boxes.push([row[0], row[1], row[2], row[3]]);
                        scores.push(row[4]);
                        classes.push(row[5] as usize);
                    }
                }
            }
            Head::Yolo | Head::Auto => {
                let (channels, anchors) = (d1, d2);
                let nc = self.class_count.unwrap_or(if channels - 4 <= 8 { channels - 4 } else { (channels - 4 - 32).max(1) });
                if channels < 4 + nc {
                    return vec![];
                }
                for a in 0..anchors {
                    let (mut best, mut best_score) = (0usize, f32::MIN);
                    for c in 0..nc {
                        let s = output[(4 + c) * anchors + a];
                        if s > best_score {
                            best_score = s;
                            best = c;
                        }
                    }
                    if best_score >= self.confidence {
                        let (cx, cy, w, h) = (output[a], output[anchors + a], output[2 * anchors + a], output[3 * anchors + a]);
                        boxes.push([cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]);
                        scores.push(best_score);
                        classes.push(best);
                    }
                }
            }
            Head::Rtdetr => {
                let nc = d2 - 4;
                for row in output.chunks_exact(d2).take(d1) {
                    let (mut best, mut best_score) = (0usize, f32::MIN);
                    for (c, s) in row[4..].iter().enumerate() {
                        if *s > best_score {
                            best_score = *s;
                            best = c;
                        }
                    }
                    if nc > 0 && best_score >= self.confidence {
                        let (cx, cy, w, h) =
                            (row[0] * input_w as f32, row[1] * input_h as f32, row[2] * input_w as f32, row[3] * input_h as f32);
                        boxes.push([cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0]);
                        scores.push(best_score);
                        classes.push(best);
                    }
                }
            }
        }
        if scores.is_empty() {
            return vec![];
        }
        let normalized: Vec<[f32; 4]> = boxes
            .iter()
            .map(|b| {
                let (x1, y1) = fit.to_frame(b[0], b[1]);
                let (x2, y2) = fit.to_frame(b[2], b[3]);
                [x1.clamp(0.0, 1.0), y1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0), y2.clamp(0.0, 1.0)]
            })
            .collect();
        non_max_suppression(&normalized, &scores, self.iou, self.max_detections)
            .into_iter()
            .map(|i| {
                let b = normalized[i];
                Detection {
                    bbox: Box2::new(b[0], b[1], b[2], b[3]),
                    confidence: scores[i],
                    label: self.labels.get(classes[i]).cloned(),
                }
            })
            .collect()
    }
}

/// ONNX Runtime wrapper around an exported detector plus its `.json` sidecar.
pub struct Detector {
    session: Session,
    input_name: String,
    pub input_w: u32,
    pub input_h: u32,
    pub labels: Vec<String>,
    pub decoder: RawDecoder,
    pub path: PathBuf,
    buffer: Vec<f32>,
}

impl Detector {
    pub fn load(path: &Path, config: DetectorConfig) -> Result<Self, VisionError> {
        let sidecar_path = path.with_extension("json");
        let sidecar: Sidecar = match std::fs::read_to_string(&sidecar_path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| VisionError::Layout(format!("{}: {e}", sidecar_path.display())))?
            }
            Err(_) => Sidecar { labels: vec![], imgsz: vec![], head: Head::Auto, prompts: vec![], source: String::new() },
        };
        let session = load_session(path, config.provider, config.threads)?;
        let input = session.inputs().first().ok_or_else(|| VisionError::Layout("model has no inputs".into()))?;
        let input_name = input.name().to_string();
        let dims: Vec<i64> = match input.dtype().tensor_shape() {
            Some(shape) => shape.iter().copied().collect(),
            None => vec![],
        };
        let (input_h, input_w) = if sidecar.imgsz.len() == 2 {
            (sidecar.imgsz[0], sidecar.imgsz[1])
        } else {
            let dim = |i: usize| dims.get(i).copied().filter(|d| *d > 0).unwrap_or(640) as u32;
            (dim(2), dim(3))
        };
        let labels = config.labels.clone().unwrap_or(sidecar.labels);
        let decoder = RawDecoder {
            confidence: config.confidence,
            iou: config.iou,
            class_count: (!labels.is_empty()).then_some(labels.len()),
            max_detections: config.max_detections,
            labels: labels.clone(),
            head: sidecar.head,
        };
        Ok(Self {
            session,
            input_name,
            input_w,
            input_h,
            labels,
            decoder,
            path: path.to_path_buf(),
            buffer: vec![0.0; (input_w * input_h * 3) as usize],
        })
    }

    /// Distinct non-decoy labels, in model order.
    pub fn object_labels(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for label in &self.labels {
            if !label.starts_with('_') && !seen.contains(label) {
                seen.push(label.clone());
            }
        }
        seen
    }

    pub fn detect(&mut self, frame: &RgbFrame) -> Result<Vec<Detection>, VisionError> {
        let fit = Letterbox::fit(frame.width, frame.height, self.input_w, self.input_h);
        frame.write_planar(&mut self.buffer, self.input_w, self.input_h, fit.scale, fit.pad_x, fit.pad_y, [PAD_VALUE; 3]);
        let tensor = Tensor::from_array((vec![1i64, 3, self.input_h as i64, self.input_w as i64], self.buffer.clone()))?;
        let outputs = self.session.run(ort::inputs![self.input_name.as_str() => tensor])?;
        let mut best: Option<(Vec<i64>, Vec<f32>)> = None;
        for (_, value) in outputs.iter() {
            if let Ok((shape, data)) = value.try_extract_tensor::<f32>() {
                if shape.len() == 3 {
                    best = Some((shape.iter().copied().collect(), data.to_vec()));
                    break;
                }
            }
        }
        let (shape, data) = best.ok_or_else(|| VisionError::Layout("no 3-D float output".into()))?;
        Ok(self.decoder.decode(&data, &shape, &fit, self.input_w, self.input_h).into_iter().filter(|d| !d.is_decoy()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_maps_back() {
        let fit = Letterbox::fit(1280, 720, 640, 384);
        assert!((fit.scale - 0.5).abs() < 1e-6);
        assert_eq!((fit.pad_x, fit.pad_y), (0.0, 12.0));
        let (x, y) = fit.to_frame(320.0, 12.0 + 180.0);
        assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6);
    }

    #[test]
    fn nms_keeps_best_of_overlapping() {
        let boxes = [[0.0, 0.0, 1.0, 1.0], [0.05, 0.05, 1.0, 1.0], [2.0, 2.0, 3.0, 3.0]];
        let scores = [0.5, 0.9, 0.7];
        assert_eq!(non_max_suppression(&boxes, &scores, 0.5, 10), vec![1, 2]);
        assert_eq!(non_max_suppression(&boxes, &scores, 0.5, 1), vec![1]);
    }

    fn decoder(labels: &[&str]) -> RawDecoder {
        RawDecoder {
            confidence: 0.25,
            iou: 0.5,
            class_count: Some(labels.len()),
            max_detections: 10,
            labels: labels.iter().map(|s| s.to_string()).collect(),
            head: Head::Auto,
        }
    }

    #[test]
    fn decodes_end_to_end_rows() {
        let fit = Letterbox::fit(640, 640, 640, 640);
        let rows = [[0.0, 0.0, 64.0, 64.0, 0.9, 1.0], [600.0, 600.0, 640.0, 640.0, 0.1, 0.0]];
        let flat: Vec<f32> = rows.concat();
        let dets = decoder(&["cube", "car"]).decode(&flat, &[1, 2, 6], &fit, 640, 640);
        assert_eq!(dets.len(), 1);
        assert_eq!(dets[0].label.as_deref(), Some("car"));
        assert!((dets[0].bbox.x2 - 0.1).abs() < 1e-6);
    }

    #[test]
    fn decodes_channels_first_head() {
        let fit = Letterbox::fit(640, 640, 640, 640);
        // 2 classes, 8 anchors (always far more anchors than channels): rows cx, cy, w, h, c0, c1.
        let anchors = 8;
        let mut out = vec![0.0f32; 6 * anchors];
        out[0] = 320.0; // cx of anchor 0
        out[anchors] = 320.0; // cy
        out[2 * anchors] = 64.0; // w
        out[3 * anchors] = 64.0; // h
        out[4 * anchors] = 0.8; // class 0 score
        out[5 * anchors + 1] = 0.3; // anchor 1 class 1, but w/h zero
        let dets = decoder(&["cube", "car"]).decode(&out, &[1, 6, anchors as i64], &fit, 640, 640);
        assert_eq!(dets.len(), 2);
        assert_eq!(dets[0].label.as_deref(), Some("cube"));
        assert!((dets[0].bbox.cx() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn decodes_rtdetr_rows() {
        let fit = Letterbox::fit(640, 640, 640, 640);
        let rows = [[0.5, 0.5, 0.2, 0.2, 0.1, 0.9], [0.1, 0.1, 0.1, 0.1, 0.05, 0.05]];
        let flat: Vec<f32> = rows.concat();
        let mut d = decoder(&["cube", "car"]);
        d.head = Head::Rtdetr;
        let dets = d.decode(&flat, &[1, 2, 6], &fit, 640, 640);
        assert_eq!(dets.len(), 1);
        assert_eq!(dets[0].label.as_deref(), Some("car"));
        assert!((dets[0].bbox.x1 - 0.4).abs() < 1e-6);
    }
}
