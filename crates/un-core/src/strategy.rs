//! The contract between the engine and a lesson strategy.
//!
//! A *strategy* is the methodics: which tasks to pose, how the difficulty moves, what
//! the tutor says. It is a pure reducer, `handle(event) -> effects`, with no access to
//! the camera, microphone or screen. The engine (`un-engine`) turns perception, speech
//! and taps into [`Event`]s and executes the [`Effect`]s in order. Strategies live in
//! separate crates and are registered through [`StrategyFactory`], so proprietary
//! lesson content can be plugged into this LGPL engine without being part of it.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::language::Language;

/// Session-wide settings every strategy receives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub language: Language,
    pub child_name: Option<String>,
    /// How many objects the family owns; bounds "add" tasks.
    pub total_objects: u32,
    /// Session ends after this many tasks.
    pub max_tasks: u32,
    pub start_level: u32,
    /// Listening window (seconds) after a direct question.
    pub answer_timeout: f64,
    /// Listening window while the child rearranges objects.
    pub perform_timeout: f64,
    /// How long to wait before a reminder.
    pub nudge_interval: f64,
    /// Random seed; sessions with the same seed and events are identical.
    pub seed: u64,
    /// Detector label being counted; picks the noun the tutor uses.
    pub noun: String,
    /// Free-form strategy-specific settings.
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            language: Language::English,
            child_name: None,
            total_objects: 10,
            max_tasks: 10,
            start_level: 0,
            answer_timeout: 8.0,
            perform_timeout: 25.0,
            nudge_interval: 10.0,
            seed: 0,
            noun: "toy".to_string(),
            extra: HashMap::new(),
        }
    }
}

/// What the strategy knows about the table at a moment in time. It never sees pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TableObservation {
    pub count: Option<u32>,
    pub hands_present: bool,
    pub is_stable: bool,
    pub finger_count: Option<u32>,
    /// How many separate groups the objects lie in; 0 without objects or when unknown.
    #[serde(default)]
    pub groups: u32,
    /// The stable number of the object a finger is on, `None` when no finger is on one.
    #[serde(default)]
    pub pointed_at: Option<u32>,
}

impl TableObservation {
    /// A settled, hands-free table with `count` objects in one group.
    pub fn stable(count: u32) -> Self {
        Self {
            count: Some(count),
            hands_present: false,
            is_stable: true,
            finger_count: None,
            groups: count.min(1),
            pointed_at: None,
        }
    }

    /// A settled, hands-free table with `count` objects lying in `groups` groups.
    pub fn grouped(count: u32, groups: u32) -> Self {
        Self { groups, ..Self::stable(count) }
    }
}

/// Everything that can happen to a lesson.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Start,
    Stop,
    Table(TableObservation),
    /// A number was heard (voice), shown (fingers) or tapped.
    Heard(u32),
    /// Words were heard that are not a number: a name, "yes", "I don't understand", chatter.
    /// Listening goes on afterwards unless the strategy replaces or stops it.
    Said(String),
    /// Every number in one utterance, in order ("one, two, three"), sent just before the
    /// `Heard` that carries the last of them. In [`ListenMode::Counting`] it is the whole
    /// count, across the child's pauses.
    Numbers(Vec<u32>),
    /// The numbers of one utterance of a count being said aloud ([`ListenMode::Counting`]),
    /// delivered as the child goes, so the tutor can react to each; `Numbers` follows with
    /// the whole sequence when the count ends.
    Counted(Vec<u32>),
    /// Listening ended without a usable number.
    NothingHeard,
    /// The microphone started picking up sound (`true`) or has been quiet for a few seconds
    /// (`false`). Sent when that changes, and once with `false` when a metered microphone is
    /// in use at all; never sent without one.
    Sound(bool),
    TimerFired(u64),
    RepeatRequested,
    SkipRequested,
    /// The language model's answer to the last [`Effect::Chat`]; `None` when there is no model,
    /// it failed, or a newer request replaced it. The strategy decides whether to say it.
    Replied(Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenMode {
    /// A spoken answer: the first number ends the listen.
    Voice,
    /// A spoken answer or fingers held up.
    VoiceOrFingers,
    /// A tap on the number pad.
    NumberPad,
    /// The child counts aloud: every number is collected, across pauses, until the sequence
    /// reaches the snapshot's `expected_answer` or the child has been quiet for a few seconds
    /// after the last number. Delivered as one `Numbers` event, then `Heard` with the last.
    Counting,
}

/// What the engine must do, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Say(String),
    Listen {
        mode: ListenMode,
        timeout: f64,
    },
    StopListening,
    /// Animate counting the objects on screen, one to `count`.
    HighlightCount(u32),
    Celebrate,
    StartTimer {
        token: u64,
        seconds: f64,
    },
    CancelTimer(u64),
    SessionEnded(SessionSummary),
    /// Ask the on-device language model for the tutor's answer to free speech; the engine runs
    /// it on its own thread and sends [`Event::Replied`]. A newer request replaces an older one.
    Chat(ChatPrompt),
}

