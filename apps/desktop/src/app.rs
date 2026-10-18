//! The window, as the "Un Desktop Board" designs it: the children, a child's trail, the lesson
//! over the webcam picture and its end, with a Settings panel for the grown-up. The iOS app is
//! the source of truth; this is its port to a desktop window (see `un.rs`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Frame, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2};
use un_app::{
    build_session, list_microphones, pick_microphone, ModelPaths, PerceptionConfig, Profile, Profiles, Session, SessionOptions,
    SimulatedTable, VoiceChoice,
};
use un_core::strategy::{SessionConfig, StrategyRegistry};
use un_core::{Box2, EstimatorConfig, Language};
use un_engine::{EngineSnapshot, StatusKind};
use un_vision::RgbFrame;

use crate::overlay::{self, Mapper};
use crate::settings::{self, Saved};
use crate::un::{self, bold, icon, regular, rounded, semibold, t, tf, Icon, LessonMeta, Theme};
use crate::worker::{SourceSpec, Worker};

/// What a product wants preselected when the window opens, whatever was chosen last time.
#[derive(Debug, Clone, Default)]
pub struct Defaults {
    /// A detector name in the models directory (`toys`); ignored when it is not installed.
    pub detector: Option<String>,
    /// A strategy id (`unmath-count-to-five`); ignored when it is not registered.
    pub strategy: Option<String>,
}

/// Opens the window with the given strategies. Returns when the window closes.
pub fn run(registry: StrategyRegistry) -> eframe::Result<()> {
    run_with(registry, Defaults::default())
}

/// Like [`run`], with the product's own preselections.
pub fn run_with(registry: StrategyRegistry, defaults: Defaults) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([1024.0, 700.0])
            .with_title("Un"),
        ..Default::default()
    };
    eframe::run_native(
        "Un",
        options,
        Box::new(|cc| {
            un::install_fonts(&cc.egui_ctx);
            Ok(Box::new(UnApp::new(cc, registry, defaults)))
        }),
    )
}

/// Devices and choices for the grown-up (the Settings panel), kept between runs.
struct Setup {
    camera: u32,
    cameras: Vec<String>,
    mirror: bool,
    detector: String,
    language: Language,
    total: u32,
    tasks: u32,
    voice: VoiceChoice,
    mic: bool,
    mic_device: Option<String>,
    mics: Vec<String>,
    chime: bool,
    simulated: bool,
    sim_count: u32,
    reduce_motion: bool,
    /// The chosen child (their trail is shown when the window opens).
    child: Option<String>,
    /// What lessons remembered without a child (the version before profiles).
    extra: HashMap<String, String>,
}

#[derive(Clone, PartialEq)]
enum Screen {
    Children,
    Trail(String),
    Lesson,
}

/// The lesson in progress (or paused, its session closed and its place saved).
struct LessonRun {
    child: Option<String>,
    lesson: String,
    session: Option<Session>,
    remembered: bool,
    /// A pause was asked for: the goodbye is being said, then the session closes.
    leaving: Option<Instant>,
    /// Paused: the picture dimmed, Continue or Back to adventures.
    paused: bool,
    ended_at: Option<Instant>,
    declined: bool,
    pad_buffer: String,
    pad_deadline: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq)]
enum EndPhase {
    Celebrate,
    Ask,
    Bye,
}

#[derive(Clone, Copy, PartialEq)]
enum TutorState {
    Speaking,
    Listening,
    Watching,
    Celebrating,
    Muted,
}

/// Which handle of the table-area frame is being dragged in Settings.
#[derive(Clone, Copy, PartialEq)]
enum AreaDrag {
    Move((f32, f32), Box2),
    Corner(usize),
}

pub struct UnApp {
    /// The lesson ids and titles, in the library's order.
    lessons: Vec<(String, String)>,
    profiles: Profiles,
    paths: ModelPaths,
    setup: Setup,
    worker: Option<Worker>,
    sim: Option<Arc<SimulatedTable>>,
    texture: Option<TextureHandle>,
    texture_serial: u64,
    frame_size: (u32, u32),
    screen: Screen,
    run: Option<LessonRun>,
    snapshot: Option<EngineSnapshot>,
    settings_open: bool,
    adding: Option<String>,
    removing: Option<String>,
    menu: Option<(String, Pos2)>,
    area_drag: Option<AreaDrag>,
    error: Option<String>,
    /// `UN_SCREENSHOT=<file.png>`: frames drawn so far, then the window saves itself and closes
    /// (screenshots of every screen without a person at the keyboard; see the README).
    frames: u32,
}

