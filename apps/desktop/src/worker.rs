//! The perception thread: camera (or simulated table) -> detector/hands/estimator -> shared state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use un_app::{ModelPaths, Perception, PerceptionConfig, SimulatedTable};
use un_core::{
    Box2, Detection, DetectionTracker, HandObservation, TableState, TableStateEstimator, TrackedDetection, TrackerConfig,
};
use un_media::{CameraSource, FrameSource};
use un_vision::RgbFrame;

pub struct Latest {
    pub frame: Option<Arc<RgbFrame>>,
    pub state: Option<TableState>,
    pub tracked: Vec<TrackedDetection>,
    pub hands: Vec<HandObservation>,
    /// Fingers the detector saw (shown, not counted).
    pub fingers: Vec<Detection>,
    pub fps: f32,
    pub error: Option<String>,
    pub labels: Vec<String>,
    pub hands_enabled: bool,
    pub frame_serial: u64,
}

pub enum SourceSpec {
    Camera { index: u32, width: u32, height: u32, mirror: bool },
    Simulated(Arc<SimulatedTable>),
}

pub struct Worker {
    pub latest: Arc<Mutex<Latest>>,
    stop: Arc<AtomicBool>,
    roi: Arc<Mutex<Option<Box2>>>,
    roi_changed: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub fn start(paths: ModelPaths, source: SourceSpec, config: PerceptionConfig) -> Self {
        let latest = Arc::new(Mutex::new(Latest {
            frame: None,
            state: None,
            tracked: vec![],
            hands: vec![],
            fingers: vec![],
            fps: 0.0,
            error: None,
            labels: vec![],
            hands_enabled: false,
            frame_serial: 0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let roi = Arc::new(Mutex::new(config.estimator.roi));
        let roi_changed = Arc::new(AtomicBool::new(false));
        let handle = {
            let (latest, stop, roi, roi_changed) =
                (Arc::clone(&latest), Arc::clone(&stop), Arc::clone(&roi), Arc::clone(&roi_changed));
            thread::Builder::new()
                .name("un-perception".into())
                .spawn(move || run_loop(paths, source, config, latest, stop, roi, roi_changed))
                .expect("spawn")
        };
        Self { latest, stop, roi, roi_changed, handle: Some(handle) }
    }

    pub fn set_roi(&self, roi: Option<Box2>) {
        *self.roi.lock() = roi;
        self.roi_changed.store(true, Ordering::SeqCst);
    }

    pub fn roi(&self) -> Option<Box2> {
        *self.roi.lock()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_loop(
    paths: ModelPaths,
    source: SourceSpec,
    config: PerceptionConfig,
    latest: Arc<Mutex<Latest>>,
    stop: Arc<AtomicBool>,
    roi: Arc<Mutex<Option<Box2>>>,
    roi_changed: Arc<AtomicBool>,
) {
    let mut fps = 0.0f32;
    let mut last_tick = Instant::now();
    let started = Instant::now();
    match source {
        SourceSpec::Simulated(table) => {
            let mut estimator = TableStateEstimator::new(config.estimator.clone());
            let mut tracker = DetectionTracker::new(TrackerConfig::default());
            latest.lock().labels = vec!["cube".into()];
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(66));
                let frame = draw_table(&table);
                let out = Perception::process_detections(
                    &mut estimator,
                    &mut tracker,
                    &table.detections(),
                    &[],
                    started.elapsed().as_secs_f64(),
                );
                fps = tick(&mut last_tick, fps);
                let mut l = latest.lock();
                l.frame = Some(Arc::new(frame));
                l.state = Some(out.state);
                l.tracked = out.tracked;
                l.hands = vec![];
                l.fingers = out.fingers;
                l.fps = fps;
                l.frame_serial += 1;
            }
        }
        SourceSpec::Camera { index, width, height, mirror } => {
            let mut perception = match Perception::load(&paths, &config) {
                Ok(p) => p,
                Err(error) => {
                    latest.lock().error = Some(error.to_string());
                    return;
                }
            };
            {
                let mut l = latest.lock();
                l.labels = perception.labels.clone();
                l.hands_enabled = perception.hands.is_some();
            }
            let mut camera = match CameraSource::open(index, width, height) {
                Ok(c) => c,
                Err(error) => {
                    latest.lock().error = Some(error.to_string());
                    return;
                }
            };
            while !stop.load(Ordering::SeqCst) {
                if roi_changed.swap(false, Ordering::SeqCst) {
                    perception.estimator.set_roi(*roi.lock());
                    perception.tracker.reset();
                }
                let (frame, t) = match camera.next_frame() {
                    Ok(Some(f)) => f,
                    Ok(None) => break,
                    Err(error) => {
                        latest.lock().error = Some(error.to_string());
                        break;
                    }
                };
                let frame = if mirror { frame.flipped() } else { frame };
                match perception.process(&frame, t) {
                    Ok(out) => {
                        fps = tick(&mut last_tick, fps);
                        let mut l = latest.lock();
                        l.frame = Some(Arc::new(frame));
                        l.state = Some(out.state);
                        l.tracked = out.tracked;
                        l.hands = out.hands;
                        l.fingers = out.fingers;
                        l.fps = fps;
                        l.frame_serial += 1;
                    }
                    Err(error) => {
                        latest.lock().error = Some(error.to_string());
                        break;
                    }
                }
            }
        }
    }
}

fn tick(last: &mut Instant, fps: f32) -> f32 {
    let now = Instant::now();
    let instant = 1.0 / (now - *last).as_secs_f32().max(1e-6);
    *last = now;
    if fps == 0.0 {
        instant
    } else {
        0.9 * fps + 0.1 * instant
    }
}

/// A warm wooden table with green blocks: the simulated stage's picture.
fn draw_table(table: &SimulatedTable) -> RgbFrame {
    let (w, h) = (table.width, table.height);
    let mut frame = RgbFrame::filled(w, h, [140, 107, 77]);
    for d in table.detections() {
        let (x1, y1) = ((d.bbox.x1 * w as f32) as u32, (d.bbox.y1 * h as f32) as u32);
        let (x2, y2) = ((d.bbox.x2 * w as f32) as u32, (d.bbox.y2 * h as f32) as u32);
        for y in y1..y2.min(h) {
            for x in x1..x2.min(w) {
                let edge = x < x1 + 3 || x + 3 >= x2 || y < y1 + 3 || y + 3 >= y2;
                let color = if edge { [40, 120, 40] } else { [90, 190, 90] };
                let i = ((y * w + x) * 3) as usize;
                frame.data[i..i + 3].copy_from_slice(&color);
            }
        }
    }
    frame
}
