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
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})), ..Default::default() },
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
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})), ..Default::default() },
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
        EngineOptions { chime: false, always_show_number_pad: false, log: Some(Arc::new(|_| {})), ..Default::default() },
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

/// Records what the engine tells it; answers nothing. Listens once, in `mode`, on start.
struct Recorder {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    phase: PhaseKind,
    mode: un_core::strategy::ListenMode,
    expected_answer: Option<u32>,
    timeout: f64,
}

impl Recorder {
    fn new(events: &Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>) -> Self {
        Self {
            events: Arc::clone(events),
            phase: PhaseKind::Idle,
            mode: un_core::strategy::ListenMode::Voice,
            expected_answer: None,
            timeout: 10.0,
        }
    }

    fn counting_to(self, count: u32) -> Self {
        Self { mode: un_core::strategy::ListenMode::Counting, expected_answer: Some(count), ..self }
    }
}

impl un_core::strategy::Strategy for Recorder {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event};
        self.events.lock().push(event.clone());
        match event {
            Event::Start => {
                self.phase = PhaseKind::Asking;
                vec![Effect::listen(self.mode, self.timeout)]
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
            expected_answer: self.expected_answer,
        }
    }
}

#[test]
fn words_are_reported_and_listening_goes_on_until_a_number() {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in = Arc::new(ScriptedSpeechInput::saying(
        vec!["Mia".into(), "I don't know".into(), "three cars".into()],
        Duration::from_millis(20),
    ));
    let engine = Engine::new(
        Box::new(Recorder::new(&events)),
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

/// Runs a counting listen over `lines`, one utterance each, and returns the events.
fn counted(lines: &[&str], expected: u32) -> Vec<un_core::strategy::Event> {
    counted_with(lines, expected, 10.0, Duration::from_millis(20))
}

/// Like [`counted`], with the listen's `timeout` and the child's `delay` before each line.
fn counted_with(lines: &[&str], expected: u32, timeout: f64, delay: Duration) -> Vec<un_core::strategy::Event> {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in = Arc::new(ScriptedSpeechInput::saying(lines.iter().map(|l| l.to_string()).collect(), delay));
    let mut recorder = Recorder::new(&events).counting_to(expected);
    recorder.timeout = timeout;
    let engine = Engine::new(
        Box::new(recorder),
        SessionConfig::default(),
        Some(speech_in),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.start();
    let done = |e: &Event| matches!(e, Event::Heard(_) | Event::NothingHeard);
    assert!(wait_for(|| events.lock().iter().any(done), Duration::from_secs(8)));
    engine.close();
    let seen = events.lock().clone();
    seen.into_iter().filter(|e| !matches!(e, Event::Start | Event::Stop)).collect()
}

#[test]
fn counting_aloud_is_collected_across_pauses_until_the_expected_number() {
    use un_core::strategy::Event;
    // "One ... two ... three": three utterances, one listen, ended by the expected three.
    let started = Instant::now();
    let seen = counted(&["one", "two", "three"], 3);
    assert_eq!(
        seen,
        vec![
            Event::Counted(vec![1]),
            Event::Counted(vec![2]),
            Event::Counted(vec![3]),
            Event::Numbers(vec![1, 2, 3]),
            Event::Heard(3)
        ]
    );
    let took = started.elapsed();
    assert!(took >= un_engine::COUNTING_DONE_GAP && took < un_engine::COUNTING_GAP, "a beat after the expected number: {took:?}");
    // Words between the numbers are reported and do not break the sequence.
    let seen = counted(&["one", "um", "two"], 2);
    assert_eq!(
        seen,
        vec![
            Event::Counted(vec![1]),
            Event::Said("um".into()),
            Event::Counted(vec![2]),
            Event::Numbers(vec![1, 2]),
            Event::Heard(2)
        ]
    );
}

#[test]
fn counting_on_past_the_expected_number_is_heard_in_full() {
    use un_core::strategy::Event;
    let seen = counted(&["one", "two", "three"], 2);
    assert_eq!(seen.last(), Some(&Event::Heard(3)));
    assert!(seen.contains(&Event::Numbers(vec![1, 2, 3])));
}

#[test]
fn a_slow_count_of_any_length_is_not_cut_by_the_listen_timeout() {
    use un_core::strategy::Event;
    // Half a second between numbers, a listen timeout of 0.7 s: the timeout is per number, so
    // the whole count is collected however long it is.
    for count in [2u32, 5, 7] {
        let words: Vec<String> = (1..=count).map(|n| n.to_string()).collect();
        let lines: Vec<&str> = words.iter().map(String::as_str).collect();
        let seen = counted_with(&lines, count, 0.7, Duration::from_millis(500));
        assert!(seen.contains(&Event::Numbers((1..=count).collect())), "{count}: {seen:?}");
    }
}

#[test]
fn counting_that_stops_short_is_delivered_after_the_gap() {
    use un_core::strategy::Event;
    let started = Instant::now();
    let seen = counted(&["one", "two"], 4);
    assert_eq!(seen, vec![Event::Counted(vec![1]), Event::Counted(vec![2]), Event::Numbers(vec![1, 2]), Event::Heard(2)]);
    assert!(started.elapsed() >= un_engine::COUNTING_GAP);
}

/// Greets, then: words become a chat request, the model's answer is said; `ask` also opens a
/// voice listen after the greeting.
struct Chatty {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    ask: bool,
}

impl un_core::strategy::Strategy for Chatty {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{ChatPrompt, ChatTurn, Effect, Event, ListenMode};
        self.events.lock().push(event.clone());
        match event {
            Event::Start if self.ask => vec![Effect::Say("How many?".into()), Effect::listen(ListenMode::Voice, 5.0)],
            Event::Start => vec![Effect::Say("Hello there.".into())],
            Event::Said(text) => vec![Effect::Chat(ChatPrompt {
                system: "You are Amber.".into(),
                turns: vec![ChatTurn { child: true, text }],
                max_tokens: 40,
            })],
            Event::Replied(Some(text)) => vec![Effect::Say(text)],
            _ => vec![],
        }
    }

    fn snapshot(&self) -> un_core::strategy::StrategySnapshot {
        un_core::strategy::StrategySnapshot {
            phase: PhaseKind::Asking,
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

fn chatty_engine(
    lines: Vec<&str>,
    ask: bool,
    chat: Option<Arc<un_engine::doubles::ScriptedChat>>,
) -> (Engine, Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>, Arc<SilentSpeechOutput>) {
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in =
        Arc::new(ScriptedSpeechInput::saying(lines.into_iter().map(String::from).collect(), Duration::from_millis(300)));
    let speech_out = Arc::new(SilentSpeechOutput::new(false));
    let engine = Engine::new(
        Box::new(Chatty { events: Arc::clone(&events), ask }),
        SessionConfig::default(),
        Some(speech_in),
        speech_out.clone(),
        EngineOptions {
            log: Some(Arc::new(|_| {})),
            always_listen: true,
            chat: chat.map(|c| c as Arc<dyn un_engine::ChatModel>),
            ..Default::default()
        },
    );
    (engine, events, speech_out)
}

#[test]
fn an_open_microphone_hears_the_child_outside_any_listen_and_the_model_answers() {
    use un_core::strategy::Event;
    let chat = Arc::new(un_engine::doubles::ScriptedChat::new("I like blue.", Duration::from_millis(50)));
    let (engine, events, speech_out) = chatty_engine(vec!["what is your favourite colour"], false, Some(Arc::clone(&chat)));
    engine.start();
    // No listen was ever asked for: the always-on microphone heard it anyway.
    assert!(wait_for(|| speech_out.spoken_joined().contains("I like blue."), Duration::from_secs(5)));
    assert!(events.lock().contains(&Event::Said("what is your favourite colour".into())));
    assert_eq!(chat.prompts.lock()[0].turns[0].text, "what is your favourite colour");
    engine.close();
}

#[test]
fn an_answer_window_still_takes_the_number_with_the_microphone_always_open() {
    use un_core::strategy::Event;
    let (engine, events, _) = chatty_engine(vec!["three"], true, None);
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Heard(3)), Duration::from_secs(5)));
    assert!(!events.lock().iter().any(|e| matches!(e, Event::Said(_))));
    engine.close();
}

#[test]
fn without_a_model_a_chat_request_gets_no_answer() {
    use un_core::strategy::Event;
    let (engine, events, speech_out) = chatty_engine(vec!["tell me a story"], false, None);
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Replied(None)), Duration::from_secs(5)));
    assert_eq!(speech_out.spoken_texts(), vec!["Hello there."]);
    engine.close();
}

#[test]
fn a_no_in_the_middle_of_a_count_is_a_word_not_zero() {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in = Arc::new(ScriptedSpeechInput::saying(vec!["нет".into(), "один два".into()], Duration::from_millis(20)));
    let config = SessionConfig { language: un_core::Language::Russian, ..Default::default() };
    let engine = Engine::new(
        Box::new(Recorder::new(&events).counting_to(2)),
        config,
        Some(speech_in),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Numbers(vec![1, 2])), Duration::from_secs(10)));
    assert!(events.lock().contains(&Event::Said("нет".into())));
    assert!(!events.lock().iter().any(|e| matches!(e, Event::Counted(n) if n.contains(&0))));
    engine.close();
}
