//! Axis-aligned boxes and points in normalized image coordinates.
//!
//! Origin top-left, x to the right, y down, both axes 0...1, on every platform
//! (iOS Vision's bottom-left origin is converted at the edge).

use serde::{Deserialize, Serialize};

/// A point in normalized image coordinates.
pub type Point = (f32, f32);

/// An axis-aligned box in normalized image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Box2 {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// The whole frame.
pub const FULL_FRAME: Box2 = Box2 { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0 };

impl Box2 {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    /// The box with corners in any order.
    pub fn from_corners(ax: f32, ay: f32, bx: f32, by: f32) -> Self {
        Self::new(ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
    }

    pub fn width(&self) -> f32 {
        self.x2 - self.x1
    }

    pub fn height(&self) -> f32 {
        self.y2 - self.y1
    }

    pub fn cx(&self) -> f32 {
        (self.x1 + self.x2) / 2.0
    }

    pub fn cy(&self) -> f32 {
        (self.y1 + self.y2) / 2.0
    }

    pub fn center(&self) -> Point {
        (self.cx(), self.cy())
    }

    pub fn area(&self) -> f32 {
        self.width().max(0.0) * self.height().max(0.0)
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.x1 <= x && x <= self.x2 && self.y1 <= y && y <= self.y2
    }

    pub fn intersection(&self, other: &Box2) -> Option<Box2> {
        let (x1, y1) = (self.x1.max(other.x1), self.y1.max(other.y1));
        let (x2, y2) = (self.x2.min(other.x2), self.y2.min(other.y2));
        if x2 < x1 || y2 < y1 {
            None
        } else {
            Some(Box2::new(x1, y1, x2, y2))
        }
    }

    /// True when the boxes share a positive area.
    pub fn intersects(&self, other: &Box2) -> bool {
        self.intersection(other).is_some_and(|i| i.area() > 0.0)
    }

    /// Intersection over union; 0 when the boxes do not overlap.
    pub fn iou(&self, other: &Box2) -> f32 {
        let Some(inter) = self.intersection(other) else { return 0.0 };
        let inter_area = inter.area();
        if inter_area <= 0.0 {
            return 0.0;
        }
        let union = self.area() + other.area() - inter_area;
        if union > 0.0 {
            inter_area / union
        } else {
            0.0
        }
    }

    pub fn union(&self, other: &Box2) -> Box2 {
        Box2::new(self.x1.min(other.x1), self.y1.min(other.y1), self.x2.max(other.x2), self.y2.max(other.y2))
    }

    /// The box clipped to the frame.
    pub fn clipped(&self) -> Box2 {
        let c = |v: f32| v.clamp(0.0, 1.0);
        Box2::new(c(self.x1), c(self.y1), c(self.x2), c(self.y2))
    }

    /// The smallest box around the points; `None` when there are none.
    pub fn enclosing<I: IntoIterator<Item = Point>>(points: I) -> Option<Box2> {
        let mut result: Option<Box2> = None;
        for (x, y) in points {
            result = Some(match result {
                None => Box2::new(x, y, x, y),
                Some(b) => Box2::new(b.x1.min(x), b.y1.min(y), b.x2.max(x), b.y2.max(y)),
            });
        }
        result
    }
}

/// Euclidean distance between two points.
pub fn distance(a: Point, b: Point) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iou_and_union() {
        let a = Box2::new(0.0, 0.0, 1.0, 1.0);
        let b = Box2::new(0.5, 0.5, 1.5, 1.5);
        assert!((a.iou(&b) - 0.25 / 1.75).abs() < 1e-6);
        assert_eq!(a.union(&b), Box2::new(0.0, 0.0, 1.5, 1.5));
        assert!(!a.intersects(&Box2::new(1.0, 1.0, 2.0, 2.0)));
        assert_eq!(a.iou(&Box2::new(2.0, 2.0, 3.0, 3.0)), 0.0);
    }

    #[test]
    fn enclosing_points() {
        assert_eq!(Box2::enclosing(Vec::<Point>::new()), None);
        assert_eq!(Box2::enclosing([(0.2, 0.9), (0.5, 0.1)]), Some(Box2::new(0.2, 0.1, 0.5, 0.9)));
    }
}
