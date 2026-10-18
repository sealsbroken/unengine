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
        engine.observe(&estimator.ingest(&cubes(count), *clock));
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

/// Greets, then answers any words with a scripted line; `ask` also opens a voice listen after
/// the greeting.
struct Chatty {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    ask: bool,
}

impl un_core::strategy::Strategy for Chatty {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event, ListenMode};
        self.events.lock().push(event.clone());
        match event {
            Event::Start if self.ask => vec![Effect::Say("How many?".into()), Effect::listen(ListenMode::Voice, 5.0)],
            Event::Start => vec![Effect::Say("Hello there.".into())],
            Event::Said(_) => vec![Effect::Say("I like blue.".into())],
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
        EngineOptions { log: Some(Arc::new(|_| {})), always_listen: true, ..Default::default() },
    );
    (engine, events, speech_out)
}

#[test]
fn an_open_microphone_hears_the_child_outside_any_listen() {
    use un_core::strategy::Event;
    let (engine, events, speech_out) = chatty_engine(vec!["what is your favourite colour"], false);
    engine.start();
    // No listen was ever asked for: the always-on microphone heard it anyway.
    assert!(wait_for(|| speech_out.spoken_joined().contains("I like blue."), Duration::from_secs(5)));
    assert!(events.lock().contains(&Event::Said("what is your favourite colour".into())));
    engine.close();
}

#[test]
fn an_answer_window_still_takes_the_number_with_the_microphone_always_open() {
    use un_core::strategy::Event;
    let (engine, events, _) = chatty_engine(vec!["three"], true);
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Heard(3)), Duration::from_secs(5)));
    assert!(!events.lock().iter().any(|e| matches!(e, Event::Said(_))));
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

#[test]
fn a_sentence_that_starts_with_no_is_talk_not_zero() {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_in = Arc::new(ScriptedSpeechInput::saying(
        vec!["нет объясни зачем мне учиться считать".into(), "нет".into()],
        Duration::from_millis(20),
    ));
    let config = SessionConfig { language: un_core::Language::Russian, ..Default::default() };
    let engine = Engine::new(
        Box::new(Recorder::new(&events)),
        config,
        Some(speech_in),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Heard(0)), Duration::from_secs(5)));
    assert!(events.lock().contains(&Event::Said("нет объясни зачем мне учиться считать".into())));
    engine.close();
}

#[test]
fn a_spoken_count_starting_at_zero_reaches_the_strategy_unchanged() {
    use un_core::strategy::Event;
    let seen = counted(&["zero", "one", "two", "three"], 3);
    assert!(seen.contains(&Event::Counted(vec![0])));
    assert!(seen.contains(&Event::Numbers(vec![0, 1, 2, 3])));
    assert_eq!(seen.last(), Some(&Event::Heard(3)));
}

