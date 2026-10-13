//! Stand-in for camera + detector: a table with a configurable number of objects.
//!
//! The fake detections go through the real estimator and tracker, so the whole lesson
//! loop runs on any machine without a camera, and CI gets a deterministic table.

use parking_lot::Mutex;
use un_core::{Box2, Detection};

pub struct SimulatedTable {
    count: Mutex<u32>,
    pub width: u32,
    pub height: u32,
}

impl SimulatedTable {
    pub fn new(count: u32) -> Self {
        Self { count: Mutex::new(count), width: 1280, height: 720 }
    }

    pub fn count(&self) -> u32 {
        *self.count.lock()
    }

    pub fn set_count(&self, value: u32) {
        *self.count.lock() = value;
    }

    pub fn add(&self, delta: i32) {
        let mut c = self.count.lock();
        *c = (*c as i32 + delta).max(0) as u32;
    }

    pub fn detections(&self) -> Vec<Detection> {
        let n = self.count();
        if n == 0 {
            return vec![];
        }
        let step = 0.8 / n as f32;
        let w = step.min(0.12);
        let h = w * 0.6 * self.width as f32 / self.height as f32;
        (0..n)
            .map(|i| {
                Detection::labeled(Box2::new(0.1 + i as f32 * step, 0.45, 0.1 + i as f32 * step + w, 0.45 + h), 0.95, "cube")
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detections_fit_the_table() {
        let t = SimulatedTable::new(10);
        let d = t.detections();
        assert_eq!(d.len(), 10);
        assert!(d.iter().all(|d| d.bbox.x2 <= 0.91));
        t.add(-20);
        assert!(t.detections().is_empty());
    }
}
