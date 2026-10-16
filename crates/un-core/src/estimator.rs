//! Debounced table state: the committed object count, stability and left/right zones.
//!
//! A pure state machine, so the same sequence of frames always yields the same states.
//! A count is *committed* once the lower median of a full window of frames has held
//! for `stable_duration` seconds with no hand over the table.

use serde::{Deserialize, Serialize};

use crate::detection::Detection;
use crate::geometry::{Box2, FULL_FRAME};
use crate::hands::HandObservation;

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
    /// True on frames where the count is committed and no hand is over the table.
    pub is_stable: bool,
    pub hands_present: bool,
    /// Smoothed total of raised fingers; `None` without hands.
    pub finger_count: Option<u32>,
    /// Detections inside the table area, decoys removed.
    pub detections: Vec<Detection>,
    pub hands: Vec<HandObservation>,
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
    /// The stable number of the object a finger is on (see [`crate::pointing`]); set by the
    /// perception pipeline after tracking, `None` when no finger is on an object.
    #[serde(default)]
    pub pointed_at: Option<u32>,
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
    pub finger_window_frames: usize,
    /// The table area; `None` means the whole frame.
    pub roi: Option<Box2>,
    pub hand_confidence: f32,
    pub midline_band: f32,
    pub overlap_iou: f32,
    /// Edge-to-edge distance, in object sizes, beyond which two objects are in different groups.
    pub group_gap: f32,
}

