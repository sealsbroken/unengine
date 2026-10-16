//! Gives each object a number that stays with it while it moves.
//!
//! Detections are matched greedily to tracks by centre distance relative to box
//! size. A new object needs `min_hits` consecutive frames before it is numbered;
//! a numbered one survives `max_missed` missed frames. Numbers are 1...n by order
//! of appearance among live tracks, so a vanished false positive leaves no gap.

use serde::{Deserialize, Serialize};

use crate::detection::Detection;
use crate::geometry::Box2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackedDetection {
    /// 1-based, in order of appearance among live tracks.
    pub number: u32,
    pub detection: Detection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackerConfig {
    pub min_hits: u32,
    /// Frames a numbered object survives unseen: long enough (about a second and a half at
    /// camera rate) for a finger resting on it while a number is said.
    pub max_missed: u32,
    pub max_distance: f32,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self { min_hits: 2, max_missed: 40, max_distance: 1.5 }
    }
}

#[derive(Debug)]
struct Track {
    serial: Option<u64>,
    bbox: Box2,
    hits: u32,
    missed: u32,
}

#[derive(Debug, Default)]
pub struct DetectionTracker {
    config: TrackerConfig,
    tracks: Vec<Track>,
    next_serial: u64,
}

impl DetectionTracker {
    pub fn new(config: TrackerConfig) -> Self {
        Self { config, tracks: Vec::new(), next_serial: 0 }
    }

    pub fn reset(&mut self) {
        self.tracks.clear();
        self.next_serial = 0;
    }

    /// Every numbered object with its last box, including those missed for a few frames
    /// (a finger on an object may hide it from the detector).
    pub fn numbered(&self) -> Vec<(u32, Box2)> {
        let mut live: Vec<(u64, Box2)> = self.tracks.iter().filter_map(|t| t.serial.map(|s| (s, t.bbox))).collect();
        live.sort_by_key(|(serial, _)| *serial);
        live.into_iter().enumerate().map(|(i, (_, bbox))| (i as u32 + 1, bbox)).collect()
    }

