//! Debounced table state: the committed object count, stability and left/right zones.
//!
//! A pure state machine, so the same sequence of frames always yields the same states.
//! A count is *committed* once the lower median of a full window of frames has held
//! for `stable_duration` seconds.

use serde::{Deserialize, Serialize};

use crate::detection::Detection;
use crate::geometry::{Box2, FULL_FRAME};

/// Lower median (for an even count, the smaller middle value); 0 when empty.
pub fn lower_median(values: &[u32]) -> u32 {
    if values.is_empty() {
        return 0;
    }
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    ordered[(ordered.len() - 1) / 2]
}

/// What the table looks like after one frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableState {
    /// The committed count; `None` until one has settled, or after it was invalidated.
    pub cube_count: Option<u32>,
    /// Objects seen in this very frame (inside the table area).
    pub raw_count: u32,
    /// True on frames where the count is committed.
    pub is_stable: bool,
    /// Detections inside the table area, decoys removed.
    pub detections: Vec<Detection>,
    pub timestamp: f64,
    pub left_count: Option<u32>,
    pub right_count: Option<u32>,
    pub zones_stable: bool,
    /// Two objects overlap (likely stacked or touching).
    pub overlapping: bool,
    /// An object sits on the left/right midline.
    pub on_midline: bool,
    /// Width of the objects' union relative to the table area.
    pub spread: f32,
    /// How many separate groups the objects lie in (smoothed): objects closer than
    /// `EstimatorConfig::group_gap` object sizes belong to one group. 0 without objects.
    #[serde(default)]
    pub groups: u32,
}