impl UnApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, registry: StrategyRegistry, defaults: Defaults) -> Self {
        let cameras: Vec<String> = un_media::list_cameras().map(|c| c.into_iter().map(|c| c.name).collect()).unwrap_or_default();
        let paths = ModelPaths::discover();
        let saved = settings::load();
        let detectors = paths.detectors();
        let detector = match (defaults.detector, saved.detector.clone()) {
            (Some(name), _) if detectors.contains(&name) => name,
            (_, Some(name)) if detectors.contains(&name) => name,
            _ => paths.default_detector(),
        };
        let mut profiles = Profiles::open(settings::profiles_path());
        // The child of earlier versions (one name, one progress) becomes the first profile.
        let mut child = saved.child.clone().filter(|id| profiles.get(id).is_some());
        if profiles.list().is_empty() {
            if let Some(name) = saved.child_name.clone().filter(|n| !n.trim().is_empty()) {
                let id = profiles.add(&name).id;
                let summary =
                    un_core::strategy::SessionSummary { paused: true, remember: saved.extra.clone(), ..Default::default() };
                profiles.record(&id, "", &summary, &[]);
                child = Some(id);
            }
        }
        let mics = list_microphones();
        let mic_device = match saved.mic_device.clone() {
            Some(name) if mics.contains(&name) => Some(name),
            _ => pick_microphone(&mics),
        };
        let lessons = registry.infos().into_iter().map(|i| (i.id, i.title)).collect();
        let mut app = Self {
            lessons,
            profiles,
            paths,
            setup: Setup {
                camera: saved.camera.filter(|c| (*c as usize) < cameras.len()).unwrap_or(0),
                cameras,
                mirror: saved.mirror.unwrap_or(false),
                detector,
                language: saved.language.unwrap_or_else(system_language),
                total: saved.total.unwrap_or(10),
                tasks: saved.tasks.unwrap_or(10),
                voice: saved.voice.as_deref().and_then(VoiceChoice::parse).unwrap_or(VoiceChoice::Auto),
                mic: saved.mic.unwrap_or(true),
                mic_device,
                mics,
                chime: saved.chime.unwrap_or(true),
                simulated: saved.simulated.unwrap_or(false),
                sim_count: saved.sim_count.unwrap_or(3),
                reduce_motion: saved.reduce_motion.unwrap_or(false),
                child: child.clone(),
                extra: saved.extra,
            },
            worker: None,
            sim: None,
            texture: None,
            texture_serial: 0,
            frame_size: (1280, 720),
            screen: child.map(Screen::Trail).unwrap_or(Screen::Children),
            run: None,
            snapshot: None,
            settings_open: false,
            adding: None,
            removing: None,
            menu: None,
            area_drag: None,
            error: None,
            frames: 0,
        };
        // The camera and the detector start at once: by the time a lesson starts they count.
        app.start_camera();
        // `UN_START_LESSON=1`: straight into the chosen child's next lesson (screenshots).
        if std::env::var_os("UN_START_LESSON").is_some() {
            if let Some(child) = app.setup.child.clone() {
                if let Some(next) = app.next_for(&child) {
                    app.start_lesson(Some(child), next);
                }
            }
        }
        if std::env::var_os("UN_SETTINGS").is_some() {
            app.settings_open = true;
        }
        app
    }

    fn ru(&self) -> bool {
        un::is_ru(self.setup.language)
    }

    fn remember(&self) {
        let s = &self.setup;
        settings::save(&Saved {
            camera: Some(s.camera),
            mirror: Some(s.mirror),
            detector: Some(s.detector.clone()),
            strategy: self.run.as_ref().map(|r| r.lesson.clone()),
            language: Some(s.language),
            child_name: None,
            child: s.child.clone(),
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
            simulated: Some(s.simulated),
            sim_count: Some(s.sim_count),
            reduce_motion: Some(s.reduce_motion),
            extra: s.extra.clone(),
        });
    }

    // -------------------------------------------------------------- camera

    /// (Re)starts the picture: the camera with the detector, or the simulated table.
    fn start_camera(&mut self) {
        let roi = self.worker.as_ref().and_then(|w| w.roi());
        self.worker = None;
        self.texture = None;
        self.texture_serial = 0;
        let s = &self.setup;
        let (source, sim) = if s.simulated {
            let table = Arc::new(SimulatedTable::new(s.sim_count));
            (SourceSpec::Simulated(Arc::clone(&table)), Some(table))
        } else {
            (SourceSpec::Camera { index: s.camera, width: 1280, height: 720, mirror: s.mirror }, None)
        };
        let config = PerceptionConfig {
            detector: s.detector.clone(),
            estimator: EstimatorConfig { roi, ..Default::default() },
            ..Default::default()
        };
        self.worker = Some(Worker::start(self.paths.clone(), source, config));
        self.sim = sim;
        self.error = None;
    }

    fn preparing(&self) -> bool {
        self.worker.as_ref().is_some_and(|w| w.latest.lock().loading)
    }

    // -------------------------------------------------------------- lessons

    fn lesson_ids(&self) -> Vec<String> {
        self.lessons.iter().map(|(id, _)| id.clone()).collect()
    }

    fn next_for(&self, child: &str) -> Option<String> {
        self.profiles.next_lesson(child, &self.lesson_ids())
    }

    fn start_lesson(&mut self, child: Option<String>, lesson: String) {
        self.stop_session();
        let s = &self.setup;
        let labels = self.worker.as_ref().map(|w| w.latest.lock().labels.clone()).unwrap_or_default();
        let noun = if self.sim.is_none() && labels.len() == 1 { labels[0].clone() } else { "toy".to_string() };
        let mut config = SessionConfig {
            language: s.language,
            total_objects: s.total,
            max_tasks: s.tasks,
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1),
            noun,
            extra: s.extra.clone(),
            ..Default::default()
        };
        if let Some(id) = &child {
            config = self.profiles.config(id, SessionConfig { child_name: None, extra: Default::default(), ..config });
        }
        let ids = self.lesson_ids();
        let has_next = ids.iter().position(|i| *i == lesson).is_some_and(|i| i + 1 < ids.len());
        let options = SessionOptions {
            strategy: lesson.clone(),
            config,
            mic: if s.mic { un_app::MicChoice::Auto } else { un_app::MicChoice::Off },
            mic_device: s.mic_device.clone(),
            chime: s.chime,
            voice: s.voice,
            ..Default::default()
        };
        match build_session(&self.paths, options, lesson_logger()) {
            Ok(session) => {
                session.engine.offer_next_lesson(has_next);
                session.engine.start();
                self.run = Some(LessonRun {
                    child,
                    lesson,
                    session: Some(session),
                    remembered: false,
                    leaving: None,
                    paused: false,
                    ended_at: None,
                    declined: false,
                    pad_buffer: String::new(),
                    pad_deadline: None,
                });
                self.screen = Screen::Lesson;
                self.remember();
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn stop_session(&mut self) {
        if let Some(run) = &mut self.run {
            if let Some(session) = run.session.take() {
                session.engine.close();
            }
        }
        self.snapshot = None;
    }

    /// Pause (the button, P, "пауза" or opening Settings): the tutor says goodbye, the place is
    /// saved, the picture dims with Continue / Back to adventures.
    fn pause(&mut self) {
        if let Some(run) = &mut self.run {
            if run.paused || run.leaving.is_some() {
                return;
            }
            if let Some(session) = &run.session {
                session.engine.stop();
            }
            run.leaving = Some(Instant::now());
        }
    }

    fn resume(&mut self) {
        if let Some(run) = &self.run {
            if run.paused {
                let (child, lesson) = (run.child.clone(), run.lesson.clone());
                self.start_lesson(child, lesson);
            }
        }
    }

    fn leave_lesson(&mut self) {
        self.stop_session();
        let child = self.run.as_ref().and_then(|r| r.child.clone());
        self.run = None;
        self.screen = child.map(Screen::Trail).unwrap_or(Screen::Children);
    }

    fn answer_next(&mut self, yes: bool) {
        let Some(run) = &mut self.run else { return };
        if let Some(session) = &run.session {
            session.engine.answer_next_lesson(yes);
        }
        if !yes {
            run.declined = true;
            return;
        }
        let (child, lesson) = (run.child.clone(), run.lesson.clone());
        let next = match &child {
            Some(c) => self.next_for(c),
            None => {
                let ids = self.lesson_ids();
                ids.iter().position(|i| *i == lesson).and_then(|i| ids.get(i + 1).cloned())
            }
        };
        if let Some(next) = next {
            self.start_lesson(child, next);
        }
    }

    fn end_phase(&self) -> Option<EndPhase> {
        let run = self.run.as_ref()?;
        let s = self.snapshot.as_ref()?;
        let summary = s.summary.as_ref()?;
        if summary.paused || run.leaving.is_some() || run.paused || s.pause_requested {
            return None;
        }
        if run.declined || s.next_lesson_declined {
            return Some(EndPhase::Bye);
        }
        if run.ended_at.is_some_and(|t| t.elapsed() < Duration::from_millis(3500)) {
            return Some(EndPhase::Celebrate);
        }
        let ids = self.lesson_ids();
        let has_next = ids.iter().position(|i| *i == run.lesson).is_some_and(|i| i + 1 < ids.len());
        Some(if has_next { EndPhase::Ask } else { EndPhase::Bye })
    }

    fn tutor_state(&self) -> TutorState {
        let Some(s) = &self.snapshot else { return TutorState::Watching };
        if s.celebrated_seconds_ago.is_some_and(|c| c < 3.0) {
            return TutorState::Celebrating;
        }
        if s.show_number_pad {
            return TutorState::Muted;
        }
        match s.status.kind {
            StatusKind::Speaking => TutorState::Speaking,
            StatusKind::Listening => TutorState::Listening,
            _ => TutorState::Watching,
        }
    }

    // -------------------------------------------------------------- every frame

    fn tick(&mut self, ctx: &egui::Context) {
        // The picture.
        if let Some(worker) = &self.worker {
            let (frame, serial, error) = {
                let l = worker.latest.lock();
                (l.frame.clone(), l.frame_serial, l.error.clone())
            };
            if let Some(error) = error {
                self.error = Some(format!("{error} (models: {})", self.paths.root.display()));
            }
            if let Some(frame) = frame {
                if serial != self.texture_serial {
                    self.texture = Some(upload(ctx, self.texture.take(), &frame));
                    self.texture_serial = serial;
                    self.frame_size = (frame.width, frame.height);
                }
            }
        }
        // The lesson.
        let Some(run) = &mut self.run else { return };
        let Some(session) = &run.session else { return };
        if let Some(state) = self.worker.as_ref().and_then(|w| w.latest.lock().state.clone()) {
            session.engine.observe(&state);
        }
        let snapshot = session.engine.snapshot();
        if snapshot.summary.is_some() && run.ended_at.is_none() {
            run.ended_at = Some(Instant::now());
        }
        if !run.remembered {
            if let Some(summary) = &snapshot.summary {
                run.remembered = true;
                match &run.child {
                    Some(id) => {
                        let ids: Vec<String> = self.lessons.iter().map(|(i, _)| i.clone()).collect();
                        self.profiles.record(id, &run.lesson, summary, &ids);
                    }
                    None => summary.apply_remembered(&mut self.setup.extra),
                }
            }
        }
        // "пауза" said aloud: as the button.
        if snapshot.pause_requested && run.leaving.is_none() {
            run.leaving = Some(Instant::now());
        }
        if let Some(since) = run.leaving {
            let said = snapshot.status.kind == StatusKind::Finished && run.remembered;
            if said || since.elapsed() > Duration::from_secs(8) {
                if let Some(session) = run.session.take() {
                    session.engine.close();
                }
                run.leaving = None;
                run.paused = true;
                self.snapshot = Some(snapshot);
                return;
            }
        }
        let next_requested = snapshot.next_lesson_requested;
        self.snapshot = Some(snapshot);
        if next_requested {
            self.answer_next(true);
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.memory(|m| m.focused().is_some()) && (self.adding.is_some());
        if typing {
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if pressed(Key::Escape) {
            if self.menu.is_some() {
                self.menu = None;
            } else if self.settings_open {
                self.settings_open = false;
            } else if self.adding.is_some() || self.removing.is_some() {
                self.adding = None;
                self.removing = None;
            } else if self.screen == Screen::Lesson && self.run.as_ref().is_some_and(|r| r.paused) {
                self.leave_lesson();
            } else if let Screen::Trail(_) = self.screen {
                self.screen = Screen::Children;
            }
            return;
        }
        if pressed(Key::S) {
            self.toggle_settings();
        }
        if pressed(Key::L) {
            self.set_language(if self.ru() { Language::English } else { Language::Russian });
        }
        if self.screen != Screen::Lesson {
            return;
        }
        match self.end_phase() {
            Some(EndPhase::Ask) => {
                if pressed(Key::Y) {
                    self.answer_next(true);
                } else if pressed(Key::N) {
                    self.answer_next(false);
                }
                return;
            }
            Some(_) => return,
            None => {}
        }
        if pressed(Key::P) {
            if self.run.as_ref().is_some_and(|r| r.paused) {
                self.resume();
            } else {
                self.pause();
            }
            return;
        }
        let Some(run) = &mut self.run else { return };
        let Some(session) = &run.session else { return };
        if pressed(Key::Space) {
            session.engine.repeat();
        }
        if pressed(Key::N) {
            session.engine.skip();
        }
        // Digits answer on the number pad (two digits within 0.8 s make 10).
        let typed: Vec<char> = ctx.input(|i| {
            i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { t.chars().next() } else { None }).collect()
        });
        for ch in typed.into_iter().filter(char::is_ascii_digit) {
            run.pad_buffer.push(ch);
            let value: u32 = run.pad_buffer.parse().unwrap_or(0);
            if value * 10 > 10 || run.pad_buffer.len() > 1 {
                tap(run, value);
            } else {
                run.pad_deadline = Some(Instant::now() + Duration::from_millis(800));
            }
        }
        if run.pad_deadline.is_some_and(|d| Instant::now() >= d) {
            if let Ok(value) = run.pad_buffer.parse::<u32>() {
                tap(run, value);
            }
        }
    }

    fn toggle_settings(&mut self) {
        self.settings_open = !self.settings_open;
        // The lesson pauses while Settings is open.
        if self.settings_open && self.screen == Screen::Lesson && self.end_phase().is_none() {
            self.pause();
        }
    }

    fn set_language(&mut self, language: Language) {
        if self.setup.language == language {
            return;
        }
        self.setup.language = language;
        self.remember();
        // A session's language is fixed when it is created: a running lesson starts over in it.
        if let Some(run) = &self.run {
            if run.session.is_some() && self.end_phase().is_none() {
                let (child, lesson) = (run.child.clone(), run.lesson.clone());
                self.start_lesson(child, lesson);
            }
        }
    }

    // -------------------------------------------------------------- children

    fn children_screen(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui);
        let ru = self.ru();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, theme.surface);
        let column = full.width().min(1240.0 + 80.0) - 80.0;
        let left = full.center().x - column / 2.0;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(20.0);
            let row = ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(left, ui.cursor().top()), Vec2::new(column, 40.0))), |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Un").font(rounded(30.0)).color(theme.orange));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if un::circle_button(ui, Icon::Gear, 38.0, theme.surface2, t(ru, "settings.title")).clicked() {
                            self.toggle_settings();
                        }
                        ui.add_space(8.0);
                        let mut language = self.setup.language;
                        if un::language_switch(ui, &mut language) {
                            self.set_language(language);
                        }
                    });
                });
            });
            let _ = row;
            ui.add_space(14.0);
            let title_rect = ui.allocate_space(Vec2::new(full.width(), 44.0)).1;
            ui.painter().text(Pos2::new(left, title_rect.center().y), Align2::LEFT_CENTER, t(ru, "children.title"), bold(34.0), theme.text);
            ui.add_space(14.0);
            self.status_card(ui, left, column);
            let children: Vec<Profile> = self.profiles.list().to_vec();
            if children.is_empty() {
                self.empty_children(ui, full, &theme);
                return;
            }
            let columns = (((column + 16.0) / (340.0 + 16.0)).floor() as usize).clamp(1, 3);
            let card_w = (column - 16.0 * (columns as f32 - 1.0)) / columns as f32;
            let card_h = 194.0;
            let count = children.len() + 1;
            let rows = count.div_ceil(columns);
            let (grid, _) = ui.allocate_exact_size(Vec2::new(full.width(), rows as f32 * (card_h + 16.0)), Sense::hover());
            for i in 0..count {
                let r = Rect::from_min_size(
                    Pos2::new(left + (i % columns) as f32 * (card_w + 16.0), grid.top() + (i / columns) as f32 * (card_h + 16.0)),
                    Vec2::new(card_w, card_h),
                );
                if i < children.len() {
                    self.child_card(ui, r, &children[i], i, &theme);
                } else {
                    let response = ui.interact(r, Id::new("add-child"), Sense::click());
                    let fill = if response.hovered() { theme.blue.gamma_multiply(0.08) } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(r, 14.0, fill);
                    overlay::dashed_rounded(ui.painter(), r.shrink(1.0), 14.0, theme.text3);
                    ui.painter().circle_filled(r.center() - Vec2::new(0.0, 16.0), 24.0, theme.blue.gamma_multiply(0.14));
                    icon(ui.painter(), Icon::Plus, r.center() - Vec2::new(0.0, 16.0), 22.0, theme.blue);
                    ui.painter().text(r.center() + Vec2::new(0.0, 26.0), Align2::CENTER_CENTER, t(ru, "children.add"), semibold(16.0), theme.blue);
                    un::focus_ring(ui, &response, r, 14.0);
                    if response.clicked() {
                        self.adding = Some(String::new());
                    }
                }
            }
            ui.add_space(40.0);
        });
    }

    fn empty_children(&mut self, ui: &mut Ui, full: Rect, theme: &Theme) {
        let ru = self.ru();
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(full.width());
            let (r, _) = ui.allocate_exact_size(Vec2::new(64.0, 64.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 32.0, theme.orange.gamma_multiply(0.15));
            icon(ui.painter(), Icon::Plus, r.center(), 30.0, theme.orange);
            ui.add_space(16.0);
            ui.label(egui::RichText::new(t(ru, "children.emptyTitle")).font(rounded(30.0)).color(theme.text));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t(ru, "children.emptyBody")).font(regular(17.0)).color(theme.text2));
            ui.add_space(20.0);
            if un::primary_button(ui, t(ru, "children.add"), 320.0, None).clicked() {
                self.adding = Some(String::new());
            }
        });
    }

    fn child_card(&mut self, ui: &mut Ui, r: Rect, child: &Profile, index: usize, theme: &Theme) {
        let ru = self.ru();
        let painter = ui.painter().clone();
        let accent = theme.accent(index);
        un::card(&painter, r, theme, 14.0, false);
        let done = self.lessons.iter().filter(|(id, _)| child.completed.contains(id)).count();
        let total = self.lessons.len();
        let next = self.next_for(&child.id);
        let paused = next.as_ref().is_some_and(|n| child.paused.as_deref() == Some(n.as_str()));
        // Header: initial, name, progress; the whole header opens the trail.
        let head = Rect::from_min_size(r.min + Vec2::new(16.0, 16.0), Vec2::new(r.width() - 32.0 - 44.0, 44.0));
        let open = ui.interact(head, Id::new(("open", &child.id)), Sense::click());
        let avatar = Pos2::new(head.left() + 22.0, head.center().y);
        painter.circle_filled(avatar, 22.0, accent.gamma_multiply(0.16));
        let initial: String = child.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        painter.text(avatar, Align2::CENTER_CENTER, initial, rounded(22.0), accent);
        painter.text(Pos2::new(head.left() + 56.0, head.top() + 13.0), Align2::LEFT_CENTER, &child.name, semibold(22.0), theme.text);
        painter.text(
            Pos2::new(head.left() + 56.0, head.top() + 35.0),
            Align2::LEFT_CENTER,
            tf(ru, "children.progress", &[("n", &done.to_string()), ("total", &total.to_string())]),
            regular(15.0),
            theme.text2,
        );
        un::focus_ring(ui, &open, head, 10.0);
        if open.clicked() {
            self.setup.child = Some(child.id.clone());
            self.remember();
            self.screen = Screen::Trail(child.id.clone());
        }
        // "…": a menu with Open adventures / Remove….
        let more = Rect::from_center_size(Pos2::new(r.right() - 34.0, head.center().y), Vec2::splat(36.0));
        let more_response = ui.interact(more, Id::new(("more", &child.id)), Sense::click());
        painter.circle_filled(more.center(), 18.0, if more_response.hovered() { theme.surface3.gamma_multiply(1.2) } else { theme.surface3 });
        icon(&painter, Icon::Ellipsis, more.center(), 18.0, theme.text2);
        un::focus_ring(ui, &more_response, more, 18.0);
        if more_response.clicked() {
            self.menu = Some((child.id.clone(), more.left_bottom() + Vec2::new(-150.0, 6.0)));
        }
        // Ten segments.
        let seg_top = r.top() + 16.0 + 44.0 + 12.0;
        let n = self.lessons.len().max(1);
        let seg_w = (r.width() - 32.0 - 4.0 * (n as f32 - 1.0)) / n as f32;
        for (i, (id, _)) in self.lessons.iter().enumerate() {
            let color = if child.completed.contains(id) {
                theme.green
            } else if child.paused.as_deref() == Some(id.as_str()) {
                theme.orange
            } else {
                theme.surface3
            };
            let s = Rect::from_min_size(Pos2::new(r.left() + 16.0 + i as f32 * (seg_w + 4.0), seg_top), Vec2::new(seg_w, 6.0));
            painter.rect_filled(s, 3.0, color);
        }
        // Next lesson and Continue.
        if let Some(next) = next {
            let meta = LessonMeta::of(&next, &self.lessons, ru);
            let y = seg_top + 6.0 + 12.0 + 11.0;
            let dot = Pos2::new(r.left() + 16.0 + 11.0, y);
            painter.circle_filled(dot, 11.0, if paused { theme.orange } else { accent.gamma_multiply(0.16) });
            painter.text(dot, Align2::CENTER_CENTER, meta.number.to_string(), rounded(12.0), if paused { Color32::WHITE } else { accent });
            let label = if paused {
                tf(ru, "children.resume", &[("lesson", &meta.title)])
            } else {
                tf(ru, "children.next", &[("lesson", &meta.title)])
            };
            painter.text(Pos2::new(dot.x + 19.0, y), Align2::LEFT_CENTER, label, semibold(15.0), theme.text);
            let button = Rect::from_min_size(Pos2::new(r.left() + 16.0, r.bottom() - 16.0 - 50.0), Vec2::new(r.width() - 32.0, 50.0));
            let clicked = ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| un::primary_button(ui, t(ru, "children.cont"), button.width(), None)).inner.clicked();
            if clicked {
                self.setup.child = Some(child.id.clone());
                self.start_lesson(Some(child.id.clone()), next);
            }
        }
    }

    /// Preparing the camera / toy recognition failed: inline at the top of a list.
    fn status_card(&mut self, ui: &mut Ui, left: f32, width: f32) {
        let theme = Theme::of(ui);
        let ru = self.ru();
        let failed = self.error.is_some();
        if !self.preparing() && !failed {
            return;
        }
        let body = t(ru, if failed { "error.body" } else { "prep.body" });
        let h = 16.0 + 26.0 + 8.0 + un::text_height(ui, body, regular(15.0), width - 40.0) + 16.0 + if failed { 50.0 + 4.0 } else { 6.0 + 26.0 } + 16.0;
        let (space, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h + 16.0), Sense::hover());
        let r = Rect::from_min_size(Pos2::new(left, space.top()), Vec2::new(width, h));
        self.status_body(ui, r, &theme, false);
    }

    fn status_body(&mut self, ui: &mut Ui, r: Rect, theme: &Theme, over_camera: bool) {
        let ru = self.ru();
        let failed = self.error.is_some();
        let painter = ui.painter().clone();
        un::card(&painter, r, theme, if over_camera { 22.0 } else { 14.0 }, over_camera);
        let mut y = r.top() + 18.0;
        if failed {
            icon(&painter, Icon::Eye, Pos2::new(r.left() + 30.0, y + 12.0), 22.0, theme.red);
            painter.text(Pos2::new(r.left() + 50.0, y + 12.0), Align2::LEFT_CENTER, t(ru, "error.title"), bold(18.0), theme.text);
        } else {
            for (i, c) in [theme.orange, theme.blue, theme.green].into_iter().enumerate() {
                painter.circle_filled(Pos2::new(r.left() + 24.0 + i as f32 * 14.0, y + 2.0), 4.5, c);
            }
            painter.text(Pos2::new(r.left() + 20.0, y + 22.0), Align2::LEFT_CENTER, t(ru, "prep.title"), bold(20.0), theme.text);
        }
        y += 34.0;
        let body = t(ru, if failed { "error.body" } else { "prep.body" });
        y += un::text_block(&painter, Pos2::new(r.left() + 20.0, y), body, regular(15.0), theme.text2, r.width() - 40.0) + 12.0;
        if failed {
            let button = Rect::from_min_size(Pos2::new(r.left() + 20.0, y), Vec2::new((r.width() - 40.0).min(320.0), 50.0));
            let clicked = ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| un::primary_button(ui, t(ru, "error.retry"), button.width(), None)).inner.clicked();
            if clicked {
                self.start_camera();
            }
        } else {
            let time = ui.input(|i| i.time);
            un::shimmer(&painter, Rect::from_min_size(Pos2::new(r.left() + 20.0, y), Vec2::new(r.width() - 40.0, 6.0)), theme, time, self.setup.reduce_motion);
            painter.text(Pos2::new(r.left() + 20.0, y + 20.0), Align2::LEFT_CENTER, t(ru, "prep.step"), regular(12.5), theme.text2);
            ui.ctx().request_repaint();
        }
    }

    // -------------------------------------------------------------- trail

    fn trail_screen(&mut self, ui: &mut Ui, child_id: &str) {
        let theme = Theme::of(ui);
        let ru = self.ru();
        let Some(child) = self.profiles.get(child_id).cloned() else {
            self.screen = Screen::Children;
            return;
        };
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, theme.surface);
        let next = self.next_for(&child.id);
        let paused = next.as_ref().is_some_and(|n| child.paused.as_deref() == Some(n.as_str()));
        let done = self.lessons.iter().filter(|(id, _)| child.completed.contains(id)).count();
        // Header: "‹ Children", the name and progress, the gear.
        let header = Rect::from_min_size(full.min, Vec2::new(full.width(), 72.0));
        ui.painter().text(Pos2::new(header.center().x, header.top() + 26.0), Align2::CENTER_CENTER, &child.name, semibold(17.0), theme.text);
        ui.painter().text(
            Pos2::new(header.center().x, header.top() + 46.0),
            Align2::CENTER_CENTER,
            tf(ru, "cur.sub", &[("n", &done.to_string()), ("total", &self.lessons.len().to_string())]),
            regular(12.5),
            theme.text2,
        );
        let back = ui
            .scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(header.min + Vec2::new(24.0, 16.0), Vec2::new(200.0, 40.0))), |ui| {
                un::pill_button(ui, Some(Icon::ChevronLeft), t(ru, "cur.back"), theme.surface2, theme.text)
            })
            .inner;
        if back.clicked() {
            self.screen = Screen::Children;
        }
        let gear = ui
            .scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(header.right() - 62.0, header.top() + 17.0), Vec2::splat(38.0))), |ui| {
                un::circle_button(ui, Icon::Gear, 38.0, theme.surface2, t(ru, "settings.title"))
            })
            .inner;
        if gear.clicked() {
            self.toggle_settings();
        }
        // The content: a fixed 720px column, the CTA pinned below.
        let bar_h = 104.0;
        let content = Rect::from_min_max(Pos2::new(full.left(), header.bottom()), Pos2::new(full.right(), full.bottom() - bar_h));
        let column = 720.0f32.min(full.width() - 48.0);
        let left = full.center().x - column / 2.0;
        let mut clicked_lesson: Option<String> = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                self.status_card(ui, left, column);
                if let Some(next) = &next {
                    let meta = LessonMeta::of(next, &self.lessons, ru);
                    let (space, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 96.0 + 24.0), Sense::hover());
                    let hero = Rect::from_min_size(Pos2::new(left, space.top()), Vec2::new(column, 96.0));
                    let response = ui.interact(hero, Id::new("hero"), Sense::click());
                    un::card(ui.painter(), hero, &theme, 14.0, false);
                    lesson_art(ui.painter(), Rect::from_min_size(hero.min + Vec2::new(16.0, 16.0), Vec2::splat(64.0)), meta.number, &theme);
                    let x = hero.left() + 96.0;
                    ui.painter().text(Pos2::new(x, hero.top() + 26.0), Align2::LEFT_CENTER, t(ru, if paused { "cur.pausedLabel" } else { "cur.nextLabel" }).to_uppercase(), semibold(12.0), theme.text2);
                    ui.painter().text(Pos2::new(x, hero.top() + 50.0), Align2::LEFT_CENTER, &meta.title, bold(22.0), theme.text);
                    ui.painter().text(
                        Pos2::new(x, hero.top() + 74.0),
                        Align2::LEFT_CENTER,
                        format!("{} · {}", tf(ru, "cur.lessonN", &[("n", &meta.number.to_string())]), meta.skill),
                        regular(15.0),
                        theme.text2,
                    );
                    un::focus_ring(ui, &response, hero, 14.0);
                    if response.clicked() {
                        clicked_lesson = Some(next.clone());
                    }
                }
                if let Some(id) = self.trail(ui, &child, next.as_deref(), left, column, &theme) {
                    clicked_lesson = Some(id);
                }
                ui.add_space(24.0);
            });
        });
        // The pinned bar: a solid surface with a hairline.
        let bar = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - bar_h), full.max);
        ui.painter().rect_filled(bar, 0.0, theme.surface);
        ui.painter().line_segment([bar.left_top(), bar.right_top()], Stroke::new(1.0, theme.hairline_strong));
        ui.painter().text(Pos2::new(bar.center().x, bar.top() + 22.0), Align2::CENTER_CENTER, t(ru, "cur.hint"), regular(12.5), theme.text2);
        if let Some(next) = &next {
            let meta = LessonMeta::of(next, &self.lessons, ru);
            let w = 450.0f32.min(bar.width() - 48.0);
            let button = Rect::from_min_size(Pos2::new(bar.center().x - w / 2.0, bar.top() + 40.0), Vec2::new(w, 50.0));
            let clicked = ui
                .scope_builder(egui::UiBuilder::new().max_rect(button), |ui| {
                    un::primary_button(ui, &tf(ru, "cur.cta", &[("lesson", &meta.title)]), w, None)
                })
                .inner
                .clicked();
            if clicked {
                clicked_lesson = Some(next.clone());
            }
        }
        if let Some(lesson) = clicked_lesson {
            self.start_lesson(Some(child.id.clone()), lesson);
        }
    }

    /// The ten nodes on a snaking path (row 0 left to right, row 1 right to left, ...); returns
    /// the lesson clicked.
    fn trail(&self, ui: &mut Ui, child: &Profile, next: Option<&str>, left: f32, column: f32, theme: &Theme) -> Option<String> {
        let ru = self.ru();
        let row_h = 150.0;
        let rows = self.lessons.len().div_ceil(2);
        let (space, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), rows as f32 * row_h + 20.0), Sense::hover());
        let xs = [left + column * 0.25, left + column * 0.75];
        let point = |i: usize| {
            let row = i / 2;
            let col = if row.is_multiple_of(2) { i % 2 } else { 1 - i % 2 };
            Pos2::new(xs[col], space.top() + 50.0 + row as f32 * row_h)
        };
        let painter = ui.painter().clone();
        // The path: straight in a row, a half loop at the side between rows.
        let mut path = vec![];
        for i in 0..self.lessons.len() {
            let p = point(i);
            if i > 0 && point(i - 1).y != p.y {
                let a = point(i - 1);
                let r = (p.y - a.y) / 2.0;
                let side = if a.x > left + column / 2.0 { 1.0 } else { -1.0 };
                for k in 1..=16 {
                    let ang = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / 16.0;
                    path.push(Pos2::new(a.x + side * r * ang.cos(), a.y + r + r * ang.sin()));
                }
            } else {
                path.push(p);
            }
        }
        painter.extend(egui::Shape::dashed_line(&path, Stroke::new(8.0, theme.surface3), 3.0, 9.0));
        let time = ui.input(|i| i.time);
        let mut clicked = None;
        for (i, (id, _)) in self.lessons.iter().enumerate() {
            let meta = LessonMeta::of(id, &self.lessons, ru);
            let c = point(i);
            let state = if child.completed.contains(id) {
                0
            } else if child.paused.as_deref() == Some(id.as_str()) {
                2
            } else if Some(id.as_str()) == next {
                1
            } else {
                3
            };
            let hit = Rect::from_center_size(c + Vec2::new(0.0, 30.0), Vec2::new(160.0, 130.0));
            let response = ui.interact(hit, Id::new(("node", id)), Sense::click());
            if (state == 1 || state == 2) && !self.setup.reduce_motion {
                let phase = (time % 1.8 / 1.8) as f32;
                painter.circle_stroke(c, 36.0 + phase * 16.0, Stroke::new(10.0 * (1.0 - phase), theme.orange.gamma_multiply(0.5 * (1.0 - phase))));
                ui.ctx().request_repaint();
            }
            match state {
                0 => {
                    painter.circle_filled(c, 36.0, theme.green);
                    icon(&painter, Icon::Check, c, 30.0, Color32::WHITE);
                }
                1 => {
                    painter.circle_filled(c, 44.0, theme.orange);
                    painter.text(c, Align2::CENTER_CENTER, meta.number.to_string(), rounded(40.0), Color32::WHITE);
                }
                2 => {
                    painter.circle_filled(c, 36.0, theme.surface2);
                    painter.circle_stroke(c, 36.0, Stroke::new(4.0, theme.orange));
                    painter.text(c, Align2::CENTER_CENTER, meta.number.to_string(), rounded(36.0), theme.orange);
                    let b = c + Vec2::new(26.0, 24.0);
                    painter.circle_filled(b, 13.0, theme.surface);
                    painter.circle_filled(b, 11.0, theme.orange);
                    icon(&painter, Icon::Pause, b, 12.0, Color32::WHITE);
                }
                _ => {
                    painter.circle_filled(c, 33.0, theme.surface2);
                    painter.circle_stroke(c, 33.0, Stroke::new(1.0, theme.hairline_strong));
                    painter.text(c, Align2::CENTER_CENTER, meta.number.to_string(), rounded(30.0), theme.text2);
                }
            }
            let (sub, color) = match state {
                0 => (t(ru, "cur.done").to_string(), theme.green),
                1 => (t(ru, "cur.next").to_string(), theme.orange),
                2 => (t(ru, "cur.paused").to_string(), theme.orange),
                _ => (meta.skill.clone(), theme.text2),
            };
            painter.text(c + Vec2::new(0.0, 54.0), Align2::CENTER_CENTER, &meta.title, semibold(15.0), theme.text);
            painter.text(c + Vec2::new(0.0, 73.0), Align2::CENTER_CENTER, sub, regular(12.5), color);
            un::focus_ring(ui, &response, hit, 14.0);
            if response.clicked() {
                clicked = Some(id.clone());
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        clicked
    }

    // -------------------------------------------------------------- lesson

    fn lesson_screen(&mut self, ui: &mut Ui) {
        let theme = Theme::of(ui);
        let ru = self.ru();
        let full = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(full, 0.0, crate::un::overlay_colors::LETTERBOX);
        let strip_h = 40.0;
        let area = Rect::from_min_max(full.min, Pos2::new(full.right(), full.bottom() - strip_h));
        // The webcam picture, 16:9 (or the frame's own aspect), letterboxed or pillarboxed.
        let (fw, fh) = self.frame_size;
        let aspect = fw.max(1) as f32 / fh.max(1) as f32;
        let mut size = Vec2::new(area.width(), area.width() / aspect);
        if size.y > area.height() {
            size = Vec2::new(area.height() * aspect, area.height());
        }
        let picture = Rect::from_center_size(area.center(), size);
        match &self.texture {
            Some(texture) => {
                painter.image(texture.id(), picture, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                painter.rect_filled(picture, 0.0, Color32::from_rgb(0x2A, 0x2A, 0x2E));
            }
        }
        let map = Mapper { rect: picture };
        let state = self.tutor_state();
        let end = self.end_phase();
        let paused = self.run.as_ref().is_some_and(|r| r.paused);
        let (table, tracked) = match &self.worker {
            Some(w) => {
                let l = w.latest.lock();
                (l.state.clone(), l.tracked.clone())
            }
            None => (None, vec![]),
        };
        if let (Some(table), false) = (&table, self.preparing()) {
            let highlight = self.snapshot.as_ref().and_then(|s| s.highlight_count);
            overlay::draw_toys(&painter, &map, table, &tracked, highlight, state == TutorState::Celebrating);
        }
        if self.sim.is_some() {
            let roi = self.worker.as_ref().and_then(|w| w.roi());
            if let Some(roi) = roi {
                overlay::dashed_rect(&painter, map.box_rect(&roi), Color32::WHITE);
            }
            // Under the lesson chip, clear of the tutor panel: the picture is not real.
            let galley = painter.layout_no_wrap(t(ru, "lesson.sim").to_string(), semibold(13.0), Color32::BLACK);
            let chip = Rect::from_min_size(Pos2::new(area.left() + 16.0, area.top() + 72.0), Vec2::new(galley.size().x + 28.0, 30.0));
            painter.rect_filled(chip, 15.0, crate::un::overlay_colors::BADGE_LIT);
            painter.galley(chip.center() - galley.size() / 2.0, galley, Color32::BLACK);
        }
        if state == TutorState::Celebrating && end.is_none() {
            painter.rect_filled(picture, 0.0, Color32::from_black_alpha(168));
            let count = table.as_ref().and_then(|t| t.cube_count).unwrap_or(0);
            count_ticket(&painter, picture.center(), count, ui.input(|i| i.time), self.setup.reduce_motion);
        }
        if paused {
            painter.rect_filled(picture, 0.0, Color32::from_black_alpha(140));
        }
        // Top: the lesson chip and the parent toolbar.
        let meta = self.run.as_ref().map(|r| LessonMeta::of(&r.lesson, &self.lessons, ru));
        if let Some(meta) = &meta {
            let galley = painter.layout_no_wrap(meta.title.clone(), semibold(15.0), theme.text);
            let chip = Rect::from_min_size(area.min + Vec2::new(16.0, 16.0), Vec2::new(galley.size().x + 56.0, 44.0));
            un::card(&painter, chip, &theme, 22.0, true);
            painter.circle_filled(Pos2::new(chip.left() + 22.0, chip.center().y), 13.0, theme.orange);
            painter.text(Pos2::new(chip.left() + 22.0, chip.center().y), Align2::CENTER_CENTER, meta.number.to_string(), rounded(14.0), Color32::WHITE);
            painter.galley(Pos2::new(chip.left() + 42.0, chip.center().y - galley.size().y / 2.0), galley, theme.text);
        }
        let bar = Rect::from_min_size(Pos2::new(area.right() - 16.0 - 5.0 * 44.0 - 8.0, area.top() + 16.0), Vec2::new(5.0 * 44.0 + 8.0, 44.0));
        un::card(&painter, bar, &theme, 22.0, true);
        let tools = [
            (Icon::Pause, if paused { "lesson.resume" } else { "lesson.pause" }, "P"),
            (Icon::Repeat, "lesson.repeat", "Space"),
            (Icon::Skip, "lesson.skip", "N"),
        ];
        for (i, (kind, label, key)) in tools.iter().enumerate() {
            let r = Rect::from_min_size(bar.min + Vec2::new(4.0 + i as f32 * 44.0, 0.0), Vec2::splat(44.0));
            let response = ui.interact(r, Id::new(("tool", i)), Sense::click());
            if response.hovered() {
                painter.circle_filled(r.center(), 18.0, theme.surface3);
            }
            icon(&painter, if *kind == Icon::Pause && paused { Icon::Play } else { *kind }, r.center(), 20.0, theme.text);
            un::focus_ring(ui, &response, r.shrink(4.0), 18.0);
            let response = response.on_hover_text(format!("{} ({key})", t(ru, label)));
            if response.clicked() {
                match i {
                    0 if paused => self.resume(),
                    0 => self.pause(),
                    1 => self.with_session(|s| s.engine.repeat()),
                    _ => self.with_session(|s| s.engine.skip()),
                }
            }
        }
        let lang_rect = Rect::from_min_size(bar.min + Vec2::new(4.0 + 3.0 * 44.0, 0.0), Vec2::splat(44.0));
        let lang = ui.interact(lang_rect, Id::new("tool-lang"), Sense::click());
        if lang.hovered() {
            painter.circle_filled(lang_rect.center(), 18.0, theme.surface3);
        }
        painter.text(lang_rect.center(), Align2::CENTER_CENTER, if ru { "RU" } else { "EN" }, bold(13.0), theme.text);
        if lang.on_hover_text("EN / RU (L)").clicked() {
            self.set_language(if ru { Language::English } else { Language::Russian });
        }
        let gear_rect = Rect::from_min_size(bar.min + Vec2::new(4.0 + 4.0 * 44.0, 0.0), Vec2::splat(44.0));
        let gear = ui.interact(gear_rect, Id::new("tool-gear"), Sense::click());
        if gear.hovered() {
            painter.circle_filled(gear_rect.center(), 18.0, theme.surface3);
        }
        icon(&painter, Icon::Gear, gear_rect.center(), 20.0, theme.text);
        if gear.on_hover_text(format!("{} (S)", t(ru, "lesson.settings"))).clicked() {
            self.toggle_settings();
        }
        // Bottom: the tutor panel (or the preparing / failed card), the number pad above it.
        let panel_w = (area.width() - 32.0).min(940.0);
        if paused {
            self.paused_card(ui, area, &theme);
        } else if self.preparing() || self.error.is_some() {
            let body = t(ru, if self.error.is_some() { "error.body" } else { "prep.body" });
            let h = 18.0 + 34.0 + un::text_height(ui, body, regular(15.0), panel_w - 40.0) + 12.0 + if self.error.is_some() { 54.0 } else { 34.0 } + 16.0;
            let r = Rect::from_min_size(Pos2::new(area.center().x - panel_w / 2.0, picture.bottom().min(area.bottom()) - 16.0 - h), Vec2::new(panel_w, h));
            self.status_body(ui, r, &theme, true);
        } else if let Some(snapshot) = self.snapshot.clone() {
            let top = self.tutor_panel(ui, &snapshot, state, area, picture, panel_w, &theme);
            if snapshot.show_number_pad {
                self.number_pad(ui, Pos2::new(area.center().x, top - 12.0), &theme);
            }
        }
        // The shortcut strip under the picture.
        let strip = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - strip_h), full.max);
        painter.rect_filled(strip, 0.0, crate::un::overlay_colors::LETTERBOX);
        let hints = [
            (t(ru, "key.space"), t(ru, "lesson.repeat")),
            ("N", t(ru, "lesson.skip")),
            ("P", t(ru, "lesson.pauseContinue")),
            ("L", "EN / RU"),
            ("S", t(ru, "lesson.settings")),
        ];
        let widths: Vec<f32> = hints
            .iter()
            .map(|(k, v)| painter.layout_no_wrap(k.to_string(), semibold(11.5), Color32::WHITE).size().x + 12.0 + 8.0 + painter.layout_no_wrap(v.to_string(), regular(12.5), Color32::WHITE).size().x + 28.0)
            .collect();
        let mut x = strip.center().x - widths.iter().sum::<f32>() / 2.0;
        for (i, (k, v)) in hints.iter().enumerate() {
            let w = un::key_cap(&painter, Pos2::new(x, strip.center().y), k, Color32::from_white_alpha(14), Color32::from_white_alpha(46), Color32::WHITE);
            painter.text(Pos2::new(x + w + 8.0, strip.center().y), Align2::LEFT_CENTER, *v, regular(12.5), Color32::from_white_alpha(180));
            x += widths[i];
        }
        if let Some(end) = end {
            self.end_screen(ui, full, end, &theme);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }

    fn with_session(&self, f: impl FnOnce(&Session)) {
        if let Some(session) = self.run.as_ref().and_then(|r| r.session.as_ref()) {
            f(session);
        }
    }

    /// The tutor panel: the orb, the state label (with mic bars), the tutor's line (it wraps and
    /// the panel grows upwards), "Heard: «…»". Returns its top edge.
    #[allow(clippy::too_many_arguments)]
    fn tutor_panel(&self, ui: &mut Ui, s: &EngineSnapshot, state: TutorState, area: Rect, picture: Rect, width: f32, theme: &Theme) -> f32 {
        let ru = self.ru();
        let painter = ui.painter().clone();
        let orb = 84.0;
        let text_w = width - 24.0 * 2.0 - orb - 20.0;
        let line = if s.last_spoken.is_empty() { "…" } else { s.last_spoken.as_str() };
        let line_h = un::text_height(ui, line, bold(30.0), text_w);
        let heard = (state == TutorState::Listening && !s.heard_text.is_empty()).then(|| s.heard_text.clone());
        let h = (24.0 + 18.0 + 6.0 + line_h + if heard.is_some() { 26.0 } else { 0.0 } + 24.0).max(orb + 48.0);
        let bottom = picture.bottom().min(area.bottom()) - 16.0;
        let r = Rect::from_min_size(Pos2::new(area.center().x - width / 2.0, bottom - h), Vec2::new(width, h));
        un::card(&painter, r, theme, 22.0, true);
        let time = ui.input(|i| i.time);
        tutor_orb(&painter, Pos2::new(r.left() + 24.0 + orb / 2.0, r.center().y), orb, state, s.mic_level, time, self.setup.reduce_motion, theme);
        let x = r.left() + 24.0 + orb + 20.0;
        let mut y = r.top() + 24.0;
        let (label, color) = match state {
            TutorState::Speaking => ("lesson.speaking", theme.orange),
            TutorState::Listening => ("lesson.listening", theme.blue),
            TutorState::Watching => ("lesson.watching", theme.purple),
            TutorState::Celebrating => ("lesson.celebrating", theme.green),
            TutorState::Muted => ("lesson.muted", Color32::from_rgb(0x8E, 0x8E, 0x93)),
        };
        let label_galley = painter.layout_no_wrap(t(ru, label).to_uppercase(), semibold(13.0), color);
        let lw = label_galley.size().x;
        painter.galley(Pos2::new(x, y), label_galley, color);
        if state == TutorState::Listening {
            for i in 0..5 {
                let wave = 0.5 + 0.5 * ((time * 9.0 + i as f64 * 1.3).sin() as f32);
                let level = if s.mic_level > 0.02 { s.mic_level } else { 0.45 };
                let bh = 4.0 + 12.0 * level * if self.setup.reduce_motion { 0.6 } else { wave };
                let bx = x + lw + 10.0 + i as f32 * 5.0;
                painter.rect_filled(Rect::from_center_size(Pos2::new(bx, y + 8.0), Vec2::new(3.0, bh)), 1.5, theme.blue);
            }
        }
        y += 24.0;
        y += un::text_block(&painter, Pos2::new(x, y), line, bold(30.0), theme.text, text_w) + 6.0;
        if let Some(heard) = heard {
            painter.text(Pos2::new(x, y + 8.0), Align2::LEFT_CENTER, format!("{}: «{}»", t(ru, "lesson.heard"), heard), regular(15.0), theme.text2);
        }
        r.top()
    }

    /// One row of keys 0–10 (52–76px); the digit keys work too.
    fn number_pad(&mut self, ui: &mut Ui, bottom_center: Pos2, theme: &Theme) {
        let ru = self.ru();
        let n = 11.0;
        let key = 60.0f32;
        let w = n * key + (n - 1.0) * 8.0 + 32.0;
        let r = Rect::from_min_size(Pos2::new(bottom_center.x - w / 2.0, bottom_center.y - key - 32.0 - 24.0), Vec2::new(w, key + 32.0 + 24.0));
        un::card(ui.painter(), r, theme, 22.0, true);
        ui.painter().text(Pos2::new(r.center().x, r.top() + 18.0), Align2::CENTER_CENTER, t(ru, "lesson.numpadHint"), semibold(13.0), theme.text2);
        for i in 0..=10u32 {
            let k = Rect::from_min_size(Pos2::new(r.left() + 16.0 + i as f32 * (key + 8.0), r.top() + 36.0), Vec2::splat(key));
            let response = ui.interact(k, Id::new(("pad", i)), Sense::click());
            ui.painter().rect_filled(k, 14.0, if response.hovered() { theme.surface3 } else { theme.surface2 });
            ui.painter().rect_stroke(k, 14.0, Stroke::new(1.0, theme.hairline_strong), StrokeKind::Inside);
            ui.painter().text(k.center(), Align2::CENTER_CENTER, i.to_string(), rounded(28.0), theme.text);
            un::focus_ring(ui, &response, k, 14.0);
            if response.clicked() {
                self.with_session(|s| s.engine.tap(i));
            }
        }
    }

    fn paused_card(&mut self, ui: &mut Ui, area: Rect, theme: &Theme) {
        let ru = self.ru();
        let r = Rect::from_center_size(area.center(), Vec2::new(420.0, 220.0));
        un::card(ui.painter(), r, theme, 22.0, true);
        icon(ui.painter(), Icon::Pause, Pos2::new(r.center().x, r.top() + 36.0), 28.0, theme.orange);
        ui.painter().text(Pos2::new(r.center().x, r.top() + 72.0), Align2::CENTER_CENTER, t(ru, "lesson.pausedTitle"), bold(24.0), theme.text);
        ui.painter().text(Pos2::new(r.center().x, r.top() + 100.0), Align2::CENTER_CENTER, t(ru, "lesson.pausedBody"), regular(15.0), theme.text2);
        let button = Rect::from_min_size(Pos2::new(r.left() + 24.0, r.top() + 124.0), Vec2::new(r.width() - 48.0, 50.0));
        let resume = ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| un::primary_button(ui, &format!("{}  (P)", t(ru, "lesson.resume")), button.width(), None)).inner;
        if resume.clicked() {
            self.resume();
        }
        let back = Rect::from_min_size(Pos2::new(r.left() + 24.0, r.top() + 178.0), Vec2::new(r.width() - 48.0, 36.0));
        let response = ui.interact(back, Id::new("paused-back"), Sense::click());
        ui.painter().text(back.center(), Align2::CENTER_CENTER, t(ru, "lesson.back"), semibold(15.0), theme.blue);
        if response.clicked() {
            self.leave_lesson();
        }
    }

    // -------------------------------------------------------------- end

    fn end_screen(&mut self, ui: &mut Ui, full: Rect, phase: EndPhase, theme: &Theme) {
        let ru = self.ru();
        let painter = ui.painter().clone();
        // The end covers the window and takes its clicks.
        let _ = ui.interact(full, Id::new("end-cover"), Sense::click());
        painter.rect_filled(full, 0.0, theme.surface);
        let name = self.run.as_ref().and_then(|r| r.child.as_ref()).and_then(|c| self.profiles.get(c)).map(|p| p.name.clone()).unwrap_or_default();
        let meta = self.run.as_ref().map(|r| LessonMeta::of(&r.lesson, &self.lessons, ru));
        let c = full.center();
        let time = ui.input(|i| i.time);
        match phase {
            EndPhase::Celebrate => {
                if let Some(meta) = &meta {
                    star_ticket(&painter, c - Vec2::new(0.0, 90.0), meta, theme, time, self.setup.reduce_motion);
                    painter.text(c + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, tf(ru, "end.hooray", &[("name", &name)]), rounded(52.0), theme.text);
                    painter.text(c + Vec2::new(0.0, 88.0), Align2::CENTER_CENTER, tf(ru, "end.done", &[("lesson", &meta.title)]), regular(17.0), theme.text2);
                    let child = self.run.as_ref().and_then(|r| r.child.as_ref()).and_then(|c| self.profiles.get(c)).cloned();
                    let n = self.lessons.len();
                    let mut x = c.x - (n as f32 * 16.0) / 2.0;
                    for (id, _) in &self.lessons {
                        let done = child.as_ref().is_some_and(|ch| ch.completed.contains(id));
                        let w = if done { 22.0 } else { 10.0 };
                        painter.rect_filled(Rect::from_min_size(Pos2::new(x, c.y + 120.0), Vec2::new(w, 10.0)), 5.0, if done { theme.green } else { theme.surface3 });
                        x += w + 6.0;
                    }
                }
            }
            EndPhase::Ask => {
                tutor_orb(&painter, c - Vec2::new(0.0, 170.0), 84.0, TutorState::Listening, 0.0, time, self.setup.reduce_motion, theme);
                painter.text(c - Vec2::new(0.0, 80.0), Align2::CENTER_CENTER, t(ru, "end.ask"), rounded(52.0), theme.text);
                for (i, (yes, key)) in [(true, "Y"), (false, "N")].into_iter().enumerate() {
                    let r = Rect::from_min_size(Pos2::new(c.x - 250.0 + i as f32 * 260.0, c.y - 20.0), Vec2::new(240.0, 220.0));
                    let response = ui.interact(r, Id::new(("answer", i)), Sense::click());
                    let lift = if response.hovered() { -3.0 } else { 0.0 };
                    let r = r.translate(Vec2::new(0.0, lift));
                    let tint = if yes { theme.green } else { Color32::from_rgb(0x8E, 0x8E, 0x93) };
                    un::shadow(&painter, r, 22.0, theme.dark);
                    painter.rect_filled(r, 22.0, tint);
                    icon(&painter, if yes { Icon::Check } else { Icon::Moon }, r.center() - Vec2::new(0.0, 34.0), 56.0, Color32::WHITE);
                    painter.text(r.center() + Vec2::new(0.0, 36.0), Align2::CENTER_CENTER, t(ru, if yes { "end.yes" } else { "end.no" }), rounded(30.0), Color32::WHITE);
                    un::key_cap(&painter, Pos2::new(r.right() - 34.0, r.top() + 22.0), key, Color32::from_white_alpha(40), Color32::from_white_alpha(90), Color32::WHITE);
                    un::focus_ring(ui, &response, r, 22.0);
                    if response.clicked() {
                        self.answer_next(yes);
                    }
                }
            }
            EndPhase::Bye => {
                icon(&painter, Icon::Moon, c - Vec2::new(0.0, 120.0), 72.0, theme.purple);
                painter.text(c - Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, tf(ru, "end.bye", &[("name", &name)]), rounded(40.0), theme.text);
                let next = self.run.as_ref().and_then(|r| r.child.clone()).and_then(|c| self.next_for(&c));
                if let Some(next) = next {
                    let m = LessonMeta::of(&next, &self.lessons, ru);
                    painter.text(c + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, tf(ru, "end.nextTime", &[("lesson", &m.title)]), regular(17.0), theme.text2);
                }
                let button = Rect::from_min_size(Pos2::new(c.x - 180.0, c.y + 50.0), Vec2::new(360.0, 50.0));
                let home = ui.scope_builder(egui::UiBuilder::new().max_rect(button), |ui| un::primary_button(ui, t(ru, "end.home"), 360.0, None)).inner;
                if home.clicked() {
                    self.stop_session();
                    self.run = None;
                    self.screen = Screen::Children;
                }
            }
        }
    }

    // -------------------------------------------------------------- dialogs and menus

    fn dialogs(&mut self, ctx: &egui::Context) {
        let theme = Theme::new(ctx.theme() == egui::Theme::Dark);
        let ru = self.ru();
        let frame = Frame::new().fill(theme.surface2).corner_radius(14).inner_margin(24).stroke(Stroke::new(1.0, theme.hairline_strong));
        if let Some(name) = self.adding.clone() {
            let mut name = name;
            let mut done = None;
            let response = egui::Modal::new(Id::new("add")).frame(frame).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(t(ru, "add.title")).font(bold(22.0)).color(theme.text));
                ui.add_space(14.0);
                let field = ui.add(egui::TextEdit::singleline(&mut name).hint_text(t(ru, "add.placeholder")).font(regular(17.0)).desired_width(f32::INFINITY).margin(Vec2::new(12.0, 12.0)));
                field.request_focus();
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t(ru, "add.helper")).font(regular(12.5)).color(theme.text2));
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if un::pill_button(ui, None, t(ru, "cancel"), theme.surface2, theme.text).clicked() {
                        done = Some(false);
                    }
                    ui.add_space(8.0);
                    let ok = un::primary_button(ui, t(ru, "add.confirm"), 140.0, None);
                    if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = Some(true);
                    }
                });
            });
            if response.should_close() && done.is_none() {
                done = Some(false);
            }
            match done {
                Some(true) if !name.trim().is_empty() => {
                    self.profiles.add(name.trim());
                    self.adding = None;
                }
                Some(_) => self.adding = None,
                None => self.adding = Some(name),
            }
        }
        if let Some(id) = self.removing.clone() {
            let name = self.profiles.get(&id).map(|p| p.name.clone()).unwrap_or_default();
            let mut done = None;
            let response = egui::Modal::new(Id::new("remove")).frame(frame).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(egui::RichText::new(tf(ru, "remove.title", &[("name", &name)])).font(bold(20.0)).color(theme.text));
                ui.add_space(10.0);
                ui.label(egui::RichText::new(t(ru, "remove.body")).font(regular(15.0)).color(theme.text2));
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    if un::pill_button(ui, None, t(ru, "cancel"), theme.surface2, theme.text).clicked() {
                        done = Some(false);
                    }
                    ui.add_space(8.0);
                    if un::primary_button(ui, t(ru, "remove.confirm"), 140.0, Some(theme.red)).clicked() {
                        done = Some(true);
                    }
                });
            });
            if response.should_close() && done.is_none() {
                done = Some(false);
            }
            if let Some(remove) = done {
                if remove {
                    self.profiles.remove(&id);
                    if self.setup.child.as_deref() == Some(id.as_str()) {
                        self.setup.child = None;
                        self.remember();
                    }
                }
                self.removing = None;
            }
        }
        if let Some((id, pos)) = self.menu.clone() {
            let area = egui::Area::new(Id::new("child-menu")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
                Frame::new().fill(theme.surface2).corner_radius(12).stroke(Stroke::new(1.0, theme.hairline_strong)).inner_margin(6).show(ui, |ui| {
                    ui.set_width(180.0);
                    for (label, remove) in [(t(ru, "children.open"), false), (t(ru, "children.remove"), true)] {
                        let (r, response) = ui.allocate_exact_size(Vec2::new(180.0, 38.0), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(r, 8.0, theme.surface3);
                        }
                        ui.painter().text(r.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, label, regular(15.0), if remove { theme.red } else { theme.text });
                        if response.clicked() {
                            self.menu = None;
                            if remove {
                                self.removing = Some(id.clone());
                            } else {
                                self.setup.child = Some(id.clone());
                                self.screen = Screen::Trail(id.clone());
                            }
                        }
                    }
                });
            });
            if area.response.clicked_elsewhere() {
                self.menu = None;
            }
        }
    }

    // -------------------------------------------------------------- settings

    fn settings_panel(&mut self, ctx: &egui::Context) {
        let theme = Theme::new(ctx.theme() == egui::Theme::Dark);
        let ru = self.ru();
        let screen = ctx.content_rect();
        let width = 440.0;
        let rect = Rect::from_min_max(Pos2::new(screen.right() - width, screen.top()), screen.max);
        egui::Area::new(Id::new("settings")).fixed_pos(rect.min).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.painter().add(egui::Shadow { offset: [-8, 0], blur: 40, spread: 0, color: Color32::from_black_alpha(60) }.as_shape(rect, 0));
            ui.painter().rect_filled(rect, 0.0, theme.surface);
            ui.painter().line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, theme.hairline_strong));
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());
            Frame::new().inner_margin(20).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t(ru, "settings.title")).font(bold(24.0)).color(theme.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if un::circle_button(ui, Icon::Close, 36.0, theme.surface3, t(ru, "settings.close")).clicked() {
                            self.settings_open = false;
                        }
                    });
                });
                ui.add_space(10.0);
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    if self.screen == Screen::Lesson {
                        Frame::new().fill(theme.surface3).corner_radius(10).inner_margin(10).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new(t(ru, "settings.paused")).font(regular(13.0)).color(theme.text2));
                        });
                    }
                    section(ui, &theme, t(ru, "settings.devices"));
                    let mut restart = false;
                    settings_card(ui, &theme, |ui| {
                        row(ui, &theme, t(ru, "settings.camera"), |ui| {
                            let shown = self.setup.cameras.get(self.setup.camera as usize).cloned().unwrap_or_else(|| format!("#{}", self.setup.camera));
                            egui::ComboBox::from_id_salt("camera").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                                for (i, name) in self.setup.cameras.iter().enumerate() {
                                    if ui.selectable_value(&mut self.setup.camera, i as u32, name).changed() {
                                        restart = true;
                                    }
                                }
                            });
                        });
                        row(ui, &theme, t(ru, "settings.mirror"), |ui| {
                            if ui.checkbox(&mut self.setup.mirror, "").changed() {
                                restart = true;
                            }
                        });
                        row(ui, &theme, t(ru, "settings.mic"), |ui| {
                            let shown = self.setup.mic_device.clone().unwrap_or_else(|| t(ru, "settings.micDefault").to_string());
                            egui::ComboBox::from_id_salt("mic").selected_text(shown).width(220.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.setup.mic_device, None, t(ru, "settings.micDefault"));
                                for name in &self.setup.mics {
                                    ui.selectable_value(&mut self.setup.mic_device, Some(name.clone()), name);
                                }
                            });
                        });
                        row(ui, &theme, t(ru, "settings.voice"), |ui| {
                            let label = |v: VoiceChoice| match v {
                                VoiceChoice::Console => t(ru, "settings.voiceText"),
                                VoiceChoice::Off => t(ru, "settings.voiceOff"),
                                _ => t(ru, "settings.voiceAuto"),
                            };
                            egui::ComboBox::from_id_salt("voice").selected_text(label(self.setup.voice)).width(220.0).show_ui(ui, |ui| {
                                for v in [VoiceChoice::Auto, VoiceChoice::Console, VoiceChoice::Off] {
                                    ui.selectable_value(&mut self.setup.voice, v, label(v));
                                }
                            });
                        });
                    });
                    section(ui, &theme, t(ru, "settings.area"));
                    settings_card(ui, &theme, |ui| self.area_editor(ui, &theme));
                    section(ui, &theme, "");
                    settings_card(ui, &theme, |ui| {
                        toggle_row(ui, &theme, t(ru, "settings.motion"), t(ru, "settings.motionHint"), &mut self.setup.reduce_motion);
                        let before = self.setup.simulated;
                        toggle_row(ui, &theme, t(ru, "settings.sim"), t(ru, "settings.simHint"), &mut self.setup.simulated);
                        if before != self.setup.simulated {
                            restart = true;
                        }
                        if let Some(sim) = &self.sim {
                            row(ui, &theme, t(ru, "settings.toys"), |ui| {
                                if ui.button("+").clicked() {
                                    sim.add(1);
                                }
                                ui.label(egui::RichText::new(sim.count().to_string()).font(semibold(15.0)));
                                if ui.button("−").clicked() {
                                    sim.add(-1);
                                }
                            });
                        }
                    });
                    section(ui, &theme, t(ru, "settings.shortcuts"));
                    settings_card(ui, &theme, |ui| {
                        for (k, v) in [
                            (t(ru, "key.space"), t(ru, "lesson.repeat")),
                            ("N", t(ru, "lesson.skip")),
                            ("P", t(ru, "lesson.pauseContinue")),
                            ("L", "EN / RU"),
                            ("S", t(ru, "lesson.settings")),
                            ("Esc", t(ru, "settings.close")),
                            ("Y / N", t(ru, "end.ask")),
                            ("0–9", t(ru, "lesson.muted")),
                        ] {
                            let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
                            let w = un::key_cap(ui.painter(), Pos2::new(r.left(), r.center().y), k, theme.surface3, theme.hairline_strong, theme.text);
                            ui.painter().text(Pos2::new(r.left() + w.max(56.0) + 12.0, r.center().y), Align2::LEFT_CENTER, v, regular(14.0), theme.text2);
                        }
                    });
                    ui.add_space(20.0);
                    if restart {
                        self.remember();
                        self.start_camera();
                    } else {
                        self.remember();
                    }
                });
            });
        });
    }

    /// The table-area editor: the picture with the area's frame; drag the frame or its corners.
    fn area_editor(&mut self, ui: &mut Ui, theme: &Theme) {
        let ru = self.ru();
        let w = ui.available_width();
        let (fw, fh) = self.frame_size;
        let h = w * fh.max(1) as f32 / fw.max(1) as f32;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        match &self.texture {
            Some(texture) => painter.image(texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE),
            None => painter.rect_filled(rect, 10.0, Color32::from_rgb(0x2A, 0x2A, 0x2E)),
        };
        let map = Mapper { rect };
        let Some(worker) = &self.worker else { return };
        let roi = worker.roi().unwrap_or(Box2::new(0.05, 0.05, 0.95, 0.95));
        let area = map.box_rect(&roi);
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(70));
        painter.image(
            self.texture.as_ref().map(|t| t.id()).unwrap_or_default(),
            area,
            Rect::from_min_max(Pos2::new(roi.x1, roi.y1), Pos2::new(roi.x2, roi.y2)),
            if self.texture.is_some() { Color32::WHITE } else { Color32::TRANSPARENT },
        );
        overlay::dashed_rect(&painter, area, Color32::WHITE);
        let corners = [area.left_top(), area.right_top(), area.right_bottom(), area.left_bottom()];
        for c in corners {
            painter.circle_filled(c, 7.0, Color32::WHITE);
            painter.circle_stroke(c, 7.0, Stroke::new(1.5, Color32::from_black_alpha(140)));
        }
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.area_drag = match corners.iter().position(|c| c.distance(pos) < 14.0) {
                    Some(i) => Some(AreaDrag::Corner(i)),
                    None if area.contains(pos) => Some(AreaDrag::Move(map.to_norm(pos), roi)),
                    None => None,
                };
            }
        }
        if response.dragged() {
            if let (Some(drag), Some(pos)) = (self.area_drag, response.interact_pointer_pos()) {
                let (x, y) = map.to_norm(pos);
                let next = match drag {
                    AreaDrag::Move((sx, sy), start) => {
                        let dx = (x - sx).clamp(-start.x1, 1.0 - start.x2);
                        let dy = (y - sy).clamp(-start.y1, 1.0 - start.y2);
                        Box2::new(start.x1 + dx, start.y1 + dy, start.x2 + dx, start.y2 + dy)
                    }
                    AreaDrag::Corner(i) => {
                        let (mut x1, mut y1, mut x2, mut y2) = (roi.x1, roi.y1, roi.x2, roi.y2);
                        match i {
                            0 => (x1, y1) = (x, y),
                            1 => (x2, y1) = (x, y),
                            2 => (x2, y2) = (x, y),
                            _ => (x1, y2) = (x, y),
                        }
                        Box2::from_corners(x1, y1, x2, y2)
                    }
                };
                if next.width() > 0.05 && next.height() > 0.05 {
                    worker.set_roi(Some(next));
                }
            }
        }
        if response.drag_stopped() {
            self.area_drag = None;
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(w - 120.0);
                ui.label(egui::RichText::new(t(ru, "settings.areaHint")).font(regular(12.5)).color(theme.text2));
            });
            if un::pill_button(ui, None, t(ru, "settings.reset"), theme.surface2, theme.blue).clicked() {
                worker.set_roi(None);
            }
        });
    }
}