impl Default for EstimatorConfig {
    fn default() -> Self {
        Self {
            window_frames: 15,
            stable_duration: 0.7,
            finger_window_frames: 10,
            roi: None,
            hand_confidence: 0.3,
            midline_band: 0.05,
            overlap_iou: 0.3,
            group_gap: 1.0,
        }
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
    finger_window: Vec<u32>,
    group_window: Vec<u32>,
    last_hands_present: bool,
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
            finger_window: Vec::new(),
            group_window: Vec::new(),
            last_hands_present: false,
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

    pub fn ingest(&mut self, detections: &[Detection], hands: &[HandObservation], timestamp: f64) -> TableState {
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

        let hands_present = hands.iter().any(|h| overlaps(&region, h.bbox.as_ref()));

        self.group_window.push(group_count(&filtered, cfg.group_gap));
        if self.group_window.len() > cfg.window_frames {
            let excess = self.group_window.len() - cfg.window_frames;
            self.group_window.drain(..excess);
        }
        let groups = lower_median(&self.group_window);

        let (mut is_stable, mut zones_stable) = (false, false);
        if hands_present {
            if !self.last_hands_present {
                self.reset_count_window();
            }
        } else {
            is_stable = self.total.ingest(raw, timestamp, &self.config);
            let left_stable = self.left.ingest(left_raw, timestamp, &self.config);
            let right_stable = self.right.ingest(raw - left_raw, timestamp, &self.config);
            zones_stable = left_stable && right_stable;
        }
        self.last_hands_present = hands_present;

        // Fingers are counted on hands with landmarks; a hand seen only as a box is present but uncounted.
        let counted: Vec<u32> = hands.iter().filter_map(|h| h.raised_finger_count).collect();
        let finger_count = if counted.is_empty() {
            self.finger_window.clear();
            None
        } else {
            self.finger_window.push(counted.iter().sum());
            let limit = self.config.finger_window_frames;
            if self.finger_window.len() > limit {
                let excess = self.finger_window.len() - limit;
                self.finger_window.drain(..excess);
            }
            Some(lower_median(&self.finger_window))
        };

        TableState {
            cube_count: self.total.committed,
            raw_count: raw,
            is_stable,
            hands_present,
            finger_count,
            detections: filtered,
            hands: hands.to_vec(),
            timestamp,
            left_count: self.left.committed,
            right_count: self.right.committed,
            zones_stable,
            overlapping,
            on_midline,
            spread,
            groups,
            pointed_at: None,
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

/// A single-joint hand has a zero-area box; containment of its centre counts as overlap too.
fn overlaps(region: &Box2, bbox: Option<&Box2>) -> bool {
    match bbox {
        None => false,
        Some(b) => region.intersects(b) || region.contains(b.cx(), b.cy()),
    }
}

#[cfg(test)]
pub(crate) mod harness {
    //! Shared builders, mirroring the Swift EstimatorHarness.
    use std::collections::BTreeMap;

    use super::*;
    use crate::hands::{Chirality, HandJoint};

    pub fn cubes(count: usize, x: f32) -> Vec<Detection> {
        (0..count)
            .map(|i| Detection::new(Box2::new(x - 0.02, 0.1 + i as f32 * 0.1, x + 0.02, 0.14 + i as f32 * 0.1), 0.9))
            .collect()
    }

    pub fn hand(fingers: u32, bbox: Box2) -> HandObservation {
        let mut joints = BTreeMap::new();
        joints.insert(HandJoint::Wrist, bbox.center());
        let mut conf = BTreeMap::new();
        conf.insert(HandJoint::Wrist, 0.9);
        HandObservation {
            chirality: Chirality::Right,
            joints,
            joint_confidences: conf,
            raised_finger_count: Some(fingers),
            bbox: Some(bbox),
            score: 1.0,
        }
    }

    pub fn default_hand(fingers: u32) -> HandObservation {
        hand(fingers, Box2::new(0.4, 0.4, 0.6, 0.7))
    }

    pub struct Harness {
        pub estimator: TableStateEstimator,
        pub frame: u32,
    }

    impl Harness {
        pub fn new(config: EstimatorConfig) -> Self {
            Self { estimator: TableStateEstimator::new(config), frame: 0 }
        }

        pub fn step(&mut self, detections: &[Detection], hands: &[HandObservation]) -> TableState {
            let state = self.estimator.ingest(detections, hands, self.frame as f64 / 30.0);
            self.frame += 1;
            state
        }

        pub fn step_cubes(&mut self, count: usize, hands: &[HandObservation]) -> TableState {
            self.step(&cubes(count, 0.5), hands)
        }

        pub fn run(&mut self, n: usize, detections: &[Detection], hands: &[HandObservation]) -> TableState {
            let mut last = None;
            for _ in 0..n {
                last = Some(self.step(detections, hands));
            }
            last.expect("n > 0")
        }

        pub fn run_cubes(&mut self, n: usize, count: usize, hands: &[HandObservation]) -> TableState {
            self.run(n, &cubes(count, 0.5), hands)
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
        let mut state = estimator.ingest(&two_groups, &[], 0.0);
        for i in 1..40 {
            state = estimator.ingest(&two_groups, &[], i as f64 / 30.0);
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
            let state = h.step_cubes(if i % 2 == 0 { 4 } else { 5 }, &[]);
            assert!(!state.is_stable && state.cube_count.is_none());
        }
    }

    #[test]
    fn stable_five_commits_after_full_window_and_duration() {
        let mut h = Harness::new(EstimatorConfig::default());
        let mut first = None;
        for _ in 0..30 {
            let frame = h.frame;
            if h.step_cubes(5, &[]).is_stable && first.is_none() {
                first = Some(frame);
            }
        }
        let last = h.step_cubes(5, &[]);
        assert!(last.is_stable && last.cube_count == Some(5) && last.raw_count == 5);
        assert!(!last.hands_present && last.finger_count.is_none());
        let first = first.expect("committed");
        assert!(first >= 14 && first as f64 / 30.0 >= 0.7);
    }

    #[test]
    fn hand_entering_marks_unstable_but_keeps_count() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5, &[]);
        let state = h.step_cubes(5, &[default_hand(2)]);
        assert!(state.hands_present && !state.is_stable);
        assert_eq!((state.cube_count, state.finger_count), (Some(5), Some(2)));
    }

    #[test]
    fn after_hands_leave_new_count_needs_fresh_window() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5, &[]);
        h.run_cubes(15, 5, &[default_hand(5)]);
        let states: Vec<TableState> = (0..40).map(|_| h.step_cubes(4, &[])).collect();
        let first = states.iter().position(|s| s.is_stable).expect("stable");
        assert!(first >= 14 && states[first].cube_count == Some(4));
        assert!(!states.iter().any(|s| s.is_stable && s.cube_count == Some(5)));
        let last = states.last().unwrap();
        assert!(last.cube_count == Some(4) && last.is_stable);
    }

    #[test]
    fn full_window_disagreeing_invalidates_committed_count() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5, &[]);
        h.run_cubes(5, 5, &[default_hand(1)]);
        let state = h.run_cubes(15, 4, &[]);
        assert!(!state.is_stable && state.cube_count.is_none());
    }

    #[test]
    fn roi_filters_detections_and_hands() {
        let mut h = Harness::new(EstimatorConfig { roi: Some(Box2::new(0.0, 0.0, 0.5, 1.0)), ..Default::default() });
        let inside = cubes(3, 0.25);
        let mut both = inside.clone();
        both.extend(cubes(2, 0.75));
        assert_eq!(h.step(&both, &[]).raw_count, 3);
        let far = h.step(&inside, &[hand(3, Box2::new(0.6, 0.2, 0.8, 0.5))]);
        assert!(!far.hands_present && far.finger_count == Some(3));
        assert!(h.step(&inside, &[hand(3, Box2::new(0.45, 0.2, 0.65, 0.5))]).hands_present);
        assert!(h.step(&inside, &[hand(0, Box2::new(0.2, 0.2, 0.2, 0.2))]).hands_present);
    }

    #[test]
    fn decoys_are_never_counted() {
        let mut h = Harness::new(EstimatorConfig::default());
        let mut dets = cubes(2, 0.5);
        dets.push(Detection::labeled(Box2::new(0.1, 0.1, 0.9, 0.9), 0.9, "_keyboard"));
        assert_eq!(h.step(&dets, &[]).raw_count, 2);
    }

    #[test]
    fn changing_roi_resets_window() {
        let mut h = Harness::new(EstimatorConfig::default());
        h.run_cubes(30, 5, &[]);
        h.estimator.set_roi(Some(Box2::new(0.0, 0.0, 1.0, 1.0)));
        let state = h.step_cubes(5, &[]);
        assert!(!state.is_stable && state.cube_count == Some(5));
    }

    #[test]
    fn finger_count_smoothed_and_none_without_hands() {
        let mut h = Harness::new(EstimatorConfig { finger_window_frames: 5, ..Default::default() });
        let mut last = None;
        for f in [2, 2, 5, 2, 2] {
            last = Some(h.step_cubes(0, &[default_hand(f)]));
        }
        assert_eq!(last.unwrap().finger_count, Some(2));
        assert_eq!(h.step_cubes(0, &[]).finger_count, None);
        assert_eq!(h.step_cubes(0, &[default_hand(4)]).finger_count, Some(4));
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
        let state = h.run(40, &dets, &[]);
        assert_eq!((state.cube_count, state.left_count, state.right_count), (Some(5), Some(2), Some(3)));
        assert!(state.zones_stable && !state.ambiguous());
        let frozen = h.run(40, &[], &[default_hand(0)]);
        assert_eq!((frozen.left_count, frozen.right_count), (Some(2), Some(3)));
        assert!(!frozen.zones_stable);
    }

    #[test]
    fn midline_and_overlap_are_ambiguous() {
        let mut dets = cubes(2, 0.2);
        dets.extend(cubes(1, 0.5));
        assert!(Harness::new(EstimatorConfig::default()).run(5, &dets, &[]).ambiguous());
        let a = Detection::new(Box2::new(0.2, 0.2, 0.3, 0.3), 0.9);
        let b = Detection::new(Box2::new(0.21, 0.21, 0.31, 0.31), 0.9);
        assert!(Harness::new(EstimatorConfig::default()).run(2, &[a, b], &[]).ambiguous());
    }
}
