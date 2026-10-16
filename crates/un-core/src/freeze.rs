//! Holds the toys' layout while a hand is over the table.
//!
//! A finger over a toy can split it into two boxes, and a hand can hide one: tracked live, the
//! split half gets a number of its own and the hidden toy may lose its own, so a fingertip lands
//! on a toy that is not there and the count under the finger goes wrong. While a hand is over the
//! table the layout of the last frame without one (each numbered toy and its box) is held
//! instead: no toy appears, disappears or is renumbered, and the fingertip is matched against it.
//! When the hand leaves, tracking goes on from where it stopped, and the table settles again
//! before anything is counted (the estimator never counts with a hand in view).

use crate::detection::Detection;
use crate::geometry::Box2;
use crate::tracker::{DetectionTracker, TrackedDetection};

#[derive(Debug, Clone, Default)]
struct Layout {
    tracked: Vec<TrackedDetection>,
    numbered: Vec<(u32, Box2)>,
}

#[derive(Debug, Default)]
pub struct LayoutFreeze {
    /// The layout held while a hand is over the table.
    held: Option<Layout>,
    /// The last layout seen without a hand.
    last: Layout,
}

impl LayoutFreeze {
    /// One frame: with a hand over the table the held layout (taken from the last frame without
    /// one), otherwise the tracker updated with `detections`. Returns the tracked toys and every
    /// numbered toy's box, for pointing.
    pub fn step(
        &mut self,
        hand_over_table: bool,
        tracker: &mut DetectionTracker,
        detections: &[Detection],
    ) -> (Vec<TrackedDetection>, Vec<(u32, Box2)>) {
        if hand_over_table {
            let held = self.held.get_or_insert_with(|| self.last.clone());
            return (held.tracked.clone(), held.numbered.clone());
        }
        self.held = None;
        let tracked = tracker.update(detections);
        self.last = Layout { tracked: tracked.clone(), numbered: tracker.numbered() };
        (tracked, self.last.numbered.clone())
    }

    pub fn holding(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracker::TrackerConfig;

    fn toy(x: f32) -> Detection {
        Detection::labeled(Box2::new(x, 0.4, x + 0.1, 0.5), 0.9, "toy")
    }

    #[test]
    fn a_finger_splitting_or_hiding_a_toy_changes_nothing_until_the_hand_leaves() {
        let mut tracker = DetectionTracker::new(TrackerConfig::default());
        let mut freeze = LayoutFreeze::default();
        let two = [toy(0.2), toy(0.6)];
        for _ in 0..3 {
            freeze.step(false, &mut tracker, &two);
        }
        let (before, boxes) = freeze.step(false, &mut tracker, &two);
        assert_eq!(before.len(), 2);
        assert_eq!(boxes.len(), 2);
        // A hand comes: the second toy is split in two by the finger, the first is hidden.
        let split = [toy(0.55), toy(0.65)];
        for _ in 0..5 {
            let (tracked, numbered) = freeze.step(true, &mut tracker, &split);
            assert_eq!(tracked, before);
            assert_eq!(numbered, boxes);
        }
        assert!(freeze.holding());
        // The hand leaves: tracking goes on with the toys where they are, numbers kept.
        let (after, numbered) = freeze.step(false, &mut tracker, &two);
        assert!(!freeze.holding());
        assert_eq!(numbered, boxes);
        assert_eq!(after.iter().map(|t| t.number).collect::<Vec<_>>(), vec![1, 2]);
    }
}