// ------------------------------------------------------------------ drawing helpers

fn section(ui: &mut Ui, theme: &Theme, title: &str) {
    if !title.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(title.to_uppercase()).font(semibold(12.5)).color(theme.text2));
    }
}

fn settings_card(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui)) {
    Frame::new().fill(theme.surface2).corner_radius(14).inner_margin(14).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn row(ui: &mut Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.label(egui::RichText::new(label).font(semibold(15.0)).color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

fn toggle_row(ui: &mut Ui, theme: &Theme, label: &str, hint: &str, value: &mut bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(ui.available_width() - 70.0);
            ui.label(egui::RichText::new(label).font(semibold(15.0)).color(theme.text));
            ui.label(egui::RichText::new(hint).font(regular(12.5)).color(theme.text2));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (r, response) = ui.allocate_exact_size(Vec2::new(51.0, 31.0), Sense::click());
            if response.clicked() {
                *value = !*value;
            }
            ui.painter().rect_filled(r, 15.5, if *value { theme.green } else { theme.surface3 });
            let knob = if *value { r.right() - 15.5 } else { r.left() + 15.5 };
            ui.painter().circle_filled(Pos2::new(knob, r.center().y), 13.5, Color32::WHITE);
            un::focus_ring(ui, &response, r, 15.5);
        });
    });
}

