//! `un`: the Un engine from the terminal, without a window.
//!
//! ```text
//!     un strategies                       # lessons linked into this build
//!     un cameras                          # usable cameras
//!     un check-speech --language ru       # the voice says "три", the recogniser hears it
//!     un run --camera 0                   # count objects; one JSON line per state change
//!     un run --source photo.jpg --frames 30 --snapshot out.png
//!     un lesson --sim 3 --answers 3,2 --voice console   # a scripted lesson on a drawn table
//!     un lesson --camera 0 --language ru --strategy counting
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use un_app::{
    build_session, registry, MicChoice, ModelPaths, Perception, PerceptionConfig, SessionOptions, SimulatedTable, VoiceChoice,
};
use un_core::strategy::{PhaseKind, SessionConfig};
use un_core::NumberParser;
use un_core::{Box2, DetectionTracker, EstimatorConfig, Language, TableStateEstimator, TrackedDetection, TrackerConfig};
use un_engine::{CancelToken, SpeechError, SpeechInput, StatusKind};
use un_media::{CameraSource, FileSource, FrameSource, Rotation};
use un_vision::{Detector, DetectorConfig, Provider};

#[derive(Parser)]
#[command(name = "un", version, about = "Un: the laptop camera counts what is on the table and the tutor talks about it.")]
struct Cli {
    /// Directory with the models (default: UN_MODELS, models/ next to the executable, or ./models).
    #[arg(long, global = true)]
    models: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// List the lesson strategies linked into this build.
    Strategies,
    /// List usable cameras.
    Cameras,
    /// List microphones (`--mic-device` takes a part of a name).
    Mics,
    /// Microphone test: prints every utterance the recogniser hears, with the number it would take.
    Listen {
        #[arg(long, default_value = "en")]
        language: String,
        #[arg(long, default_value_t = 20.0)]
        seconds: f64,
        /// Part of a microphone name (see `un mics`); default: the system default.
        #[arg(long)]
        mic_device: Option<String>,
    },
    /// Self-test: the voice says a number and the recogniser hears it; no microphone or speakers needed.
    CheckSpeech {
        #[arg(long, default_value = "en")]
        language: String,
    },
    /// Count objects: one JSON line per state change.
    Run {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        vision: VisionArgs,
    },
    /// Run a lesson headlessly: the tutor's lines and the state go to stdout as JSON.
    Lesson {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        vision: VisionArgs,
        #[command(flatten)]
        lesson: LessonArgs,
    },
    /// Record auto-labelled camera frames (YOLO txt labels) to fine-tune a detector with tools/teach.py.
    Collect {
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        vision: VisionArgs,
        #[command(flatten)]
        collect: CollectArgs,
    },
}

#[derive(Args)]
struct SourceArgs {
    /// Camera index (see `un cameras`).
    #[arg(long, conflicts_with_all = ["source", "sim"])]
    camera: Option<u32>,
    /// Replay an image instead of the camera.
    #[arg(long)]
    source: Option<PathBuf>,
    /// No camera: a simulated table with N objects.
    #[arg(long, value_name = "N")]
    sim: Option<u32>,
    /// Requested camera resolution.
    #[arg(long, default_value = "1280x720")]
    size: String,
    /// Rotate frames clockwise (0, 90, 180, 270).
    #[arg(long, default_value_t = 0)]
    rotate: u32,
    /// Flip left/right so the picture matches the child's side of the table.
    #[arg(long)]
    mirror: bool,
    /// Stop after this many frames (0 = until the lesson ends or Ctrl-C).
    #[arg(long, default_value_t = 0)]
    frames: u32,
    /// Write the last frame here on exit.
    #[arg(long)]
    snapshot: Option<PathBuf>,
}

