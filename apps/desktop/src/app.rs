//! The window: setup panel, camera view with overlay, tutor HUD and number pad.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, Key, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use un_app::{
    build_session, list_microphones, pick_microphone, ModelPaths, PerceptionConfig, Session, SessionOptions, SimulatedTable,
    VoiceChoice,
};
use un_core::strategy::{SessionConfig, StrategyRegistry};
use un_core::{Box2, EstimatorConfig, Language};
use un_engine::StatusKind;
use un_vision::RgbFrame;

use crate::overlay::{self, Mapper};
use crate::settings::{self, Saved};
use crate::worker::{SourceSpec, Worker};

/// What a product wants preselected when the window opens, whatever was chosen last time.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    /// A detector name in the models directory (`toys`); ignored when it is not installed.
    pub detector: Option<String>,
    /// A strategy id (`lesson-01`); ignored when it is not registered.
    pub strategy: Option<String>,
}

/// Opens the window with the given strategies. Returns when the window closes.
pub fn run(registry: StrategyRegistry) -> eframe::Result<()> {
    run_with(registry, Defaults::default())
}

/// Like [`run`], with the product's own preselections.
pub fn run_with(registry: StrategyRegistry, defaults: Defaults) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_title("Un"),
        ..Default::default()
    };
    eframe::run_native("Un", options, Box::new(|cc| Ok(Box::new(UnApp::new(cc, registry, defaults)))))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Camera,
    Simulated,
}

struct Setup {
    source: SourceKind,
    camera: u32,
    cameras: Vec<String>,
    mirror: bool,
    detector: String,
    strategy: String,
    language: Language,
    child_name: String,
    total: u32,
    tasks: u32,
    voice: VoiceChoice,
    mic: bool,
    /// A microphone name, or `None` for the system default.
    mic_device: Option<String>,
    mics: Vec<String>,
    chime: bool,
    sim_count: u32,
    /// What the lessons remember between sessions.
    extra: HashMap<String, String>,
}

struct Live {
    worker: Worker,
    sim: Option<Arc<SimulatedTable>>,
    session: Option<Session>,
    texture: Option<TextureHandle>,
    texture_serial: u64,
    frame_size: (u32, u32),
    drag_start: Option<(f32, f32)>,
    drag: Option<Box2>,
    pad_buffer: String,
    pad_deadline: Option<Instant>,
    /// The finished session's `remember` map has been saved.
    remembered: bool,
}

pub struct UnApp {
    registry: StrategyRegistry,
    paths: ModelPaths,
    setup: Setup,
    live: Option<Live>,
    error: Option<String>,
}