/// Lesson art is a placeholder until an illustrator draws it: stripes and the lesson's number.
fn lesson_art(painter: &egui::Painter, r: Rect, number: usize, theme: &Theme) {
    painter.rect_filled(r, 12.0, theme.surface3);
    let clip = painter.with_clip_rect(r);
    let mut x = r.left() - r.height();
    while x < r.right() {
        clip.line_segment([Pos2::new(x, r.bottom()), Pos2::new(x + r.height(), r.top())], Stroke::new(5.0, theme.text3.gamma_multiply(0.35)));
        x += 12.0;
    }
    painter.text(r.center(), Align2::CENTER_CENTER, number.to_string(), rounded(26.0), theme.orange);
}

/// The tutor's orb: colour, icon and motion say the state, never colour alone.
#[allow(clippy::too_many_arguments)]
fn tutor_orb(painter: &egui::Painter, c: Pos2, size: f32, state: TutorState, level: f32, time: f64, still: bool, theme: &Theme) {
    let r = size / 2.0;
    let color = match state {
        TutorState::Speaking => theme.orange,
        TutorState::Listening => theme.blue,
        TutorState::Watching => theme.purple,
        TutorState::Celebrating => theme.green,
        TutorState::Muted => Color32::from_rgb(0x8E, 0x8E, 0x93),
    };
    let t = if still { 0.0 } else { time };
    match state {
        TutorState::Speaking => {
            for i in 0..2 {
                let phase = ((t / 1.8 + i as f64 * 0.5) % 1.0) as f32;
                painter.circle_stroke(c, r * (1.0 + phase * 0.75), Stroke::new(3.0, color.gamma_multiply(0.65 * (1.0 - phase))));
            }
        }
        TutorState::Listening => {
            let breath = if level > 0.02 { level } else { 0.35 + 0.35 * (t * 4.0).sin() as f32 };
            painter.circle_filled(c, r * (1.1 + 0.3 * breath), color.gamma_multiply(0.18));
            painter.circle_filled(c, r * (1.05 + 0.15 * breath), color.gamma_multiply(0.28));
        }
        TutorState::Watching => {
            let n = 18;
            let spin = (t / 9.0 * std::f64::consts::TAU) as f32;
            for i in 0..n {
                let a0 = spin + i as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = a0 + std::f32::consts::TAU / n as f32 * 0.55;
                let rr = r * 1.2;
                painter.line_segment([c + Vec2::angled(a0) * rr, c + Vec2::angled(a1) * rr], Stroke::new(3.0, color));
            }
        }
        TutorState::Celebrating => {
            let phase = ((t / 1.2) % 1.0) as f32;
            painter.circle_filled(c, r * (0.95 + phase * 0.65), color.gamma_multiply(0.35 * (1.0 - phase)));
        }
        TutorState::Muted => {}
    }
    painter.circle_filled(c, r, color);
    let kind = match state {
        TutorState::Speaking => Icon::Speaker,
        TutorState::Listening => Icon::Mic,
        TutorState::Watching => Icon::Eye,
        TutorState::Celebrating => Icon::Star,
        TutorState::Muted => Icon::MicSlash,
    };
    icon(painter, kind, c, size * 0.46, Color32::WHITE);
}

