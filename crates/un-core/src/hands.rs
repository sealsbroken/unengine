//! Hand joints, finger counting and hand observations.
//!
//! The 21 joints follow MediaPipe's hand landmark order, which is also what Apple
//! Vision reports (with different names). [`FingerCounter`] is a pure geometric
//! heuristic, so the same joints always give the same count on every platform.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::geometry::{distance, Box2, Point};

/// MediaPipe landmark indices (Vision's names in comments where they differ).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HandJoint {
    Wrist = 0,
    ThumbCmc = 1,
    ThumbMcp = 2, // Vision: thumbMP
    ThumbIp = 3,
    ThumbTip = 4,
    IndexMcp = 5,
    IndexPip = 6,
    IndexDip = 7,
    IndexTip = 8,
    MiddleMcp = 9,
    MiddlePip = 10,
    MiddleDip = 11,
    MiddleTip = 12,
    RingMcp = 13,
    RingPip = 14,
    RingDip = 15,
    RingTip = 16,
    LittleMcp = 17, // MediaPipe: pinky
    LittlePip = 18,
    LittleDip = 19,
    LittleTip = 20,
}

impl HandJoint {
    pub const ALL: [HandJoint; 21] = [
        HandJoint::Wrist,
        HandJoint::ThumbCmc,
        HandJoint::ThumbMcp,
        HandJoint::ThumbIp,
        HandJoint::ThumbTip,
        HandJoint::IndexMcp,
        HandJoint::IndexPip,
        HandJoint::IndexDip,
        HandJoint::IndexTip,
        HandJoint::MiddleMcp,
        HandJoint::MiddlePip,
        HandJoint::MiddleDip,
        HandJoint::MiddleTip,
        HandJoint::RingMcp,
        HandJoint::RingPip,
        HandJoint::RingDip,
        HandJoint::RingTip,
        HandJoint::LittleMcp,
        HandJoint::LittlePip,
        HandJoint::LittleDip,
        HandJoint::LittleTip,
    ];

    pub fn from_index(index: usize) -> Option<HandJoint> {
        HandJoint::ALL.get(index).copied()
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Bones, for drawing.
pub const CONNECTIONS: [(HandJoint, HandJoint); 21] = {
    use HandJoint::*;
    [
        (Wrist, ThumbCmc),
        (ThumbCmc, ThumbMcp),
        (ThumbMcp, ThumbIp),
        (ThumbIp, ThumbTip),
        (Wrist, IndexMcp),
        (IndexMcp, IndexPip),
        (IndexPip, IndexDip),
        (IndexDip, IndexTip),
        (IndexMcp, MiddleMcp),
        (MiddleMcp, MiddlePip),
        (MiddlePip, MiddleDip),
        (MiddleDip, MiddleTip),
        (MiddleMcp, RingMcp),
        (RingMcp, RingPip),
        (RingPip, RingDip),
        (RingDip, RingTip),
        (RingMcp, LittleMcp),
        (Wrist, LittleMcp),
        (LittleMcp, LittlePip),
        (LittlePip, LittleDip),
        (LittleDip, LittleTip),
    ]
};

/// Which hand, from the child's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Chirality {
    Left,
    Right,
    Unknown,
}

/// Pure geometric heuristic for counting extended fingers from hand joints.
pub struct FingerCounter;

impl FingerCounter {
    const FOUR_FINGERS: [(HandJoint, HandJoint); 4] = [
        (HandJoint::IndexTip, HandJoint::IndexPip),
        (HandJoint::MiddleTip, HandJoint::MiddlePip),
        (HandJoint::RingTip, HandJoint::RingPip),
        (HandJoint::LittleTip, HandJoint::LittlePip),
    ];

    /// Index...little are raised when the tip is 1.15x further from the wrist than the PIP;
    /// the thumb when it is extended laterally and clearly away from the index finger.
    /// Fingers with a missing or low-confidence joint count as not raised.
    pub fn raised_fingers(
        joints: &BTreeMap<HandJoint, Point>,
        confidences: &BTreeMap<HandJoint, f32>,
        minimum_confidence: f32,
    ) -> u32 {
        let point = |joint: HandJoint| -> Option<Point> {
            let p = joints.get(&joint)?;
            (confidences.get(&joint).copied().unwrap_or(0.0) >= minimum_confidence).then_some(*p)
        };
        let Some(wrist) = point(HandJoint::Wrist) else { return 0 };
        let mut count = 0;
        for (tip_joint, pip_joint) in Self::FOUR_FINGERS {
            if let (Some(tip), Some(pip)) = (point(tip_joint), point(pip_joint)) {
                if distance(tip, wrist) > distance(pip, wrist) * 1.15 {
                    count += 1;
                }
            }
        }
        if let (Some(thumb_tip), Some(thumb_ip), Some(little_mcp), Some(index_mcp)) =
            (point(HandJoint::ThumbTip), point(HandJoint::ThumbIp), point(HandJoint::LittleMcp), point(HandJoint::IndexMcp))
        {
            let lateral = distance(thumb_tip, little_mcp) > distance(thumb_ip, little_mcp) * 1.1;
            let apart = distance(thumb_tip, index_mcp) > 0.6 * distance(index_mcp, wrist);
            if lateral && apart {
                count += 1;
            }
        }
        count
    }
}

/// One hand in a frame; joints in normalized top-left-origin coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandObservation {
    pub chirality: Chirality,
    pub joints: BTreeMap<HandJoint, Point>,
    pub joint_confidences: BTreeMap<HandJoint, f32>,
    /// Extended fingers, from [`FingerCounter`] unless a platform supplies its own.
    pub raised_finger_count: Option<u32>,
    /// The box around the joints (zero-area for a single joint).
    pub bbox: Option<Box2>,
    /// Confidence that this is a hand at all.
    pub score: f32,
}