impl UnApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, registry: StrategyRegistry, defaults: Defaults) -> Self {
        let cameras: Vec<String> = un_media::list_cameras().map(|c| c.into_iter().map(|c| c.name).collect()).unwrap_or_default();
        let first = registry.infos().first().map(|i| i.id.clone()).unwrap_or_else(|| "demo".into());
        let paths = ModelPaths::discover();
        let saved = settings::load();
        let detectors = paths.detectors();
        // The product's preselection wins at start; otherwise last time's choice; otherwise a guess.
        let detector = match (defaults.detector, saved.detector.clone()) {
            (Some(name), _) if detectors.contains(&name) => name,
            (_, Some(name)) if detectors.contains(&name) => name,
            _ => paths.default_detector(),
        };
        let strategy = match (defaults.strategy, saved.strategy.clone()) {
            (Some(id), _) if registry.get(&id).is_some() => id,
            (_, Some(id)) if registry.get(&id).is_some() => id,
            _ => first,
        };
        let mics = list_microphones();
        // A remembered microphone that is plugged in wins; otherwise the best one present.
        let mic_device = match saved.mic_device.clone() {
            Some(name) if mics.contains(&name) => Some(name),
            _ => pick_microphone(&mics),
        };
        Self {
            paths,
            registry,
            setup: Setup {
                source: SourceKind::Camera,
                camera: saved.camera.filter(|c| (*c as usize) < cameras.len()).unwrap_or(0),
                cameras,
                mirror: saved.mirror.unwrap_or(false),
                detector,
                strategy,
                language: saved.language.unwrap_or(Language::English),
                child_name: saved.child_name.unwrap_or_default(),
                total: saved.total.unwrap_or(10),
                tasks: saved.tasks.unwrap_or(10),
                voice: saved.voice.as_deref().and_then(VoiceChoice::parse).unwrap_or(VoiceChoice::Auto),
                mic: saved.mic.unwrap_or(true),
                mic_device,
                mics,
                chime: saved.chime.unwrap_or(true),
                sim_count: 3,
                extra: saved.extra,
            },
            live: None,
            error: None,
        }
    }

    fn remember(&self) {
        let s = &self.setup;
        settings::save(&Saved {
            camera: Some(s.camera),
            mirror: Some(s.mirror),
            detector: Some(s.detector.clone()),
            strategy: Some(s.strategy.clone()),
            language: Some(s.language),
            child_name: Some(s.child_name.clone()),
            total: Some(s.total),
            tasks: Some(s.tasks),
            voice: Some(
                match s.voice {
                    VoiceChoice::Auto => "auto",
                    VoiceChoice::Piper => "piper",
                    VoiceChoice::Console => "console",
                    VoiceChoice::Off => "off",
                }
                .to_string(),
            ),
            mic: Some(s.mic),
            mic_device: s.mic_device.clone(),
            chime: Some(s.chime),
            extra: s.extra.clone(),
        });
    }

    fn start_watching(&mut self) {
        self.remember();
        let s = &self.setup;
        let (source, sim) = match s.source {
            SourceKind::Camera => (SourceSpec::Camera { index: s.camera, width: 1280, height: 720, mirror: s.mirror }, None),
            SourceKind::Simulated => {
                let table = Arc::new(SimulatedTable::new(s.sim_count));
                (SourceSpec::Simulated(Arc::clone(&table)), Some(table))
            }
        };
        let config = PerceptionConfig {
            detector: s.detector.clone(),
            hands: true,
            mirrored: s.mirror,
            estimator: EstimatorConfig::default(),
            ..Default::default()
        };
        let worker = Worker::start(self.paths.clone(), source, config);
        self.live = Some(Live {
            worker,
            sim,
            session: None,
            texture: None,
            texture_serial: 0,
            frame_size: (1280, 720),
            drag_start: None,
            drag: None,
            pad_buffer: String::new(),
            pad_deadline: None,
            remembered: false,
        });
        self.error = None;
    }

    fn start_lesson(&mut self) {
        let Some(live) = &mut self.live else { return };
        let s = &self.setup;
        let labels = live.worker.latest.lock().labels.clone();
        // One kind of toy on the detector: its name ("машинки"); several: "игрушки".
        let noun = if live.sim.is_none() && labels.len() == 1 { labels[0].clone() } else { "toy".to_string() };
        let config = SessionConfig {
            language: s.language,
            child_name: (!s.child_name.trim().is_empty()).then(|| s.child_name.trim().to_string()),
            total_objects: s.total,
            max_tasks: s.tasks,
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1),
            noun,
            extra: s.extra.clone(),
            ..Default::default()
        };
        let options = SessionOptions {
            strategy: s.strategy.clone(),
            config,
            mic: if s.mic { un_app::MicChoice::Auto } else { un_app::MicChoice::Off },
            mic_device: s.mic_device.clone(),
            chime: s.chime,
            voice: s.voice,
            ..Default::default()
        };
        match build_session(&self.paths, options, lesson_logger()) {
            Ok(session) => {
                session.engine.start();
                live.session = Some(session);
                live.remembered = false;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Keeps what a finished lesson asked to remember (the child's name, where it stopped).
    fn remember_session(&mut self) {
        let Some(live) = &mut self.live else { return };
        if live.remembered {
            return;
        }
        let Some(summary) = live.session.as_ref().and_then(|s| s.engine.snapshot().summary) else { return };
        live.remembered = true;
        summary.apply_remembered(&mut self.setup.extra);
        if let Some(name) = self.setup.extra.get("child_name") {
            self.setup.child_name = name.clone();
        }
        self.remember();
    }

    fn stop_lesson(&mut self) {
        if let Some(live) = &mut self.live {
            if let Some(session) = live.session.take() {
                session.engine.close();
            }
        }
    }

    fn setup_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Un");
        ui.label("Tilt the lid forward so the camera looks down at the table, then drag a rectangle over the table area.");
        ui.separator();
        let s = &mut self.setup;
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.source, SourceKind::Camera, "Camera");
            ui.radio_value(&mut s.source, SourceKind::Simulated, "Simulated table");
        });
        if s.source == SourceKind::Camera {
            egui::ComboBox::from_label("Camera")
                .selected_text(s.cameras.get(s.camera as usize).cloned().unwrap_or_else(|| format!("camera {}", s.camera)))
                .show_ui(ui, |ui| {
                    for (i, name) in s.cameras.iter().enumerate() {
                        ui.selectable_value(&mut s.camera, i as u32, name);
                    }
                });
            ui.checkbox(&mut s.mirror, "Mirror (picture matches the child's side)");
            let detectors = self.paths.detectors();
            if detectors.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Detector");
                    ui.text_edit_singleline(&mut s.detector);
                });
            } else {
                egui::ComboBox::from_label("Detector").selected_text(s.detector.clone()).show_ui(ui, |ui| {
                    for name in &detectors {
                        ui.selectable_value(&mut s.detector, name.clone(), name);
                    }
                });
            }
        } else {
            ui.add(egui::Slider::new(&mut s.sim_count, 0..=10).text("objects on the table"));
        }
        ui.separator();
        egui::ComboBox::from_label("Lesson")
            .selected_text(self.registry.get(&s.strategy).map(|f| f.info().title).unwrap_or_else(|| s.strategy.clone()))
            .show_ui(ui, |ui| {
                for info in self.registry.infos() {
                    ui.selectable_value(&mut s.strategy, info.id.clone(), info.title);
                }
            });
        ui.horizontal(|ui| {
            ui.radio_value(&mut s.language, Language::English, "English");
            ui.radio_value(&mut s.language, Language::Russian, "Русский");
        });
        ui.horizontal(|ui| {
            ui.label("Child's name");
            ui.text_edit_singleline(&mut s.child_name);
        });
        ui.add(egui::Slider::new(&mut s.total, 1..=20).text("objects the family owns"));
        ui.add(egui::Slider::new(&mut s.tasks, 1..=30).text("tasks per session"));
        ui.horizontal(|ui| {
            ui.label("Voice");
            ui.radio_value(&mut s.voice, VoiceChoice::Auto, "auto");
            ui.radio_value(&mut s.voice, VoiceChoice::Console, "text only");
            ui.radio_value(&mut s.voice, VoiceChoice::Off, "off");
        });
        ui.checkbox(&mut s.mic, "Listen on the microphone");
        if s.mic {
            ui.horizontal(|ui| {
                let shown = s.mic_device.clone().unwrap_or_else(|| "System default".to_string());
                egui::ComboBox::from_id_salt("mic").selected_text(shown).show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.mic_device, None, "System default");
                    for name in &s.mics {
                        ui.selectable_value(&mut s.mic_device, Some(name.clone()), name);
                    }
                });
                if ui.button("⟳").on_hover_text("Look for microphones again").clicked() {
                    s.mics = list_microphones();
                }
            });
            ui.checkbox(&mut s.chime, "Beep when the tutor starts listening");
        }
        ui.separator();
        if ui.add(egui::Button::new("Start").min_size(Vec2::new(120.0, 36.0))).clicked() {
            self.start_watching();
        }
        if let Some(error) = &self.error {
            ui.colored_label(overlay::RED, error);
        }
    }

    fn live_view(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let paths_models = self.paths.root.display().to_string();
        let Some(live) = &mut self.live else { return };
        let (frame, state, tracked, hands, fingers, fps, error, serial, hands_enabled) = {
            let l = live.worker.latest.lock();
            (
                l.frame.clone(),
                l.state.clone(),
                l.tracked.clone(),
                l.hands.clone(),
                l.fingers.clone(),
                l.fps,
                l.error.clone(),
                l.frame_serial,
                l.hands_enabled,
            )
        };
        if let Some(error) = error {
            self.error = Some(format!("{error} (models: {paths_models})"));
            self.stop_lesson();
            self.live = None;
            return;
        }
        if let Some(frame) = &frame {
            if serial != live.texture_serial {
                live.texture = Some(upload(ctx, live.texture.take(), frame));
                live.texture_serial = serial;
                live.frame_size = (frame.width, frame.height);
            }
        }
        let snapshot = live.session.as_ref().map(|s| s.engine.snapshot());
        if let (Some(session), Some(state)) = (&live.session, &state) {
            session.engine.observe(state);
        }
        if snapshot.as_ref().is_some_and(|s| s.summary.is_some()) {
            self.remember_session();
        }

        // Top bar.
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.stop_lesson();
                self.live = None;
                return;
            }
            let live = self.live.as_mut().unwrap();
            match &live.session {
                None => {
                    if ui.button("Start lesson").clicked() {
                        self.start_lesson();
                        return;
                    }
                }
                Some(_) => {
                    if ui.button("End lesson").clicked() {
                        if let Some(s) = &live.session {
                            s.engine.stop();
                        }
                    }
                }
            }
            if let Some(sim) = &live.sim {
                if ui.button("−").clicked() {
                    sim.add(-1);
                }
                ui.label(format!("{} on the table", sim.count()));
                if ui.button("+").clicked() {
                    sim.add(1);
                }
            }
            if let Some(s) = &live.session {
                // As on the phone: the line again, or on to the next task.
                if ui.button("Repeat").on_hover_text("Space").clicked() {
                    s.engine.repeat();
                }
                if ui.button("Skip").on_hover_text("N").clicked() {
                    s.engine.skip();
                }
            }
            if ui.button("Reset area (R)").clicked() {
                live.worker.set_roi(None);
            }
            if let Some(state) = &state {
                let text = match state.cube_count {
                    Some(n) if state.is_stable => format!("{n} (stable)"),
                    Some(n) => format!("{n}"),
                    None => "…".into(),
                };
                ui.colored_label(
                    if state.is_stable { overlay::GREEN } else { Color32::LIGHT_GRAY },
                    format!("count: {text}   seen now: {}   groups: {}", state.raw_count, state.groups),
                );
                if let Some(f) = state.finger_count {
                    ui.label(format!("fingers: {f}"));
                }
            }
            ui.label(format!("{fps:.0} fps{}", if hands_enabled { "" } else { "  (hand models missing)" }));
            // The tutor's language, as on the phone's HUD; a running lesson starts over in the
            // new one, since a session's language is fixed when it is created.
            let before = self.setup.language;
            ui.radio_value(&mut self.setup.language, Language::English, "EN");
            ui.radio_value(&mut self.setup.language, Language::Russian, "RU");
            if self.setup.language != before {
                self.remember();
                if self.live.as_ref().is_some_and(|l| l.session.is_some()) {
                    self.stop_lesson();
                    self.start_lesson();
                }
            }
        });
        let Some(live) = &mut self.live else { return };

        // Picture.
        let available = ui.available_size();
        let (fw, fh) = live.frame_size;
        let aspect = fw as f32 / fh as f32;
        let pad_height = if snapshot.as_ref().is_some_and(|s| s.show_number_pad) { 70.0 } else { 0.0 };
        let mut size = Vec2::new(available.x, available.x / aspect);
        if size.y > available.y - pad_height {
            size = Vec2::new((available.y - pad_height) * aspect, available.y - pad_height);
        }
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        if let Some(texture) = &live.texture {
            painter.image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        } else {
            painter.rect_filled(rect, 0.0, Color32::from_gray(30));
        }
        let map = Mapper { rect };

        // Table area by dragging.
        if live.sim.is_none() {
            if response.drag_started() {
                if let Some(pos) = response.interact_pointer_pos() {
                    live.drag_start = Some(map.to_norm(pos));
                }
            }
            if response.dragged() {
                if let (Some(start), Some(pos)) = (live.drag_start, response.interact_pointer_pos()) {
                    let (x, y) = map.to_norm(pos);
                    live.drag = Some(Box2::from_corners(start.0, start.1, x, y));
                }
            }
            if response.drag_stopped() {
                if let Some(d) = live.drag.take() {
                    if d.width() > 0.03 && d.height() > 0.03 {
                        live.worker.set_roi(Some(d));
                    }
                }
                live.drag_start = None;
            }
        }
        overlay::draw_roi(&painter, &map, live.worker.roi(), live.drag);
        if let Some(state) = &state {
            overlay::draw_detections(&painter, &map, state, &tracked, snapshot.as_ref().and_then(|s| s.highlight_count));
        }
        overlay::draw_fingers(&painter, &map, &fingers);
        overlay::draw_hands(&painter, &map, &hands);

        // Tutor HUD.
        if let Some(snap) = &snapshot {
            let status = match (snap.status.kind, snap.status.listen_mode) {
                (StatusKind::Listening, Some(mode)) => format!("listening ({mode:?})"),
                (kind, _) => format!("{kind:?}").to_lowercase(),
            };
            let bubble_rect = Rect::from_min_size(rect.min + Vec2::new(16.0, 16.0), Vec2::new(rect.width() - 32.0, 64.0));
            overlay::bubble(&painter, bubble_rect, &snap.last_spoken, 24.0, Color32::from_black_alpha(170));
            painter.text(
                rect.right_top() + Vec2::new(-16.0, 90.0),
                egui::Align2::RIGHT_TOP,
                format!(
                    "L{}/{}  {}  {} ✓ / {}",
                    snap.strategy.level + 1,
                    snap.strategy.level_count,
                    status,
                    snap.strategy.correct,
                    snap.strategy.tasks_completed
                ),
                egui::FontId::proportional(18.0),
                Color32::WHITE,
            );
            if snap.speech_available {
                let mic = if snap.mic_name.is_empty() { "system default".to_string() } else { snap.mic_name.clone() };
                painter.text(
                    rect.left_bottom() + Vec2::new(16.0, -34.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("🎤 {mic}"),
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );
                if !snap.heard_text.is_empty() {
                    painter.text(
                        rect.left_bottom() + Vec2::new(16.0, -52.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("heard: {}", snap.heard_text.to_lowercase()),
                        egui::FontId::proportional(14.0),
                        Color32::from_gray(220),
                    );
                }
                let listening = snap.status.kind == StatusKind::Listening;
                let bar = Rect::from_min_size(
                    rect.left_bottom() + Vec2::new(16.0, -24.0),
                    Vec2::new(200.0 * snap.mic_level.clamp(0.0, 1.0), 10.0),
                );
                painter.rect_filled(bar, 4.0, if listening { overlay::GREEN } else { Color32::from_gray(160) });
            }
            if let Some(ago) = snap.celebrated_seconds_ago {
                if ago < 1.5 {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "★",
                        egui::FontId::proportional(120.0 * (1.0 - ago as f32 / 1.5) + 40.0),
                        overlay::AMBER,
                    );
                }
            }
            if let Some(summary) = &snap.summary {
                let card = Rect::from_center_size(rect.center(), Vec2::new(420.0, 140.0));
                let text = if summary.paused {
                    format!("Paused\n{} of {} right so far", summary.correct, summary.tasks_completed)
                } else {
                    format!("{} of {} right\nlevel {}", summary.correct, summary.tasks_completed, summary.final_level + 1)
                };
                overlay::bubble(&painter, card, &text, 28.0, Color32::from_rgba_unmultiplied(30, 30, 60, 230));
            }
            if let Some(warning) = &snap.warning {
                painter.text(
                    rect.left_bottom() + Vec2::new(16.0, -40.0),
                    egui::Align2::LEFT_BOTTOM,
                    warning,
                    egui::FontId::proportional(14.0),
                    overlay::AMBER,
                );
            }
        }

        // Number pad and keys.
        if let Some(snap) = &snapshot {
            if snap.show_number_pad && snap.status.kind != StatusKind::Finished {
                ui.horizontal_wrapped(|ui| {
                    for n in 0..=self.setup.total.min(20) {
                        if ui
                            .add(egui::Button::new(egui::RichText::new(n.to_string()).size(22.0)).min_size(Vec2::new(48.0, 48.0)))
                            .clicked()
                        {
                            if let Some(s) = &live.session {
                                s.engine.tap(n);
                            }
                        }
                    }
                });
            }
        }
        let typed: Vec<char> = ctx.input(|i| {
            i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { t.chars().next() } else { None }).collect()
        });
        for ch in typed {
            match ch {
                '0'..='9' => {
                    live.pad_buffer.push(ch);
                    let value: u32 = live.pad_buffer.parse().unwrap_or(0);
                    if value > self.setup.total {
                        live.pad_buffer = ch.to_string();
                    }
                    let value: u32 = live.pad_buffer.parse().unwrap_or(0);
                    if value * 10 > self.setup.total {
                        tap(live, value);
                    } else {
                        live.pad_deadline = Some(Instant::now() + std::time::Duration::from_millis(800));
                    }
                }
                ' ' => {
                    if let Some(s) = &live.session {
                        s.engine.repeat();
                    }
                }
                'n' => {
                    if let Some(s) = &live.session {
                        s.engine.skip();
                    }
                }
                'r' => live.worker.set_roi(None),
                '+' | '=' => {
                    if let Some(sim) = &live.sim {
                        sim.add(1);
                    }
                }
                '-' => {
                    if let Some(sim) = &live.sim {
                        sim.add(-1);
                    }
                }
                _ => {}
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if let Ok(value) = live.pad_buffer.parse::<u32>() {
                tap(live, value);
            }
        }
        if live.pad_deadline.is_some_and(|d| Instant::now() >= d) {
            if let Ok(value) = live.pad_buffer.parse::<u32>() {
                tap(live, value);
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// Appends the engine's transcript to `lesson.log` in the config directory (see `settings`).
fn lesson_logger() -> Option<un_engine::Logger> {
    use std::io::Write;
    let path = settings::log_path()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::OpenOptions::new().append(true).create(true).open(&path).ok()?;
    let file = std::sync::Mutex::new(file);
    let started = Instant::now();
    let _ = writeln!(file.lock().unwrap(), "--- session {}", chrono_free_timestamp());
    Some(Arc::new(move |line: &str| {
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(f, "{:8.2}  {line}", started.elapsed().as_secs_f64());
        }
    }))
}

/// Seconds since the Unix epoch, good enough to tell sessions apart without a date crate.
fn chrono_free_timestamp() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn tap(live: &mut Live, value: u32) {
    live.pad_buffer.clear();
    live.pad_deadline = None;
    if let Some(s) = &live.session {
        s.engine.tap(value);
    }
}

fn upload(ctx: &egui::Context, existing: Option<TextureHandle>, frame: &RgbFrame) -> TextureHandle {
    let image = egui::ColorImage::from_rgb([frame.width as usize, frame.height as usize], &frame.data);
    match existing {
        Some(mut t) => {
            t.set(image, TextureOptions::LINEAR);
            t
        }
        None => ctx.load_texture("camera", image, TextureOptions::LINEAR),
    }
}

impl eframe::App for UnApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            if self.live.is_some() {
                self.live_view(&ctx, ui);
            } else {
                self.setup_panel(ui);
            }
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_lesson();
        self.live = None;
    }
}