/// A correct count: the stamped ticket with the big green numeral and confetti.
fn count_ticket(painter: &egui::Painter, c: Pos2, count: u32, time: f64, still: bool) {
    confetti(painter, c, time, still);
    let r = Rect::from_center_size(c, Vec2::new(240.0, 220.0));
    painter.add(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(90) }.as_shape(r, 22));
    painter.rect_filled(r, 22.0, Color32::WHITE);
    painter.text(c - Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, count.to_string(), rounded(132.0), crate::un::overlay_colors::BADGE_CORRECT);
    painter.extend(egui::Shape::dashed_line(&[Pos2::new(r.left() + 16.0, c.y + 52.0), Pos2::new(r.right() - 16.0, c.y + 52.0)], Stroke::new(1.5, Color32::from_black_alpha(40)), 5.0, 5.0));
    for (i, color) in [0xD8402Fu32, 0xF2BF2A, 0x3A6CD0].into_iter().enumerate() {
        let x = c.x - 34.0 + i as f32 * 34.0;
        painter.rect_filled(Rect::from_center_size(Pos2::new(x, c.y + 80.0), Vec2::new(24.0, 16.0)), 5.0, Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8));
    }
    let stamp = Pos2::new(r.right() - 32.0, r.top() + 32.0);
    painter.circle_stroke(stamp, 20.0, Stroke::new(2.5, Color32::from_rgb(0xE8, 0x5F, 0x2A)));
    icon(painter, Icon::Check, stamp, 22.0, Color32::from_rgb(0xE8, 0x5F, 0x2A));
}