    pub fn update(&mut self, detections: &[Detection]) -> Vec<TrackedDetection> {
        let cfg = &self.config;
        let tracks = &mut self.tracks;

        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (ti, t) in tracks.iter().enumerate() {
            for (di, d) in detections.iter().enumerate() {
                let dist = distance(&t.bbox, &d.bbox);
                if dist <= cfg.max_distance {
                    pairs.push((dist, ti, di));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut track_matched = vec![false; tracks.len()];
        let mut detection_track: Vec<Option<usize>> = vec![None; detections.len()];
        for (_, ti, di) in pairs {
            if !track_matched[ti] && detection_track[di].is_none() {
                track_matched[ti] = true;
                detection_track[di] = Some(ti);
            }
        }

        for (ti, t) in tracks.iter_mut().enumerate() {
            if !track_matched[ti] {
                t.missed += 1;
                if t.serial.is_none() {
                    t.hits = 0;
                }
            }
        }
        for (di, d) in detections.iter().enumerate() {
            match detection_track[di] {
                Some(ti) => {
                    let t = &mut tracks[ti];
                    t.bbox = d.bbox;
                    t.hits += 1;
                    t.missed = 0;
                }
                None => {
                    tracks.push(Track { serial: None, bbox: d.bbox, hits: 1, missed: 0 });
                    detection_track[di] = Some(tracks.len() - 1);
                }
            }
        }

        for t in tracks.iter_mut() {
            if t.serial.is_none() && t.hits >= cfg.min_hits {
                t.serial = Some(self.next_serial);
                self.next_serial += 1;
            }
        }

        let lost: Vec<bool> = tracks.iter().map(|t| t.missed > if t.serial.is_none() { 0 } else { cfg.max_missed }).collect();
        let mut live: Vec<u64> =
            tracks.iter().zip(&lost).filter(|(t, gone)| !**gone && t.serial.is_some()).map(|(t, _)| t.serial.unwrap()).collect();
        live.sort_unstable();

        let mut result: Vec<TrackedDetection> = Vec::new();
        for (di, d) in detections.iter().enumerate() {
            let serial = detection_track[di].and_then(|ti| tracks[ti].serial);
            if let Some(serial) = serial {
                if let Ok(index) = live.binary_search(&serial) {
                    result.push(TrackedDetection { number: index as u32 + 1, detection: d.clone() });
                }
            }
        }

        let mut keep = lost.iter().map(|gone| !gone);
        tracks.retain(|_| keep.next().unwrap_or(false));
        result.sort_by_key(|r| r.number);
        result
    }
}

/// Centre distance in multiples of the larger side of the two boxes.
pub fn distance(a: &Box2, b: &Box2) -> f32 {
    let scale = a.width().max(a.height()).max(b.width()).max(b.height()).max(0.001);
    ((a.cx() - b.cx()).powi(2) + (a.cy() - b.cy()).powi(2)).sqrt() / scale
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn toy(x: f32, y: f32) -> Detection {
        Detection::new(Box2::new(x, y, x + 0.05, y + 0.05), 0.9)
    }

    fn numbers(tracked: &[TrackedDetection]) -> BTreeMap<u32, i32> {
        tracked.iter().map(|t| (t.number, (t.detection.bbox.x1 * 10000.0).round() as i32)).collect()
    }

    fn map(pairs: &[(u32, f32)]) -> BTreeMap<u32, i32> {
        pairs.iter().map(|(n, x)| (*n, (x * 10000.0).round() as i32)).collect()
    }

    #[test]
    fn new_object_needs_two_frames() {
        let mut t = DetectionTracker::default();
        assert!(t.update(&[toy(0.5, 0.5)]).is_empty());
        assert_eq!(t.update(&[toy(0.5, 0.5)]).iter().map(|d| d.number).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn number_follows_object_across_the_other() {
        let mut t = DetectionTracker::default();
        t.update(&[toy(0.5, 0.5)]);
        t.update(&[toy(0.5, 0.5)]);
        t.update(&[toy(0.5, 0.5), toy(0.8, 0.5)]);
        assert_eq!(numbers(&t.update(&[toy(0.5, 0.5), toy(0.8, 0.5)])), map(&[(1, 0.5), (2, 0.8)]));
        let mut x = 0.8f32;
        while x > 0.2 {
            x -= 0.04;
            t.update(&[toy(x, 0.5), toy(0.5, 0.5)]);
        }
        assert_eq!(numbers(&t.update(&[toy(x, 0.5), toy(0.5, 0.5)])), map(&[(1, 0.5), (2, x)]));
    }

    #[test]
    fn brief_miss_keeps_numbers_long_miss_frees_them() {
        let mut t = DetectionTracker::new(TrackerConfig { min_hits: 1, max_missed: 3, ..Default::default() });
        t.update(&[toy(0.2, 0.5), toy(0.7, 0.5)]);
        for _ in 0..3 {
            t.update(&[toy(0.2, 0.5)]);
        }
        // The missed one is still numbered, at its last box.
        assert_eq!(t.numbered().iter().map(|(n, b)| (*n, b.x1)).collect::<Vec<_>>(), vec![(1, 0.2), (2, 0.7)]);
        assert_eq!(numbers(&t.update(&[toy(0.2, 0.5), toy(0.7, 0.5)])), map(&[(1, 0.2), (2, 0.7)]));
        for _ in 0..5 {
            t.update(&[toy(0.7, 0.5)]);
        }
        assert_eq!(numbers(&t.update(&[toy(0.7, 0.5), toy(0.4, 0.5)])), map(&[(1, 0.7), (2, 0.4)]));
    }

    #[test]
    fn single_frame_flicker_never_numbered() {
        let mut t = DetectionTracker::default();
        t.update(&[toy(0.5, 0.5)]);
        t.update(&[toy(0.5, 0.5)]);
        t.update(&[toy(0.5, 0.5), toy(0.9, 0.1)]);
        let nums = |r: Vec<TrackedDetection>| r.iter().map(|d| d.number).collect::<Vec<_>>();
        assert_eq!(nums(t.update(&[toy(0.5, 0.5)])), vec![1]);
        assert_eq!(nums(t.update(&[toy(0.5, 0.5), toy(0.3, 0.5)])), vec![1]);
        assert_eq!(nums(t.update(&[toy(0.5, 0.5), toy(0.3, 0.5)])), vec![1, 2]);
    }

    #[test]
    fn vanished_false_positive_leaves_no_gap() {
        let mut t = DetectionTracker::new(TrackerConfig { min_hits: 2, max_missed: 3, ..Default::default() });
        for _ in 0..2 {
            t.update(&[toy(0.5, 0.5)]);
        }
        for _ in 0..2 {
            t.update(&[toy(0.5, 0.5), toy(0.9, 0.9)]);
        }
        for _ in 0..2 {
            t.update(&[toy(0.5, 0.5), toy(0.2, 0.5)]);
        }
        assert_eq!(numbers(&t.update(&[toy(0.5, 0.5), toy(0.2, 0.5)])), map(&[(1, 0.5), (3, 0.2)]));
        assert_eq!(numbers(&t.update(&[toy(0.5, 0.5), toy(0.2, 0.5)])), map(&[(1, 0.5), (2, 0.2)]));
    }
}
