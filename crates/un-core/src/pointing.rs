//! Which numbered object the index finger is on.
//!
//! Only the index finger points: its tip comes from the hand landmarks (joint 8), which tell the
//! index finger from the thumb and the others, and it counts only when the landmarks show a
//! pointing index finger ([`pointing_tip`]): stretched out, and clearly apart from the thumb, so a
//! thumb the model took for the index finger, or a fist, points at nothing. A finger box from the object detector cannot,
//! so it never counts as pointing. The tip is on the object whose box, grown by a margin of its
//! own size, contains it (the nearest centre when several do).
//!
//! The object pointed at last time keeps its claim while the tip is still inside its grown box:
//! between two close objects the tip would otherwise flicker from one to the other, which the
//! lesson would read as the finger coming back.

use crate::geometry::{distance, Box2, Point};
use crate::hands::{HandJoint, HandObservation};

/// How far outside an object's box a fingertip may be, as a fraction of the box's size.
pub const TOUCH_MARGIN: f32 = 0.35;

/// The stable number of the object an index fingertip is on, from `objects` (number, box);
/// `previous` is the answer for the frame before, which keeps its claim while the tip is still on it.
pub fn touched_object(objects: &[(u32, Box2)], hands: &[HandObservation], margin: f32, previous: Option<u32>) -> Option<u32> {
    let tips = hands.iter().filter_map(pointing_tip);
    for tip in tips {
        let under: Vec<&(u32, Box2)> = objects.iter().filter(|(_, b)| grown(b, margin).contains(tip.0, tip.1)).collect();
        if previous.is_some_and(|p| under.iter().any(|(n, _)| *n == p)) {
            return previous;
        }
        let nearest = under.iter().min_by(|(_, a), (_, b)| {
            let (da, db) = (distance(a.center(), tip), distance(b.center(), tip));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some((number, _)) = nearest {
            return Some(*number);
        }
    }
    None
}

/// The index fingertip of a hand that points with it: the index finger stretched out (the tip
/// further from the wrist than the middle joint, and the finger straight from its knuckle) and its
/// tip clearly apart from the thumb's. A hand with only some joints (a platform that reports just
/// the fingertip) is taken at its word.
pub fn pointing_tip(hand: &HandObservation) -> Option<Point> {
    let joint = |j: HandJoint| hand.joints.get(&j).copied();
    let tip = joint(HandJoint::IndexTip)?;
    let (Some(pip), Some(mcp), Some(wrist), Some(thumb)) =
        (joint(HandJoint::IndexPip), joint(HandJoint::IndexMcp), joint(HandJoint::Wrist), joint(HandJoint::ThumbTip))
    else {
        return Some(tip);
    };
    let size = distance(wrist, mcp).max(f32::EPSILON);
    let stretched = distance(tip, wrist) > distance(pip, wrist) * 1.15;
    let straight = distance(tip, mcp) > distance(pip, mcp) * 1.5;
    let apart_from_thumb = distance(tip, thumb) > 0.5 * size;
    (stretched && straight && apart_from_thumb).then_some(tip)
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

    fn toys() -> Vec<(u32, Box2)> {
        vec![(1, Box2::new(0.1, 0.4, 0.2, 0.5)), (2, Box2::new(0.4, 0.4, 0.5, 0.5)), (3, Box2::new(0.7, 0.4, 0.8, 0.5))]
    }

    #[test]
    fn a_fingertip_on_or_just_beside_an_object_touches_it() {
        assert_eq!(touched_object(&toys(), &[hand_with_tip(0.45, 0.45)], TOUCH_MARGIN, None), Some(2));
        assert_eq!(touched_object(&toys(), &[hand_with_tip(0.52, 0.45)], TOUCH_MARGIN, None), Some(2));
        assert_eq!(touched_object(&toys(), &[hand_with_tip(0.6, 0.45)], TOUCH_MARGIN, None), None);
        assert_eq!(touched_object(&toys(), &[hand_with_tip(0.45, 0.1)], TOUCH_MARGIN, None), None);
    }

    #[test]
    fn only_the_index_fingertip_points() {
        // A hand whose landmarks have no index tip (only the thumb's): pointing at nothing.
        let mut thumb_only = hand_with_tip(0.45, 0.45);
        let tip = thumb_only.joints.remove(&HandJoint::IndexTip).unwrap();
        thumb_only.joints.insert(HandJoint::ThumbTip, tip);
        assert_eq!(touched_object(&toys(), &[thumb_only], TOUCH_MARGIN, None), None);
        // A hand seen only as a box, without landmarks, does not point either.
        let mut boxed = hand_with_tip(0.45, 0.45);
        boxed.joints.clear();
        boxed.bbox = Some(Box2::new(0.4, 0.4, 0.5, 0.5));
        assert_eq!(touched_object(&toys(), &[boxed], TOUCH_MARGIN, None), None);
    }

    /// A right hand from the wrist at (0.5, 0.9) up; the index finger pointing at `tip` or bent.
    fn hand(index_out: bool, thumb_tip: (f32, f32)) -> HandObservation {
        let mut joints = BTreeMap::new();
        joints.insert(HandJoint::Wrist, (0.5, 0.9));
        joints.insert(HandJoint::IndexMcp, (0.48, 0.75));
        joints.insert(HandJoint::IndexPip, (0.47, 0.68));
        joints.insert(HandJoint::IndexTip, if index_out { (0.45, 0.55) } else { (0.49, 0.72) });
        joints.insert(HandJoint::ThumbTip, thumb_tip);
        HandObservation {
            chirality: Chirality::Right,
            joints,
            joint_confidences: BTreeMap::new(),
            raised_finger_count: Some(1),
            bbox: None,
            score: 0.9,
        }
    }

    #[test]
    fn only_a_stretched_index_finger_away_from_the_thumb_points() {
        let toy = vec![(1, Box2::new(0.4, 0.5, 0.5, 0.6))];
        // Pointing: index out, thumb tucked by the palm.
        assert_eq!(touched_object(&toy, &[hand(true, (0.58, 0.8))], TOUCH_MARGIN, None), Some(1));
        // A fist, or the index curled: nothing.
        assert_eq!(touched_object(&toy, &[hand(false, (0.58, 0.8))], TOUCH_MARGIN, None), None);
        // The "index" tip on the thumb's: the model took the thumb for the index finger.
        assert_eq!(touched_object(&toy, &[hand(true, (0.46, 0.56))], TOUCH_MARGIN, None), None);
    }

    #[test]
    fn a_fingertip_between_two_close_objects_stays_on_the_one_it_was_on() {
        let close = vec![(1, Box2::new(0.40, 0.4, 0.50, 0.5)), (2, Box2::new(0.52, 0.4, 0.62, 0.5))];
        // Inside both grown boxes, nearer the first's centre.
        assert_eq!(touched_object(&close, &[hand_with_tip(0.50, 0.45)], TOUCH_MARGIN, None), Some(1));
        assert_eq!(touched_object(&close, &[hand_with_tip(0.50, 0.45)], TOUCH_MARGIN, Some(2)), Some(2));
        // Clear of the second's grown box: the first, whatever it was before.
        assert_eq!(touched_object(&close, &[hand_with_tip(0.42, 0.45)], TOUCH_MARGIN, Some(2)), Some(1));
    }
}