/// The lesson-end ticket: three stars over the lesson's chip.
fn star_ticket(painter: &egui::Painter, c: Pos2, meta: &LessonMeta, theme: &Theme, time: f64, still: bool) {
    confetti(painter, c, time, still);
    let r = Rect::from_center_size(c, Vec2::new(300.0, 130.0));
    un::shadow(painter, r, 22.0, theme.dark);
    painter.rect_filled(r, 22.0, Color32::WHITE);
    for (i, size) in [36.0f32, 46.0, 36.0].into_iter().enumerate() {
        painter.add(un::star(Pos2::new(c.x - 56.0 + i as f32 * 56.0, r.top() + 40.0), size / 2.0, Color32::from_rgb(0xFF, 0xC9, 0x3C)));
    }
    painter.extend(egui::Shape::dashed_line(&[Pos2::new(r.left() + 16.0, r.top() + 76.0), Pos2::new(r.right() - 16.0, r.top() + 76.0)], Stroke::new(1.5, Color32::from_black_alpha(40)), 5.0, 5.0));
    let chip = Pos2::new(c.x - 70.0, r.top() + 102.0);
    painter.circle_filled(chip, 12.0, theme.orange);
    painter.text(chip, Align2::CENTER_CENTER, meta.number.to_string(), rounded(13.0), Color32::WHITE);
    painter.text(chip + Vec2::new(20.0, 0.0), Align2::LEFT_CENTER, &meta.title, semibold(15.0), Color32::from_rgba_unmultiplied(10, 10, 12, 180));
}