impl HandObservation {
    pub const DEFAULT_MIN_JOINT_CONFIDENCE: f32 = 0.3;

    /// Builds an observation and derives the finger count and box from the joints.
    pub fn new(
        chirality: Chirality,
        joints: BTreeMap<HandJoint, Point>,
        joint_confidences: BTreeMap<HandJoint, f32>,
        score: f32,
    ) -> Self {
        let raised = FingerCounter::raised_fingers(&joints, &joint_confidences, Self::DEFAULT_MIN_JOINT_CONFIDENCE);
        let bbox = Box2::enclosing(joints.values().copied());
        Self { chirality, joints, joint_confidences, raised_finger_count: Some(raised), bbox, score }
    }

    /// Builds an observation from 21 joints in landmark order sharing one confidence.
    pub fn from_landmarks(chirality: Chirality, landmarks: &[Point], confidence: f32) -> Self {
        let joints: BTreeMap<HandJoint, Point> =
            landmarks.iter().take(21).enumerate().filter_map(|(i, p)| HandJoint::from_index(i).map(|j| (j, *p))).collect();
        let confidences = joints.keys().map(|j| (*j, confidence)).collect();
        Self::new(chirality, joints, confidences, confidence)
    }
}

#[cfg(test)]
mod tests {
    use super::HandJoint as J;
    use super::*;

    /// Synthetic right hand, fingers up (top-left origin, so "up" is smaller y).
    fn make(index: bool, middle: bool, ring: bool, little: bool, thumb: bool) -> (BTreeMap<J, Point>, BTreeMap<J, f32>) {
        let mut joints = BTreeMap::new();
        joints.insert(J::Wrist, (0.5, 0.8));
        let mut finger = |x: f32, mcp: J, pip: J, dip: J, tip: J, raised: bool| {
            joints.insert(mcp, (x, 0.55));
            joints.insert(pip, (x, 0.45));
            if raised {
                joints.insert(dip, (x, 0.38));
                joints.insert(tip, (x, 0.30));
            } else {
                joints.insert(dip, (x, 0.48));
                joints.insert(tip, (x, 0.55));
            }
        };
        finger(0.42, J::IndexMcp, J::IndexPip, J::IndexDip, J::IndexTip, index);
        finger(0.47, J::MiddleMcp, J::MiddlePip, J::MiddleDip, J::MiddleTip, middle);
        finger(0.52, J::RingMcp, J::RingPip, J::RingDip, J::RingTip, ring);
        finger(0.58, J::LittleMcp, J::LittlePip, J::LittleDip, J::LittleTip, little);
        joints.insert(J::ThumbCmc, (0.44, 0.72));
        if thumb {
            joints.insert(J::ThumbMcp, (0.38, 0.66));
            joints.insert(J::ThumbIp, (0.30, 0.60));
            joints.insert(J::ThumbTip, (0.22, 0.56));
        } else {
            joints.insert(J::ThumbMcp, (0.42, 0.66));
            joints.insert(J::ThumbIp, (0.40, 0.62));
            joints.insert(J::ThumbTip, (0.45, 0.60));
        }
        let conf = joints.keys().map(|j| (*j, 0.9)).collect();
        (joints, conf)
    }

    #[test]
    fn counts() {
        for (fingers, expected) in [
            ((true, true, true, true, true), 5),
            ((false, false, false, false, false), 0),
            ((true, false, false, false, false), 1),
            ((true, true, false, false, false), 2),
            ((false, false, false, false, true), 1),
        ] {
            let (j, c) = make(fingers.0, fingers.1, fingers.2, fingers.3, fingers.4);
            assert_eq!(FingerCounter::raised_fingers(&j, &c, 0.3), expected, "{fingers:?}");
        }
    }

    #[test]
    fn low_confidence_joint_is_skipped() {
        let (j, mut c) = make(true, true, true, true, true);
        c.insert(J::IndexTip, 0.1);
        assert_eq!(FingerCounter::raised_fingers(&j, &c, 0.3), 4);
    }

    #[test]
    fn observation_derives_count_and_box() {
        let (j, c) = make(true, true, false, false, false);
        let hand = HandObservation::new(Chirality::Right, j, c, 0.9);
        assert_eq!(hand.raised_finger_count, Some(2));
        let b = hand.bbox.unwrap();
        assert!(b.x1 <= 0.42 && b.y2 >= 0.8);
    }
}