#[derive(Args)]
struct VisionArgs {
    /// Detector: a name in the models directory (toys, or one taught with tools/teach.py) or a path to an .onnx.
    #[arg(long, default_value = "toys")]
    model: String,
    #[arg(long, default_value_t = 0.25)]
    conf: f32,
    /// Execution provider: cpu, coreml, cuda, directml, auto.
    #[arg(long, default_value = "cpu")]
    provider: String,
    #[arg(long)]
    no_hands: bool,
    /// Count only this label.
    #[arg(long, value_name = "LABEL")]
    count: Option<String>,
    /// Table area as normalized x1,y1,x2,y2.
    #[arg(long)]
    roi: Option<String>,
}

#[derive(Args)]
struct LessonArgs {
    /// Strategy id (see `un strategies`).
    #[arg(long, default_value = "demo")]
    strategy: String,
    #[arg(long, default_value = "en")]
    language: String,
    #[arg(long)]
    child_name: Option<String>,
    /// How many objects the family owns.
    #[arg(long, default_value_t = 10)]
    total: u32,
    /// Session length in tasks.
    #[arg(long, default_value_t = 10)]
    tasks: u32,
    #[arg(long, default_value_t = 0)]
    level: u32,
    #[arg(long)]
    seed: Option<u64>,
    /// Scripted lines the child says, e.g. 3,2,4 or Mia,yes,3 (no microphone).
    #[arg(long)]
    answers: Option<String>,
    /// Speech output: auto, piper, console, off.
    #[arg(long, default_value = "auto")]
    voice: String,
    /// Speech input: auto, mic, off.
    #[arg(long, default_value = "auto")]
    mic: String,
    /// Microphone name substring.
    #[arg(long)]
    mic_device: Option<String>,
    /// Play a short chime when the tutor starts listening.
    #[arg(long)]
    chime: bool,
    /// Seconds between the tutor's reminders.
    #[arg(long, default_value_t = 10.0)]
    nudge: f64,
}

#[derive(Args)]
struct CollectArgs {
    /// Dataset directory: images/, labels/ and classes.json (default datasets/toys).
    #[arg(long, default_value = "datasets/toys")]
    out: PathBuf,
    /// Comma-separated model labels to keep (default: every non-decoy label).
    #[arg(long)]
    labels: Option<String>,
    /// How long to record.
    #[arg(long, default_value_t = 90.0)]
    seconds: f64,
    /// Seconds between saved frames.
    #[arg(long, default_value_t = 0.5)]
    every: f64,
    /// Also save frames in which the model found nothing.
    #[arg(long)]
    keep_empty: bool,
    /// Start a fresh dataset instead of adding to it.
    #[arg(long)]
    force: bool,
}