/// Sixteen pieces flying out over 2.4 s (none with Reduce motion).
fn confetti(painter: &egui::Painter, c: Pos2, time: f64, still: bool) {
    if still {
        return;
    }
    let colors = [0xFFC93Cu32, 0xE85F2A, 0x1F86E0, 0x1FA84A, 0x8B4DEB, 0xE03253];
    let p = ((time % 2.4) / 2.4) as f32;
    for i in 0..16 {
        let a = (i as f32 * 22.5 + (i % 2) as f32 * 7.0).to_radians();
        let pos = c + Vec2::angled(a) * 230.0 * p;
        let col = colors[i % 6];
        let color = Color32::from_rgb((col >> 16) as u8, (col >> 8) as u8, col as u8).gamma_multiply(if p < 0.7 { 1.0 } else { (1.0 - p) / 0.3 });
        let h = if i % 3 == 0 { 24.0 } else { 14.0 };
        let rot = p * 2.8;
        let d = Vec2::angled(rot) * h / 2.0;
        painter.line_segment([pos - d, pos + d], Stroke::new(8.0, color));
    }
}

/// The tutor's language when nothing was chosen yet: the system's (Russian when it is Russian).
fn system_language() -> Language {
    let locale = std::env::var("LANG").or_else(|_| std::env::var("LC_ALL")).unwrap_or_default();
    if locale.starts_with("ru") || sys_locale_is_ru() {
        Language::Russian
    } else {
        Language::English
    }
}

