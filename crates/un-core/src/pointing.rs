//! Which numbered object a finger is on.
//!
//! Two sources, in order of trust: the index fingertip from the hand landmarks, a point; and
//! the detector's `finger` boxes, which cover the whole finger. A fingertip is on the object
//! whose box, grown by a margin of its own size, contains it (the nearest centre when several
//! do). A finger box is on the object it covers most.

use crate::detection::Detection;
use crate::geometry::{distance, Box2};
use crate::hands::{HandJoint, HandObservation};

/// How far outside an object's box a fingertip may be, as a fraction of the box's size.
pub const TOUCH_MARGIN: f32 = 0.35;

/// The stable number of the object a finger is on, from `objects` (number, box).
pub fn touched_object(objects: &[(u32, Box2)], hands: &[HandObservation], fingers: &[Detection], margin: f32) -> Option<u32> {
    let tips = hands.iter().filter_map(|h| h.joints.get(&HandJoint::IndexTip).copied());
    for tip in tips {
        let nearest = objects.iter().filter(|(_, b)| grown(b, margin).contains(tip.0, tip.1)).min_by(|(_, a), (_, b)| {
            let (da, db) = (distance(a.center(), tip), distance(b.center(), tip));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some((number, _)) = nearest {
            return Some(*number);
        }
    }
    for finger in fingers {
        let covered = objects
            .iter()
            .filter_map(|(number, b)| {
                let inter = b.intersection(&finger.bbox)?.area();
                (inter > 0.0).then(|| (inter / b.area().max(f32::EPSILON), *number))
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((_, number)) = covered {
            return Some(number);
        }
    }
    None
}

fn grown(b: &Box2, margin: f32) -> Box2 {
    let (dx, dy) = (b.width() * margin, b.height() * margin);
    Box2::new(b.x1 - dx, b.y1 - dy, b.x2 + dx, b.y2 + dy)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::hands::Chirality;

    fn hand_with_tip(x: f32, y: f32) -> HandObservation {
        let mut joints = BTreeMap::new();
        joints.insert(HandJoint::IndexTip, (x, y));
        HandObservation {
            chirality: Chirality::Right,
            joints,
            joint_confidences: BTreeMap::new(),
            raised_finger_count: Some(1),
            bbox: None,
            score: 0.9,
        }
    }

    fn caps() -> Vec<(u32, Box2)> {
        vec![(1, Box2::new(0.1, 0.4, 0.2, 0.5)), (2, Box2::new(0.4, 0.4, 0.5, 0.5)), (3, Box2::new(0.7, 0.4, 0.8, 0.5))]
    }

    #[test]
    fn a_fingertip_on_or_just_beside_an_object_touches_it() {
        assert_eq!(touched_object(&caps(), &[hand_with_tip(0.45, 0.45)], &[], TOUCH_MARGIN), Some(2));
        assert_eq!(touched_object(&caps(), &[hand_with_tip(0.52, 0.45)], &[], TOUCH_MARGIN), Some(2));
        assert_eq!(touched_object(&caps(), &[hand_with_tip(0.6, 0.45)], &[], TOUCH_MARGIN), None);
        assert_eq!(touched_object(&caps(), &[hand_with_tip(0.45, 0.1)], &[], TOUCH_MARGIN), None);
    }

    #[test]
    fn a_finger_box_touches_the_object_it_covers_most() {
        let finger = Detection::labeled(Box2::new(0.45, 0.2, 0.78, 0.48), 0.5, "finger");
        assert_eq!(touched_object(&caps(), &[], &[finger], TOUCH_MARGIN), Some(3));
        let beside = Detection::labeled(Box2::new(0.25, 0.1, 0.35, 0.3), 0.5, "finger");
        assert_eq!(touched_object(&caps(), &[], &[beside], TOUCH_MARGIN), None);
    }

    #[test]
    fn the_fingertip_wins_over_the_finger_box() {
        let finger = Detection::labeled(Box2::new(0.65, 0.2, 0.85, 0.48), 0.5, "finger");
        assert_eq!(touched_object(&caps(), &[hand_with_tip(0.15, 0.45)], &[finger], TOUCH_MARGIN), Some(1));
    }
}