#[test]
fn a_table_observed_before_start_is_replayed_to_the_active_strategy() {
    use un_core::strategy::{Event, TableObservation};
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let engine = Engine::new(
        Box::new(Recorder::new(&events)),
        SessionConfig::default(),
        None,
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { chime: false, log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.observe_table(TableObservation::stable(5));
    engine.start();
    assert!(events.lock().contains(&Event::Table(TableObservation::stable(5))));
    assert_eq!(events.lock().iter().filter(|e| matches!(e, Event::Table(_))).count(), 1);
    engine.observe_table(TableObservation::stable(5));
    assert_eq!(events.lock().iter().filter(|e| matches!(e, Event::Table(_))).count(), 1);
    engine.close();
}

/// Asks once; the answer ends the question.
struct AsksOnce {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    phase: PhaseKind,
}

impl un_core::strategy::Strategy for AsksOnce {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event, ListenMode};
        self.events.lock().push(event.clone());
        match event {
            Event::Start => {
                self.phase = PhaseKind::Asking;
                vec![Effect::listen(ListenMode::NumberPad, 30.0)]
            }
            Event::Heard(_) => {
                self.phase = PhaseKind::Performing;
                vec![Effect::StopListening]
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
fn the_table_is_held_while_the_child_answers_and_follows_after() {
    use un_core::strategy::{Event, TableObservation};
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let engine = Engine::new(
        Box::new(AsksOnce { events: Arc::clone(&events), phase: PhaseKind::Idle }),
        SessionConfig::default(),
        None,
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { chime: false, log: Some(Arc::new(|_| {})), ..Default::default() },
    );
    engine.start();
    let tables = || events.lock().iter().filter(|e| matches!(e, Event::Table(_))).count();
    // A hand pointing at the toys while the child counts: the table moves, the question stays.
    for count in [Some(5), None, Some(4), Some(5)] {
        engine.observe_table(TableObservation { count, is_stable: count == Some(4), groups: 1 });
    }
    assert_eq!(tables(), 0);
    engine.tap(5);
    // The answer is in: the latest table follows.
    let seen: Vec<Event> = events.lock().iter().filter(|e| matches!(e, Event::Table(_))).cloned().collect();
    assert_eq!(seen, vec![Event::Table(TableObservation { count: Some(5), is_stable: false, groups: 1 })]);
    engine.close();
}

/// Models a count, then lets the child count.
struct Models {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
}

impl un_core::strategy::Strategy for Models {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event};
        self.events.lock().push(event.clone());
        match event {
            Event::Start => vec![Effect::Say("One more friend! One, two! Now you.".into())],
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
            expected_answer: Some(2),
        }
    }
}

#[test]
fn the_child_repeating_the_tutors_modelled_count_is_heard_not_taken_for_an_echo() {
    use un_core::strategy::Event;
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let engine = Engine::new(
        Box::new(Models { events: Arc::clone(&events) }),
        SessionConfig::default(),
        Some(Arc::new(ScriptedSpeechInput::saying(vec!["one two".into()], Duration::from_millis(300)).deaf_to_the_tutor())),
        Arc::new(SilentSpeechOutput::new(false)),
        EngineOptions { log: Some(Arc::new(|_| {})), always_listen: true, ..Default::default() },
    );
    engine.start();
    // The child spoke after the line had ended: their count, though the tutor said the same.
    assert!(wait_for(|| events.lock().contains(&Event::Heard(2)), Duration::from_secs(5)));
    engine.close();
}

/// Asks where to go; any answer naming the forest is taken.
struct Choice {
    events: Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>,
    mode: un_core::strategy::ListenMode,
    line: &'static str,
}

impl un_core::strategy::Strategy for Choice {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event};
        self.events.lock().push(event.clone());
        match event {
            Event::Start => vec![Effect::Say(self.line.into()), Effect::listen(self.mode, 10.0)],
            Event::Said(words) if words.contains("forest") => vec![Effect::Say("Off to the forest!".into())],
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
            expected_answer: Some(2),
        }
    }
}

fn barging_engine(
    line: &'static str,
    mode: un_core::strategy::ListenMode,
    said: Vec<&str>,
) -> (Engine, Arc<parking_lot::Mutex<Vec<un_core::strategy::Event>>>, Arc<SilentSpeechOutput>) {
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let speech_out = Arc::new(SilentSpeechOutput::paced(false));
    let engine = Engine::new(
        Box::new(Choice { events: Arc::clone(&events), mode, line }),
        SessionConfig::default(),
        Some(Arc::new(
            ScriptedSpeechInput::saying(said.into_iter().map(String::from).collect(), Duration::from_millis(800)).barging_in(),
        )),
        speech_out.clone(),
        EngineOptions { chime: false, log: Some(Arc::new(|_| {})), always_listen: true, ..Default::default() },
    );
    (engine, events, speech_out)
}

#[test]
fn an_answer_over_the_question_cuts_the_question_short() {
    let line = "Choose our adventure! The forest or the moon? Say forest or moon, and off we go.";
    let (engine, _, speech_out) = barging_engine(line, un_core::strategy::ListenMode::Voice, vec!["forest"]);
    let began = Instant::now();
    engine.start();
    assert!(wait_for(|| speech_out.spoken_joined().contains("Off to the forest!"), Duration::from_secs(5)));
    // The question takes 3.6 s to say; the answer came at 0.8 s and the tutor went on at once.
    assert!(began.elapsed() < Duration::from_millis(2500), "{:?}", began.elapsed());
    engine.close();
}