/// A conversation for the language model: the strategy's master prompt and the turns so far.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatPrompt {
    /// Who the tutor is, what the lesson is doing now and how to answer.
    pub system: String,
    /// Oldest first; the last one is the child's.
    pub turns: Vec<ChatTurn>,
    /// Upper bound on the answer's length, in model tokens.
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    /// `true` for the child's words, `false` for the tutor's.
    pub child: bool,
    pub text: String,
}

impl Effect {
    pub fn listen(mode: ListenMode, timeout: f64) -> Self {
        Effect::Listen { mode, timeout }
    }
}

/// The texts of the `Say` effects, in order.
pub fn spoken(effects: &[Effect]) -> Vec<&str> {
    effects.iter().filter_map(|e| if let Effect::Say(t) = e { Some(t.as_str()) } else { None }).collect()
}

pub fn listens(effects: &[Effect]) -> Vec<ListenMode> {
    effects.iter().filter_map(|e| if let Effect::Listen { mode, .. } = e { Some(*mode) } else { None }).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskRecord {
    /// Human-readable task, e.g. "remove 2".
    pub task: String,
    pub answers: Vec<u32>,
    pub expected: Option<u32>,
    pub correct: bool,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SessionSummary {
    pub tasks_completed: u32,
    pub correct: u32,
    pub final_level: u32,
    pub records: Vec<TaskRecord>,
    /// The lesson stopped before its end (contact with the child was lost) and can be resumed.
    #[serde(default)]
    pub paused: bool,
    /// What the strategy wants back next time in [`SessionConfig::extra`]: the child's name,
    /// where the lesson stopped. An empty value forgets the key.
    #[serde(default)]
    pub remember: HashMap<String, String>,
}

impl SessionSummary {
    /// Applies `remember` to a product's stored `extra` map.
    pub fn apply_remembered(&self, extra: &mut HashMap<String, String>) {
        for (key, value) in &self.remember {
            if value.is_empty() {
                extra.remove(key);
            } else {
                extra.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Where the lesson is, in terms every strategy shares (for the HUD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    Idle,
    /// Talking with the child before the tasks: names, "shall we begin?".
    Meeting,
    /// Waiting for the first objects to appear.
    AwaitingObjects,
    /// Instruction given; the child is rearranging the table.
    Performing,
    /// Question asked; listening for the answer.
    Asking,
    /// Stopped early with the state kept; the next session can pick it up.
    Paused,
    Finished,
}

/// What the strategy is willing to show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategySnapshot {
    pub phase: PhaseKind,
    pub level: u32,
    pub level_count: u32,
    pub tasks_completed: u32,
    pub correct: u32,
    /// The current task, human-readable.
    pub task: Option<String>,
    /// The count the table must reach before the question is asked, when known.
    pub target_count: Option<u32>,
    /// Answer the current question would need, when the table is as expected.
    pub expected_answer: Option<u32>,
}

/// A lesson reducer. Implementations must be deterministic given `SessionConfig::seed`.
pub trait Strategy: Send {
    fn handle(&mut self, event: Event) -> Vec<Effect>;
    fn snapshot(&self) -> StrategySnapshot;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyInfo {
    /// Stable identifier, e.g. `counting`.
    pub id: String,
    pub title: String,
    pub description: String,
    pub languages: Vec<Language>,
    pub license: String,
}

/// Creates strategies; registered with the engine at startup.
pub trait StrategyFactory: Send + Sync {
    fn info(&self) -> StrategyInfo;
    fn create(&self, config: &SessionConfig) -> Box<dyn Strategy>;
}

/// A strategy registered at link time.
///
/// A strategy crate declares itself with
/// `inventory::submit! { un_core::strategy::LinkedStrategy(|| Arc::new(MyFactory)) }`;
/// every binary that links the crate then finds it through [`StrategyRegistry::linked`].
/// Static libraries can drop unreferenced objects, so products also call their strategies'
/// `register` functions explicitly; the registry de-duplicates by id.
pub struct LinkedStrategy(pub fn() -> Arc<dyn StrategyFactory>);

inventory::collect!(LinkedStrategy);

/// The strategies an app knows about.
#[derive(Default, Clone)]
pub struct StrategyRegistry {
    factories: Vec<Arc<dyn StrategyFactory>>,
}

impl StrategyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every strategy linked into this binary (see [`LinkedStrategy`]).
    pub fn linked() -> Self {
        let mut registry = Self::new();
        for entry in inventory::iter::<LinkedStrategy> {
            registry.register((entry.0)());
        }
        registry
    }

    pub fn register(&mut self, factory: Arc<dyn StrategyFactory>) -> &mut Self {
        self.factories.retain(|f| f.info().id != factory.info().id);
        self.factories.push(factory);
        self
    }

    pub fn with(mut self, factory: Arc<dyn StrategyFactory>) -> Self {
        self.register(factory);
        self
    }

    pub fn infos(&self) -> Vec<StrategyInfo> {
        self.factories.iter().map(|f| f.info()).collect()
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn StrategyFactory>> {
        self.factories.iter().find(|f| f.info().id == id).cloned()
    }

    pub fn create(&self, id: &str, config: &SessionConfig) -> Option<Box<dyn Strategy>> {
        self.get(id).map(|f| f.create(config))
    }

    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }
}

/// SplitMix64: small, fast, reproducible, so lesson tests are deterministic. Shared by
/// strategies so a seed means the same thing everywhere.
#[derive(Debug, Clone)]
pub struct SeededGenerator {
    state: u64,
}

impl SeededGenerator {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish integer in `low..=high`.
    pub fn random_int(&mut self, low: u32, high: u32) -> u32 {
        debug_assert!(high >= low);
        low + (self.next_u64() % (high - low + 1) as u64) as u32
    }

    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next_u64() % items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_is_reproducible() {
        let mut a = SeededGenerator::new(1);
        let mut b = SeededGenerator::new(1);
        assert_eq!(a.next_u64(), b.next_u64());
        // Known SplitMix64 first output for seed 1.
        assert_eq!(SeededGenerator::new(1).next_u64(), 0x910A2DEC89025CC1);
        let mut rng = SeededGenerator::new(9);
        for _ in 0..100 {
            let v = rng.random_int(3, 5);
            assert!((3..=5).contains(&v));
        }
    }

    struct Echo;
    impl Strategy for Echo {
        fn handle(&mut self, event: Event) -> Vec<Effect> {
            match event {
                Event::Start => vec![Effect::Say("hi".into())],
                _ => vec![],
            }
        }
        fn snapshot(&self) -> StrategySnapshot {
            StrategySnapshot {
                phase: PhaseKind::Idle,
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
    struct EchoFactory;
    impl StrategyFactory for EchoFactory {
        fn info(&self) -> StrategyInfo {
            StrategyInfo {
                id: "echo".into(),
                title: "Echo".into(),
                description: String::new(),
                languages: vec![Language::English],
                license: "LGPL-3.0".into(),
            }
        }
        fn create(&self, _config: &SessionConfig) -> Box<dyn Strategy> {
            Box::new(Echo)
        }
    }

    #[test]
    fn registry_creates_by_id() {
        let registry = StrategyRegistry::new().with(Arc::new(EchoFactory)).with(Arc::new(EchoFactory));
        assert_eq!(registry.infos().len(), 1);
        let mut s = registry.create("echo", &SessionConfig::default()).expect("registered");
        assert_eq!(spoken(&s.handle(Event::Start)), vec!["hi"]);
        assert!(registry.create("nope", &SessionConfig::default()).is_none());
    }
}