impl TableState {
    pub fn ambiguous(&self) -> bool {
        self.overlapping || self.on_midline
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EstimatorConfig {
    pub window_frames: usize,
    pub stable_duration: f64,
    /// The table area; `None` means the whole frame.
    pub roi: Option<Box2>,
    pub midline_band: f32,
    pub overlap_iou: f32,
    /// Edge-to-edge distance, in object sizes, beyond which two objects are in different groups.
    pub group_gap: f32,
}

impl Default for EstimatorConfig {
    fn default() -> Self {
        Self { window_frames: 15, stable_duration: 0.7, roi: None, midline_band: 0.05, overlap_iou: 0.3, group_gap: 1.0 }
    }
}

/// A median window that commits once it has held for `stable_duration`.
#[derive(Debug, Default)]
struct CountChannel {
    window: Vec<u32>,
    candidate: Option<u32>,
    candidate_since: Option<f64>,
    committed: Option<u32>,
}

impl CountChannel {
    fn ingest(&mut self, raw: u32, timestamp: f64, config: &EstimatorConfig) -> bool {
        self.window.push(raw);
        if self.window.len() > config.window_frames {
            let excess = self.window.len() - config.window_frames;
            self.window.drain(..excess);
        }
        let median = lower_median(&self.window);
        if Some(median) != self.candidate {
            self.candidate = Some(median);
            self.candidate_since = Some(timestamp);
        }
        let full = self.window.len() >= config.window_frames;
        if full && self.candidate_since.is_some_and(|since| timestamp - since >= config.stable_duration) {
            self.committed = Some(median);
            return true;
        }
        if full && self.committed.is_some_and(|c| c != median) {
            // A full window disagrees with the old count: stop advertising it.
            self.committed = None;
        }
        false
    }

    fn reset(&mut self) {
        self.window.clear();
        self.candidate = None;
        self.candidate_since = None;
    }
}

#[derive(Debug)]
pub struct TableStateEstimator {
    config: EstimatorConfig,
    total: CountChannel,
    left: CountChannel,
    right: CountChannel,
    group_window: Vec<u32>,
}

impl Default for TableStateEstimator {
    fn default() -> Self {
        Self::new(EstimatorConfig::default())
    }
}

impl TableStateEstimator {
    pub fn new(config: EstimatorConfig) -> Self {
        Self {
            config,
            total: CountChannel::default(),
            left: CountChannel::default(),
            right: CountChannel::default(),
            group_window: Vec::new(),
        }
    }

    pub fn config(&self) -> &EstimatorConfig {
        &self.config
    }

    pub fn roi(&self) -> Option<Box2> {
        self.config.roi
    }

    /// Changing the table area restarts the count windows.
    pub fn set_roi(&mut self, roi: Option<Box2>) {
        if roi != self.config.roi {
            self.config.roi = roi;
            self.reset_count_window();
        }
    }

    pub fn reset_count_window(&mut self) {
        self.total.reset();
        self.left.reset();
        self.right.reset();
    }

    pub fn ingest(&mut self, detections: &[Detection], timestamp: f64) -> TableState {
        let cfg = &self.config;
        let region = cfg.roi.unwrap_or(FULL_FRAME);
        let region_mid = region.cx();

        let filtered: Vec<Detection> =
            detections.iter().filter(|d| !d.is_decoy() && region.contains(d.bbox.cx(), d.bbox.cy())).cloned().collect();
        let raw = filtered.len() as u32;
        let left_raw = filtered.iter().filter(|d| d.bbox.cx() < region_mid).count() as u32;
        let band = cfg.midline_band * region.width();
        let on_midline = filtered.iter().any(|d| (d.bbox.cx() - region_mid).abs() < band);
        let overlapping = has_overlap(&filtered, cfg.overlap_iou);
        let union = filtered.iter().fold(None::<Box2>, |acc, d| Some(acc.map_or(d.bbox, |u| u.union(&d.bbox))));
        let spread = match union {
            Some(u) if region.width() > 0.0 => u.width() / region.width(),
            _ => 0.0,
        };

        self.group_window.push(group_count(&filtered, cfg.group_gap));
        if self.group_window.len() > cfg.window_frames {
            let excess = self.group_window.len() - cfg.window_frames;
            self.group_window.drain(..excess);
        }
        let groups = lower_median(&self.group_window);

        let is_stable = self.total.ingest(raw, timestamp, &self.config);
        let left_stable = self.left.ingest(left_raw, timestamp, &self.config);
        let right_stable = self.right.ingest(raw - left_raw, timestamp, &self.config);
        let zones_stable = left_stable && right_stable;

        TableState {
            cube_count: self.total.committed,
            raw_count: raw,
            is_stable,
            detections: filtered,
            timestamp,
            left_count: self.left.committed,
            right_count: self.right.committed,
            zones_stable,
            overlapping,
            on_midline,
            spread,
            groups,
        }
    }
}

/// How many groups the objects form: two objects are in one group when the gap between their
/// boxes is under `gap` times the typical object size (the median of the boxes' larger side).
pub fn group_count(detections: &[Detection], gap: f32) -> u32 {
    let n = detections.len();
    if n == 0 {
        return 0;
    }
    let mut sizes: Vec<f32> = detections.iter().map(|d| d.bbox.width().max(d.bbox.height())).collect();
    sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let size = sizes[(n - 1) / 2].max(1e-4);
    let limit = gap * size;
    // Union-find over pairs closer than the limit.
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&detections[i].bbox, &detections[j].bbox);
            let dx = (a.x1.max(b.x1) - a.x2.min(b.x2)).max(0.0);
            let dy = (a.y1.max(b.y1) - a.y2.min(b.y2)).max(0.0);
            if (dx * dx + dy * dy).sqrt() < limit {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    (0..n).filter(|&i| root(&mut parent, i) == i).count() as u32
}

pub fn has_overlap(detections: &[Detection], threshold: f32) -> bool {
    for i in 0..detections.len() {
        for j in i + 1..detections.len() {
            if detections[i].bbox.iou(&detections[j].bbox) > threshold {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod harness {
    //! Shared builders, mirroring the Swift EstimatorHarness.
    use super::*;

    pub fn cubes(count: usize, x: f32) -> Vec<Detection> {
        (0..count)
            .map(|i| Detection::new(Box2::new(x - 0.02, 0.1 + i as f32 * 0.1, x + 0.02, 0.14 + i as f32 * 0.1), 0.9))
            .collect()
    }

    pub struct Harness {
        pub estimator: TableStateEstimator,
        pub frame: u32,
    }

    impl Harness {
        pub fn new(config: EstimatorConfig) -> Self {
            Self { estimator: TableStateEstimator::new(config), frame: 0 }
        }

        pub fn step(&mut self, detections: &[Detection]) -> TableState {
            let state = self.estimator.ingest(detections, self.frame as f64 / 30.0);
            self.frame += 1;
            state
        }

        pub fn step_cubes(&mut self, count: usize) -> TableState {
            self.step(&cubes(count, 0.5))
        }

        pub fn run(&mut self, n: usize, detections: &[Detection]) -> TableState {
            let mut last = None;
            for _ in 0..n {
                last = Some(self.step(detections));
            }
            last.expect("n > 0")
        }

        pub fn run_cubes(&mut self, n: usize, count: usize) -> TableState {
            self.run(n, &cubes(count, 0.5))
        }
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn chip(x: f32, y: f32) -> Detection {
        Detection::new(Box2::new(x, y, x + 0.05, y + 0.05), 0.9)
    }

    #[test]
    fn chips_lying_apart_form_groups() {
        assert_eq!(group_count(&[], 1.0), 0);
        // Two touching pairs with a clear gap between the pairs.
        let eyes = [chip(0.10, 0.5), chip(0.16, 0.5)];
        let ears = [chip(0.40, 0.5), chip(0.46, 0.5)];
        let all: Vec<Detection> = eyes.iter().chain(ears.iter()).cloned().collect();
        assert_eq!(group_count(&eyes, 1.0), 1);
        assert_eq!(group_count(&all, 1.0), 2);
        let mouth = chip(0.70, 0.5);
        let five: Vec<Detection> = all.iter().cloned().chain([mouth]).collect();
        assert_eq!(group_count(&five, 1.0), 3);
        // A row of evenly spaced chips with small gaps is one group.
        let row: Vec<Detection> = (0..5).map(|i| chip(0.1 + i as f32 * 0.08, 0.5)).collect();
        assert_eq!(group_count(&row, 1.0), 1);
    }

    #[test]
    fn the_estimator_reports_groups() {
        let mut estimator = TableStateEstimator::default();
        let two_groups = [chip(0.10, 0.5), chip(0.16, 0.5), chip(0.60, 0.5)];
        let mut state = estimator.ingest(&two_groups, 0.0);
        for i in 1..40 {
            state = estimator.ingest(&two_groups, i as f64 / 30.0);
        }
        assert!(state.is_stable);
        assert_eq!((state.cube_count, state.groups), (Some(3), 2));
    }
}

#[cfg(test)]
mod tests {
    use super::harness::*;
    use super::*;

    #[test]
    fn flicker_never_commits() {
        let mut h = Harness::new(EstimatorConfig::default());
        for i in 0..60 {
            let state = h.step_cubes(if i % 2 == 0 { 4 } else { 5 });
            assert!(!state.is_stable && state.cube_count.is_none());
        }
    }

    #[test]
    fn stable_five_commits_after_full_window_and_duration() {
        let mut h = Harness::new(EstimatorConfig::default());
        let mut first = None;
        for _ in 0..30 {
            let frame = h.frame;
            if h.step_cubes(5).is_stable && first.is_none() {
                first = Some(frame);
            }
        }
        let last = h.step_cubes(5);
        assert!(last.is_stable && last.cube_count == Some(5) && last.raw_count == 5);
        let first = first.expect("committed");
        assert!(first >= 14 && first as f64 / 30.0 >= 0.7);
    }

    #[test]
    fn full_window_disagreeing_invalidates_committed_count() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5);
        let state = h.run_cubes(15, 4);
        assert!(!state.is_stable && state.cube_count.is_none());
    }

    #[test]
    fn roi_filters_detections() {
        let mut h = Harness::new(EstimatorConfig { roi: Some(Box2::new(0.0, 0.0, 0.5, 1.0)), ..Default::default() });
        let inside = cubes(3, 0.25);
        let mut both = inside.clone();
        both.extend(cubes(2, 0.75));
        assert_eq!(h.step(&both).raw_count, 3);
    }

    #[test]
    fn decoys_are_never_counted() {
        let mut h = Harness::new(EstimatorConfig::default());
        let mut dets = cubes(2, 0.5);
        dets.push(Detection::labeled(Box2::new(0.1, 0.1, 0.9, 0.9), 0.9, "_keyboard"));
        assert_eq!(h.step(&dets).raw_count, 2);
    }

    #[test]
    fn changing_roi_resets_window() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5);
        h.estimator.set_roi(Some(Box2::new(0.0, 0.0, 1.0, 1.0)));
        let state = h.step_cubes(5);
        assert!(!state.is_stable && state.cube_count == Some(5));
    }

    #[test]
    fn lower_median_values() {
        assert_eq!(lower_median(&[]), 0);
        assert_eq!(lower_median(&[5, 4]), 4);
        assert_eq!(lower_median(&[1, 9, 4, 4, 2]), 4);
    }

    #[test]
    fn zones() {
        let mut h = Harness::new(EstimatorConfig::default());
        let mut dets = cubes(2, 0.2);
        dets.extend(cubes(3, 0.8));
        let state = h.run(40, &dets);
        assert_eq!((state.cube_count, state.left_count, state.right_count), (Some(5), Some(2), Some(3)));
        assert!(state.zones_stable && !state.ambiguous());
    }

    #[test]
    fn midline_and_overlap_are_ambiguous() {
        let mut dets = cubes(2, 0.2);
        dets.extend(cubes(1, 0.5));
        assert!(Harness::new(EstimatorConfig::default()).run(5, &dets).ambiguous());
        let a = Detection::new(Box2::new(0.2, 0.2, 0.3, 0.3), 0.9);
        let b = Detection::new(Box2::new(0.21, 0.21, 0.31, 0.31), 0.9);
        assert!(Harness::new(EstimatorConfig::default()).run(2, &[a, b]).ambiguous());
    }
}