#[cfg(windows)]
fn sys_locale_is_ru() -> bool {
    // GetUserDefaultUILanguage: the primary language id of Russian is 0x19.
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    unsafe { GetUserDefaultUILanguage() & 0x3FF == 0x19 }
}

#[cfg(not(windows))]
fn sys_locale_is_ru() -> bool {
    false
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
    let _ = writeln!(file.lock().unwrap(), "--- session {}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    Some(Arc::new(move |line: &str| {
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(f, "{:8.2}  {line}", started.elapsed().as_secs_f64());
        }
    }))
}

fn tap(run: &mut LessonRun, value: u32) {
    run.pad_buffer.clear();
    run.pad_deadline = None;
    if let Some(s) = &run.session {
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

impl UnApp {
    /// `UN_SCREENSHOT=<file.png>` (with `UN_SCREENSHOT_AFTER=<frames>`, default 120): saves the
    /// window once and closes it.
    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = std::env::var_os("UN_SCREENSHOT") else { return };
        self.frames += 1;
        let after: u32 = std::env::var("UN_SCREENSHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
        if self.frames == after {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
        });
        if let Some(image) = shot {
            let [w, h] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buffer) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                let _ = buffer.save(&path);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for UnApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        self.keys(&ctx);
        let theme = Theme::new(ctx.theme() == egui::Theme::Dark);
        egui::CentralPanel::default().frame(Frame::new().fill(theme.surface)).show(ui, |ui| match self.screen.clone() {
            Screen::Children => self.children_screen(ui),
            Screen::Trail(id) => self.trail_screen(ui, &id),
            Screen::Lesson => {
                if self.run.is_some() {
                    self.lesson_screen(ui);
                } else {
                    self.screen = Screen::Children;
                }
            }
        });
        self.dialogs(&ctx);
        if self.settings_open {
            self.settings_panel(&ctx);
        }
        if self.preparing() || self.screen == Screen::Lesson {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        self.screenshot(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_session();
        self.worker = None;
    }
}