/// Parses the command line and runs it. Products call this from their own `main` after linking strategies.
pub fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();
    let paths = cli.models.map(ModelPaths::new).unwrap_or_else(ModelPaths::discover);
    match cli.command {
        Command::Strategies => {
            for info in registry().infos() {
                println!(
                    "{:<12} {}  [{}]  {}",
                    info.id,
                    info.title,
                    info.languages.iter().map(|l| l.code()).collect::<Vec<_>>().join(", "),
                    info.license
                );
            }
            Ok(())
        }
        Command::Cameras => {
            let cameras = un_media::list_cameras().map_err(|e| anyhow!("{e}"))?;
            for c in &cameras {
                println!("camera {}: {}", c.index, c.name);
            }
            if cameras.is_empty() {
                println!("no cameras found");
            }
            Ok(())
        }
        Command::Mics => {
            let mics = un_app::list_microphones();
            for name in &mics {
                println!("{name}");
            }
            if mics.is_empty() {
                println!("no microphones found");
            }
            Ok(())
        }
        Command::Listen { language, seconds, mic_device } => {
            let language = parse_language(&language)?;
            let device = match mic_device {
                Some(name) => un_speech::audio::MicDevice::Named(name),
                None => un_speech::audio::MicDevice::Default,
            };
            let mic = un_speech::MicrophoneInput::new(paths.speech(), device);
            mic.prepare(language).map_err(|e| anyhow!("{e}"))?;
            eprintln!("recogniser: {}", recognizer_name(&paths.speech()));
            eprintln!("listening on {:?} for {seconds} s; speak now", mic.device_name());
            let started = Instant::now();
            let token = CancelToken::default();
            let result = mic.listen_for_text(language, Duration::from_secs_f64(seconds), &token, &|text| {
                let number = NumberParser::parse(text, language).map(|n| n.to_string()).unwrap_or_else(|| "-".into());
                println!("{:6.2}s  {number:>2}  {text}", started.elapsed().as_secs_f64());
                false
            });
            match result {
                Err(SpeechError::Timeout { .. }) | Ok(_) => Ok(()),
                Err(error) => Err(anyhow!("{error}")),
            }
        }
        Command::CheckSpeech { language } => {
            let language = parse_language(&language)?;
            let models = paths.speech();
            eprintln!("recogniser: {}", recognizer_name(&models));
            match un_speech::check_ok(&models, language) {
                Ok((said, heard, ok)) => {
                    println!(
                        "speech check ({}): said {said:?}, heard {heard:?}{}",
                        language.locale(),
                        if ok { "" } else { " (not a 3)" }
                    );
                    if ok {
                        Ok(())
                    } else {
                        std::process::exit(1)
                    }
                }
                Err(error) => {
                    eprintln!("speech check ({}) failed: {error}", language.locale());
                    std::process::exit(1)
                }
            }
        }
        Command::Run { source, vision } => run(&paths, source, vision, None),
        Command::Lesson { source, vision, lesson } => run(&paths, source, vision, Some(lesson)),
        Command::Collect { source, vision, collect: args } => collect(&paths, source, vision, args),
    }
}

fn recognizer_name(models: &un_speech::SpeechModels) -> String {
    models.whisper().map(|w| format!("whisper-{}", w.size)).unwrap_or_else(|| "zipformer".into())
}

fn parse_language(text: &str) -> Result<Language> {
    Language::parse(text).ok_or_else(|| anyhow!("unknown language {text:?}; use en or ru"))
}

fn parse_roi(text: &str) -> Result<Box2> {
    let values: Vec<f32> =
        text.split(',').map(|v| v.trim().parse::<f32>()).collect::<Result<_, _>>().context("--roi needs four numbers")?;
    if values.len() != 4 {
        bail!("--roi needs four numbers: x1,y1,x2,y2");
    }
    Ok(Box2::from_corners(values[0], values[1], values[2], values[3]).clipped())
}

enum Source {
    Camera(CameraSource),
    File(FileSource),
    Sim(SimulatedTable),
}

fn open_source(source: &SourceArgs) -> Result<Source> {
    Ok(match (source.sim, &source.source, source.camera) {
        (Some(n), _, _) => Source::Sim(SimulatedTable::new(n)),
        (None, Some(path), _) => {
            Source::File(FileSource::open(&[path.as_path()], source.frames.max(1) as usize).map_err(|e| anyhow!("{e}"))?)
        }
        (None, None, camera) => {
            let (w, h) = source
                .size
                .to_lowercase()
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .ok_or_else(|| anyhow!("--size must look like 1280x720"))?;
            let mut cam = CameraSource::open(camera.unwrap_or(0), w, h).map_err(|e| anyhow!("{e}"))?;
            cam.rotate = Rotation::from_degrees(source.rotate).ok_or_else(|| anyhow!("--rotate must be 0, 90, 180 or 270"))?;
            Source::Camera(cam)
        }
    })
}