#[test]
fn a_count_begun_over_the_tutors_line_is_counted_in_full() {
    use un_core::strategy::Event;
    let line = "One, two! Now it is your turn to count them.";
    let (engine, events, speech_out) = barging_engine(line, un_core::strategy::ListenMode::Counting, vec!["one", "two"]);
    engine.start();
    assert!(wait_for(|| events.lock().contains(&Event::Numbers(vec![1, 2])), Duration::from_secs(8)));
    // Numbers do not cut the line: the count goes on into the listen after it.
    assert_eq!(speech_out.spoken_texts(), vec![line.to_string()]);
    engine.close();
}

/// Says one line and ends, completed or paused for a rest.
struct EndsAtOnce {
    paused: bool,
    done: bool,
}

impl un_core::strategy::Strategy for EndsAtOnce {
    fn handle(&mut self, event: un_core::strategy::Event) -> Vec<un_core::strategy::Effect> {
        use un_core::strategy::{Effect, Event, SessionSummary};
        match event {
            Event::Start => {
                self.done = true;
                vec![
                    Effect::Say("Well done!".into()),
                    Effect::SessionEnded(SessionSummary { paused: self.paused, ..Default::default() }),
                ]
            }
            _ => vec![],
        }
    }
    fn snapshot(&self) -> un_core::strategy::StrategySnapshot {
        un_core::strategy::StrategySnapshot {
            phase: if self.done { PhaseKind::Finished } else { PhaseKind::Idle },
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

fn ending_engine(paused: bool, said: &str) -> (Engine, Arc<SilentSpeechOutput>) {
    let speech_out = Arc::new(SilentSpeechOutput::new(false));
    let engine = Engine::new(
        Box::new(EndsAtOnce { paused, done: false }),
        SessionConfig::default(),
        Some(Arc::new(ScriptedSpeechInput::saying(vec![said.into()], Duration::from_millis(400)))),
        speech_out.clone(),
        EngineOptions { chime: false, log: Some(Arc::new(|_| {})), always_listen: true, ..Default::default() },
    );
    engine.offer_next_lesson(true);
    (engine, speech_out)
}

#[test]
fn a_yes_to_shall_we_go_on_asks_for_the_next_lesson() {
    let (engine, speech_out) = ending_engine(false, "yes");
    engine.start();
    assert!(wait_for(|| engine.snapshot().next_lesson_requested, Duration::from_secs(5)));
    assert!(speech_out.spoken_texts().contains(&"Shall we go on?".to_string()));
    engine.close();
}

#[test]
fn a_no_to_shall_we_go_on_says_goodbye() {
    let (engine, speech_out) = ending_engine(false, "no, not now");
    engine.start();
    assert!(wait_for(|| speech_out.spoken_joined().contains("See you next time"), Duration::from_secs(5)));
    assert!(!engine.snapshot().next_lesson_requested);
    engine.close();
}

#[test]
fn a_lesson_paused_for_a_rest_does_not_offer_the_next_one() {
    let (engine, speech_out) = ending_engine(true, "yes");
    engine.start();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!speech_out.spoken_joined().contains("Shall we go on?"));
    assert!(!engine.snapshot().next_lesson_requested);
    engine.close();
}

#[test]
fn saying_enough_pauses_the_lesson_over_the_tutor() {
    use un_core::strategy::Event;
    let line = "Choose our adventure! The forest or the moon? Say forest or moon, and off we go.";
    let (engine, events, _) = barging_engine(line, un_core::strategy::ListenMode::Voice, vec!["enough"]);
    engine.start();
    assert!(wait_for(|| engine.snapshot().pause_requested, Duration::from_secs(5)));
    assert!(wait_for(|| events.lock().contains(&Event::Stop), Duration::from_secs(2)));
    engine.close();
}
