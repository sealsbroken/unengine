//! The engine drives a whole lesson with scripted speech and a simulated table, without audio or camera.

use std::sync::Arc;
use std::time::{Duration, Instant};

use un_core::strategy::{PhaseKind, SessionConfig};
use un_core::{Box2, Detection, EstimatorConfig, TableStateEstimator};
use un_engine::doubles::{ScriptedSpeechInput, SilentSpeechOutput};
use un_engine::{Engine, EngineOptions, StatusKind};
use un_strategy_demo::DemoStrategy;

fn cubes(count: u32) -> Vec<Detection> {
    (0..count).map(|i| Detection::new(Box2::new(0.1 + i as f32 * 0.1, 0.3, 0.15 + i as f32 * 0.1, 0.35), 0.9)).collect()
}

/// Feeds enough identical frames for the estimator to commit a stable count.
fn settle(engine: &Engine, estimator: &mut TableStateEstimator, count: u32, clock: &mut f64) {
    for _ in 0..40 {
        *clock += 1.0 / 30.0;
        engine.observe(&estimator.ingest(&cubes(count), &[], *clock));
    }
}

fn wait_for(predicate: impl Fn() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn lesson_runs_end_to_end_with_scripted_answers() {
    let config = SessionConfig { max_tasks: 2, seed: 3, nudge_interval: 30.0, ..Default::default() };
    let speech_in = Arc::new(ScriptedSpeechInput::new(vec![3], Duration::from_millis(50)));
    let speech_out = Arc::new(SilentSpeechOutput::new(false));
    let engine = Engine::new(
        Box::new(DemoStrategy::new(config.clone())),
        config,
        Some(speech_in.clone()),
        speech_out.clone(),
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})) },
    );
    let mut estimator = TableStateEstimator::new(EstimatorConfig::default());
    let mut clock = 0.0;

    engine.start();
    assert!(wait_for(|| speech_out.spoken_joined().contains("Put some"), Duration::from_secs(5)));
    settle(&engine, &mut estimator, 3, &mut clock);
    // The tutor asks how many, the scripted child says three, the next round starts.
    assert!(wait_for(|| engine.strategy_snapshot().correct == 1, Duration::from_secs(5)));
    // The celebration is an effect, run on the worker thread a moment after the answer was judged.
    assert!(wait_for(|| engine.snapshot().celebrated_seconds_ago.is_some(), Duration::from_secs(5)));
    assert!(wait_for(|| engine.phase() == PhaseKind::AwaitingObjects, Duration::from_secs(5)));

    // Change the table and answer by number pad.
    settle(&engine, &mut estimator, 5, &mut clock);
    assert!(wait_for(|| engine.phase() == PhaseKind::Asking, Duration::from_secs(5)));
    engine.tap(5);
    assert!(wait_for(|| engine.snapshot().summary.is_some(), Duration::from_secs(5)));
    let snap = engine.snapshot();
    let summary = snap.summary.unwrap();
    assert_eq!((summary.tasks_completed, summary.correct), (2, 2));
    assert_eq!(snap.status.kind, StatusKind::Finished);
    assert!(speech_out.spoken_joined().contains("Well done"));
    engine.close();
}

#[test]
fn missing_microphone_falls_back_to_number_pad() {
    let config = SessionConfig::default();
    let engine = Engine::new(
        Box::new(DemoStrategy::new(config.clone())),
        config,
        Some(Arc::new(ScriptedSpeechInput::broken("no microphone"))),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})) },
    );
    engine.start();
    let snap = engine.snapshot();
    assert!(!snap.speech_available);
    assert!(snap.show_number_pad);
    assert!(snap.warning.unwrap().contains("number pad"));
    engine.close();
}

#[test]
fn nothing_heard_escalates_and_fingers_answer() {
    let config = SessionConfig { answer_timeout: 10.0, seed: 1, ..Default::default() };
    // Two listens time out at once; the third (voice or fingers) waits the full 10 s.
    let timeout = || Err(un_engine::SpeechError::Timeout { last_transcript: None });
    let speech_in = Arc::new(ScriptedSpeechInput::with_script(vec![timeout(), timeout()], Duration::ZERO));
    let engine = Engine::new(
        Box::new(DemoStrategy::new(config.clone())),
        config,
        Some(speech_in),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})) },
    );
    let mut estimator = TableStateEstimator::new(EstimatorConfig::default());
    let mut clock = 0.0;
    engine.start();
    settle(&engine, &mut estimator, 2, &mut clock);
    assert!(wait_for(|| engine.phase() == PhaseKind::Asking, Duration::from_secs(5)));
    // Two timeouts later the engine listens for voice or fingers; hold up two fingers for 1.5 s.
    assert!(wait_for(
        || engine.snapshot().status.listen_mode == Some(un_core::strategy::ListenMode::VoiceOrFingers),
        Duration::from_secs(5)
    ));
    let fingers = Instant::now();
    while fingers.elapsed() < Duration::from_millis(2500) && engine.strategy_snapshot().correct == 0 {
        clock += 1.0 / 30.0;
        let hand = un_core::HandObservation {
            chirality: un_core::Chirality::Right,
            joints: Default::default(),
            joint_confidences: Default::default(),
            raised_finger_count: Some(2),
            bbox: Some(Box2::new(0.4, 0.4, 0.6, 0.7)),
            score: 1.0,
        };
        engine.observe(&estimator.ingest(&cubes(2), &[hand], clock));
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(engine.strategy_snapshot().correct, 1);
    engine.close();
}

/// Records what the engine tells it; answers nothing.
struct Recorder {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    phase: PhaseKind,
}

impl un_core::strategy::Strategy for Recorder {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event, ListenMode};
        self.events.lock().push(event.clone());
        match event {
            Event::Start => {
                self.phase = PhaseKind::Asking;
                vec![Effect::listen(ListenMode::Voice, 10.0)]
            }
            _ => vec![],
        }
    }
    fn snapshot(&self) -> un_core::strategy::StrategySnapshot {
        un_core::strategy::StrategySnapshot {
            phase: self.phase,
            level: 0,
            level_count: 1,
            tasks_completed: 0,
            correct: 0,
            task: None,
            target_count: None,
            expected_answer: None,
        }
    }
}

#[test]
fn words_are_reported_and_listening_goes_on_until_a_number() {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in = Arc::new(ScriptedSpeechInput::saying(
        vec!["Mia".into(), "I don't know".into(), "three caps".into()],
        Duration::from_millis(20),
    ));
    let engine = Engine::new(
        Box::new(Recorder { events: Arc::clone(&events), phase: PhaseKind::Idle }),
        SessionConfig::default(),
        Some(speech_in),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Heard(3)), Duration::from_secs(5)));
    let seen = events.lock().clone();
    let said: Vec<&str> = seen.iter().filter_map(|e| if let Event::Said(t) = e { Some(t.as_str()) } else { None }).collect();
    assert_eq!(said, vec!["Mia", "I don't know"]);
    // One listen served all three lines: the words did not end it.
    assert!(!seen.contains(&Event::NothingHeard));
    engine.close();
}