fn run(paths: &ModelPaths, source: SourceArgs, vision: VisionArgs, lesson: Option<LessonArgs>) -> Result<()> {
    let provider = Provider::parse(&vision.provider).ok_or_else(|| anyhow!("unknown provider {:?}", vision.provider))?;
    let roi = source.sim.is_none().then(|| vision.roi.as_deref().map(parse_roi)).flatten().transpose()?;
    let estimator_config = EstimatorConfig { roi, ..Default::default() };

    let mut frame_source = open_source(&source)?;

    let mut perception = match &frame_source {
        Source::Sim(_) => None,
        _ => Some(
            Perception::load(
                paths,
                &PerceptionConfig {
                    detector: vision.model.clone(),
                    confidence: vision.conf,
                    provider,
                    hands: !vision.no_hands,
                    count_label: vision.count.clone(),
                    estimator: estimator_config.clone(),
                    mirrored: source.mirror,
                },
            )
            .map_err(|e| anyhow!("{e}"))?,
        ),
    };
    let labels: Vec<String> = match &perception {
        Some(p) => p.labels.clone(),
        None => vec!["cube".into()],
    };
    if let Some(label) = &vision.count {
        if !labels.contains(label) {
            bail!("--count {label:?} is not a model label; labels are {labels:?}");
        }
    }
    let mut sim_estimator = TableStateEstimator::new(estimator_config);
    let mut sim_tracker = DetectionTracker::new(TrackerConfig::default());

    let session = match lesson {
        Some(args) => {
            let language = parse_language(&args.language)?;
            let answers = args
                .answers
                .as_deref()
                .map(|a| a.split(',').map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).collect::<Vec<_>>());
            let noun = args_noun(&vision, &labels, source.sim.is_some());
            let config = SessionConfig {
                language,
                child_name: args.child_name.clone(),
                total_objects: args.total,
                max_tasks: args.tasks,
                start_level: args.level,
                nudge_interval: args.nudge,
                seed: args.seed.unwrap_or_else(|| {
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1)
                }),
                noun,
                ..Default::default()
            };
            let options = SessionOptions {
                strategy: args.strategy.clone(),
                config,
                mic: MicChoice::parse(&args.mic).ok_or_else(|| anyhow!("--mic must be auto, mic or off"))?,
                mic_device: args.mic_device.clone(),
                voice: VoiceChoice::parse(&args.voice).ok_or_else(|| anyhow!("--voice must be auto, piper, console or off"))?,
                answers,
                chime: args.chime,
                ..Default::default()
            };
            let log: Arc<dyn Fn(&str) + Send + Sync> = Arc::new(|text| println!("{}", serde_json::json!({ "lesson": text })));
            Some(build_session(paths, options, Some(log)).map_err(|e| anyhow!("{e}"))?)
        }
        None => None,
    };

    eprintln!(
        "labels: {labels:?}  counting: {}  hands: {}  {}",
        vision.count.as_deref().unwrap_or("all"),
        perception.as_ref().map(|p| p.hands.is_some()).unwrap_or(false),
        session.as_ref().map(|s| format!("strategy: {}  speech: {}", s.options.strategy, s.speech_available)).unwrap_or_default()
    );
    if let Some(session) = &session {
        session.engine.start();
    }

    let started = Instant::now();
    let mut last_tracked: Vec<TrackedDetection> = Vec::new();
    let mut last_tips: Vec<(f32, f32)> = Vec::new();
    let mut processed = 0u32;
    let mut last_summary: Option<serde_json::Value> = None;
    let mut last_frame = None;
    let sim_period = Duration::from_millis(66);
    loop {
        let (state, tracked, fingers) = match &mut frame_source {
            Source::Sim(table) => {
                std::thread::sleep(sim_period);
                let out = Perception::process_detections(
                    &mut sim_estimator,
                    &mut sim_tracker,
                    &table.detections(),
                    &[],
                    started.elapsed().as_secs_f64(),
                    None,
                );
                (out.state, out.tracked, out.fingers)
            }
            Source::Camera(cam) => {
                let Some((frame, t)) = cam.next_frame().map_err(|e| anyhow!("{e}"))? else { break };
                let frame = if source.mirror { frame.flipped() } else { frame };
                let out = perception.as_mut().unwrap().process(&frame, t).map_err(|e| anyhow!("{e}"))?;
                last_frame = Some(frame);
                (out.state, out.tracked, out.fingers)
            }
            Source::File(file) => {
                let Some((frame, t)) = file.next_frame().map_err(|e| anyhow!("{e}"))? else { break };
                let frame = if source.mirror { frame.flipped() } else { frame };
                let out = perception.as_mut().unwrap().process(&frame, t).map_err(|e| anyhow!("{e}"))?;
                last_frame = Some(frame);
                (out.state, out.tracked, out.fingers)
            }
        };
        processed += 1;
        last_tracked = tracked.clone();
        let tips: Vec<(f32, f32)> =
            state.hands.iter().filter_map(|h| h.joints.get(&un_core::HandJoint::IndexTip).copied()).collect();
        last_tips = tips.clone();
        if let Some(session) = &session {
            session.engine.observe(&state);
        }
        let mut per_label: BTreeMap<String, u32> = BTreeMap::new();
        for d in &state.detections {
            *per_label.entry(d.label_or("toy")).or_default() += 1;
        }
        let summary = serde_json::json!({
            "count": state.cube_count, "stable": state.is_stable, "raw": state.raw_count,
            "labels": per_label, "numbered": tracked.iter().map(|t| t.number).collect::<Vec<_>>(),
            "hands": state.hands.len(), "fingers": state.finger_count,
            "pointing": fingers.iter().map(|d| (d.confidence * 100.0).round() / 100.0).collect::<Vec<_>>(),
            "pointed_at": state.pointed_at,
            "tips": tips.iter().map(|(x, y)| [(x * 1000.0).round() / 1000.0, (y * 1000.0).round() / 1000.0]).collect::<Vec<_>>(),
        });
        if Some(&summary) != last_summary.as_ref() {
            let mut line = summary.clone();
            line["t"] = serde_json::json!((state.timestamp * 100.0).round() / 100.0);
            println!("{line}");
            last_summary = Some(summary);
        }
        if let Some(session) = &session {
            let snap = session.engine.snapshot();
            if snap.status.kind == StatusKind::Finished || snap.strategy.phase == PhaseKind::Finished {
                break;
            }
        }
        if source.frames > 0 && processed >= source.frames {
            break;
        }
    }
    if let Some(session) = &session {
        session.engine.close();
    }
    if let (Some(path), Some(mut frame)) = (&source.snapshot, last_frame) {
        for t in &last_tracked {
            let b = &t.detection.bbox;
            frame.draw_rect(b.x1, b.y1, b.x2, b.y2, [80, 220, 120], 3);
        }
        // The index fingertips: a small red square each.
        for (x, y) in &last_tips {
            frame.draw_rect(x - 0.012, y - 0.007, x + 0.012, y + 0.007, [235, 40, 40], 4);
        }
        if let Some(roi) = perception.as_ref().and_then(|p| p.estimator.roi()) {
            frame.draw_rect(roi.x1, roi.y1, roi.x2, roi.y2, [255, 190, 60], 2);
        }
        un_media::save_image(path, &frame).map_err(|e| anyhow!("{e}"))?;
        eprintln!("snapshot: {}", path.display());
    }
    let fps = processed as f64 / started.elapsed().as_secs_f64().max(1e-6);
    eprintln!("processed {processed} frames at {fps:.1} fps");
    Ok(())
}

fn collect(paths: &ModelPaths, source: SourceArgs, vision: VisionArgs, args: CollectArgs) -> Result<()> {
    let provider = Provider::parse(&vision.provider).ok_or_else(|| anyhow!("unknown provider {:?}", vision.provider))?;
    let detector_path = paths.detector(&vision.model);
    if !detector_path.exists() {
        bail!("detector {} not found", detector_path.display());
    }
    let mut detector = Detector::load(&detector_path, DetectorConfig { confidence: vision.conf, provider, ..Default::default() })
        .map_err(|e| anyhow!("{e}"))?;
    let mut classes = detector.object_labels();
    if let Some(wanted) = &args.labels {
        let wanted: Vec<String> = wanted.split(',').map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        let unknown: Vec<&String> = wanted.iter().filter(|l| !classes.contains(l)).collect();
        if !unknown.is_empty() {
            bail!("{unknown:?} are not labels of this model; it knows {classes:?}");
        }
        classes = wanted;
    }
    let roi = vision.roi.as_deref().map(parse_roi).transpose()?;

    let (images, labels, classes_file) = (args.out.join("images"), args.out.join("labels"), args.out.join("classes.json"));
    if args.force && args.out.exists() {
        std::fs::remove_dir_all(&args.out).with_context(|| format!("removing {}", args.out.display()))?;
    }
    if let Ok(text) = std::fs::read_to_string(&classes_file) {
        let existing: Vec<String> = serde_json::from_str(&text).context("classes.json")?;
        if existing != classes {
            bail!(
                "{} was collected with classes {existing:?}, now {classes:?}; pass the same --labels or --force to start over",
                args.out.display()
            );
        }
    }
    std::fs::create_dir_all(&images)?;
    std::fs::create_dir_all(&labels)?;
    std::fs::write(&classes_file, serde_json::to_string(&classes)? + "\n")?;

    let mut frame_source = open_source(&source)?;
    if matches!(frame_source, Source::Sim(_)) {
        bail!("collect needs a camera or a file");
    }
    eprintln!("teacher: {}  classes: {classes:?}  -> {}", detector_path.display(), args.out.display());
    eprintln!("move the objects around between grabs; Ctrl-C stops early");

    let warmup = 1.0; // let auto-exposure settle before the first grab
    let started = Instant::now();
    let (mut saved, mut boxes, mut processed) = (0usize, 0usize, 0u32);
    let mut next_save = warmup;
    loop {
        let frame = match &mut frame_source {
            Source::Camera(cam) => cam.next_frame().map_err(|e| anyhow!("{e}"))?.map(|(f, _)| f),
            Source::File(file) => file.next_frame().map_err(|e| anyhow!("{e}"))?.map(|(f, _)| f),
            Source::Sim(_) => None,
        };
        let Some(frame) = frame else { break };
        let frame = if source.mirror { frame.flipped() } else { frame };
        processed += 1;
        let elapsed = started.elapsed().as_secs_f64();
        if elapsed >= args.seconds + warmup {
            break;
        }
        if elapsed >= next_save || matches!(frame_source, Source::File(_)) {
            next_save = elapsed + args.every;
            let detections = detector.detect(&frame).map_err(|e| anyhow!("{e}"))?;
            let lines: Vec<String> = detections
                .iter()
                .filter(|d| d.label.as_ref().is_some_and(|l| classes.contains(l)))
                .filter(|d| roi.is_none_or(|r| r.contains(d.bbox.cx(), d.bbox.cy())))
                .map(|d| {
                    let b = &d.bbox;
                    let class = classes.iter().position(|c| Some(c) == d.label.as_ref()).unwrap_or(0);
                    format!("{class} {:.6} {:.6} {:.6} {:.6}", b.cx(), b.cy(), b.width(), b.height())
                })
                .collect();
            if !lines.is_empty() || args.keep_empty {
                let millis =
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let stem = format!("{millis}-{saved:04}");
                un_media::save_image(&images.join(format!("{stem}.jpg")), &frame).map_err(|e| anyhow!("{e}"))?;
                let mut text = lines.join("\n");
                if !text.is_empty() {
                    text.push('\n');
                }
                std::fs::write(labels.join(format!("{stem}.txt")), text)?;
                saved += 1;
                boxes += lines.len();
                eprintln!("saved {stem}: {} boxes", lines.len());
            }
        }
        if source.frames > 0 && processed >= source.frames {
            break;
        }
    }
    let total = std::fs::read_dir(&images).map(|d| d.count()).unwrap_or(0);
    eprintln!("saved {saved} frames with {boxes} boxes; {} now holds {total} frames", args.out.display());
    Ok(())
}

fn args_noun(vision: &VisionArgs, labels: &[String], sim: bool) -> String {
    if let Some(label) = &vision.count {
        return label.clone();
    }
    if !sim && labels.len() == 1 {
        return labels[0].clone();
    }
    "toy".into()
}
