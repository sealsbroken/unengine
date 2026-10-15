//! A minimal strategy: "How many are on the table?", repeated.
//!
//! It exists so the engine, the apps and the CI build and run without any
//! proprietary lesson content, and as the smallest example of a [`Strategy`].
//! Real methodics live in separate crates registered the same way.

use std::sync::Arc;

use un_core::language::{Language, Noun, Numerals};
use un_core::strategy::{
    Effect, Event, ListenMode, PhaseKind, SessionConfig, SessionSummary, Strategy, StrategyFactory, StrategyInfo,
    StrategyRegistry, StrategySnapshot, TableObservation, TaskRecord,
};

pub const ID: &str = "demo";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Waiting,
    Asking { count: u32 },
    Finished,
}

pub struct DemoStrategy {
    config: SessionConfig,
    noun: Noun,
    phase: Phase,
    records: Vec<TaskRecord>,
    misses: u32,
    last_prompt: Option<String>,
    timer: u64,
}

impl DemoStrategy {
    pub fn new(config: SessionConfig) -> Self {
        let noun = Noun::for_label(&config.noun);
        Self { config, noun, phase: Phase::Idle, records: Vec::new(), misses: 0, last_prompt: None, timer: 0 }
    }

    fn language(&self) -> Language {
        self.config.language
    }

    fn say(&mut self, text: String) -> Effect {
        self.last_prompt = Some(text.clone());
        Effect::Say(text)
    }

    fn ask(&mut self, count: u32) -> Vec<Effect> {
        self.phase = Phase::Asking { count };
        self.misses = 0;
        let text = match self.language() {
            Language::English => format!("How many {} are on the table?", self.noun.en_many),
            Language::Russian => format!("Сколько {} на столе?", self.noun.ru_many),
        };
        vec![self.say(text), Effect::listen(ListenMode::Voice, self.config.answer_timeout)]
    }

    fn wait(&mut self) -> Vec<Effect> {
        self.phase = Phase::Waiting;
        let text = match self.language() {
            Language::English => format!("Change the {} on the table.", self.noun.en_many),
            Language::Russian => format!("Поменяй {} на столе.", self.noun.ru_plural),
        };
        self.timer += 1;
        vec![self.say(text), Effect::StartTimer { token: self.timer, seconds: self.config.nudge_interval }]
    }

    fn finish(&mut self) -> Vec<Effect> {
        let active = !matches!(self.phase, Phase::Idle | Phase::Finished);
        self.phase = Phase::Finished;
        let correct = self.records.iter().filter(|r| r.correct).count() as u32;
        let summary = SessionSummary {
            tasks_completed: self.records.len() as u32,
            correct,
            final_level: 0,
            records: self.records.clone(),
            ..Default::default()
        };
        let mut effects = vec![Effect::StopListening, Effect::CancelTimer(self.timer)];
        if active {
            let text = match self.language() {
                Language::English => "Well done! See you next time.".to_string(),
                Language::Russian => "Молодец! До встречи.".to_string(),
            };
            effects.push(self.say(text));
        }
        effects.push(Effect::SessionEnded(summary));
        effects
    }
}

impl Strategy for DemoStrategy {
    fn handle(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Start => {
                self.phase = Phase::Waiting;
                let text = match self.language() {
                    Language::English => format!("Hi! Put some {} on the table.", self.noun.en_many),
                    Language::Russian => format!("Привет! Положи {} на стол.", self.noun.ru_plural),
                };
                vec![self.say(text)]
            }
            Event::Stop => self.finish(),
            Event::Table(TableObservation { count: Some(count), is_stable: true, hands_present: false, .. }) => {
                match self.phase {
                    Phase::Waiting if count > 0 => {
                        let mut effects = vec![Effect::CancelTimer(self.timer)];
                        effects.extend(self.ask(count));
                        effects
                    }
                    Phase::Asking { count: asked } if count != asked => {
                        self.phase = Phase::Asking { count };
                        vec![]
                    }
                    _ => vec![],
                }
            }
            Event::Table(_) => vec![],
            Event::Heard(answer) => {
                let Phase::Asking { count } = self.phase else { return vec![] };
                let correct = answer == count;
                self.records.push(TaskRecord {
                    task: "count all".into(),
                    answers: vec![answer],
                    expected: Some(count),
                    correct,
                    skipped: false,
                });
                let objs = Numerals::objects(count, self.language(), &self.noun);
                let text = match (self.language(), correct) {
                    (Language::English, true) => format!("Yes! {objs}."),
                    (Language::English, false) => format!("Not quite. There are {objs}."),
                    (Language::Russian, true) => format!("Да! {objs}."),
                    (Language::Russian, false) => format!("Не совсем. Тут {objs}."),
                };
                let mut effects = if correct { vec![Effect::Celebrate] } else { vec![Effect::HighlightCount(count)] };
                effects.push(self.say(text));
                if self.records.len() as u32 >= self.config.max_tasks {
                    effects.extend(self.finish());
                } else {
                    effects.extend(self.wait());
                }
                effects
            }
            Event::NothingHeard => {
                if !matches!(self.phase, Phase::Asking { .. }) {
                    return vec![];
                }
                self.misses += 1;
                let mode = match self.misses {
                    1 => ListenMode::Voice,
                    2 => ListenMode::VoiceOrFingers,
                    _ => ListenMode::NumberPad,
                };
                let text = match self.language() {
                    Language::English => "I didn't hear you. Say the number, show it with your fingers, or tap it.".to_string(),
                    Language::Russian => "Мне не слышно. Скажи число, покажи на пальцах или нажми его.".to_string(),
                };
                vec![self.say(text), Effect::listen(mode, self.config.answer_timeout)]
            }
            Event::TimerFired(token) if token == self.timer && self.phase == Phase::Waiting => {
                let text = match self.language() {
                    Language::English => "I'm waiting!".to_string(),
                    Language::Russian => "Я жду!".to_string(),
                };
                self.timer += 1;
                vec![self.say(text), Effect::StartTimer { token: self.timer, seconds: self.config.nudge_interval }]
            }
            Event::TimerFired(_) | Event::Said(_) | Event::Numbers(_) | Event::Counted(_) | Event::Sound(_) => vec![],
            Event::RepeatRequested => {
                let mut effects = vec![];
                if let Some(prompt) = self.last_prompt.clone() {
                    effects.push(Effect::Say(prompt));
                }
                if matches!(self.phase, Phase::Asking { .. }) {
                    effects.push(Effect::listen(ListenMode::Voice, self.config.answer_timeout));
                }
                effects
            }
            Event::SkipRequested => {
                if !matches!(self.phase, Phase::Asking { .. }) {
                    return vec![];
                }
                self.records.push(TaskRecord {
                    task: "count all".into(),
                    answers: vec![],
                    expected: None,
                    correct: false,
                    skipped: true,
                });
                let mut effects = vec![Effect::StopListening];
                effects.extend(self.wait());
                effects
            }
        }
    }

    fn snapshot(&self) -> StrategySnapshot {
        let (phase, target) = match self.phase {
            Phase::Idle => (PhaseKind::Idle, None),
            Phase::Waiting => (PhaseKind::AwaitingObjects, None),
            Phase::Asking { count } => (PhaseKind::Asking, Some(count)),
            Phase::Finished => (PhaseKind::Finished, None),
        };
        StrategySnapshot {
            phase,
            level: 0,
            level_count: 1,
            tasks_completed: self.records.len() as u32,
            correct: self.records.iter().filter(|r| r.correct).count() as u32,
            task: matches!(self.phase, Phase::Asking { .. }).then(|| "count all".to_string()),
            target_count: target,
            expected_answer: target,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DemoFactory;

impl StrategyFactory for DemoFactory {
    fn info(&self) -> StrategyInfo {
        StrategyInfo {
            id: ID.to_string(),
            title: "Demo: how many?".to_string(),
            description: "Asks how many objects are on the table and checks the answer. The smallest strategy.".to_string(),
            languages: vec![Language::English, Language::Russian],
            license: "LGPL-3.0-or-later".to_string(),
        }
    }

    fn create(&self, config: &SessionConfig) -> Box<dyn Strategy> {
        Box::new(DemoStrategy::new(config.clone()))
    }
}

pub fn register(registry: &mut StrategyRegistry) {
    registry.register(Arc::new(DemoFactory));
}

inventory::submit! { un_core::strategy::LinkedStrategy(|| Arc::new(DemoFactory)) }

#[cfg(test)]
mod tests {
    use un_core::strategy::{listens, spoken};

    use super::*;

    #[test]
    fn asks_checks_and_finishes() {
        let mut s = DemoStrategy::new(SessionConfig { max_tasks: 1, ..Default::default() });
        assert_eq!(spoken(&s.handle(Event::Start)), vec!["Hi! Put some cubes on the table."]);
        let asked = s.handle(Event::Table(TableObservation::stable(3)));
        assert_eq!(spoken(&asked), vec!["How many cubes are on the table?"]);
        assert_eq!(listens(&asked), vec![ListenMode::Voice]);
        let answered = s.handle(Event::Heard(3));
        assert!(answered.contains(&Effect::Celebrate));
        assert!(answered.iter().any(|e| matches!(e, Effect::SessionEnded(s) if s.correct == 1)));
        assert_eq!(s.snapshot().phase, PhaseKind::Finished);
    }

    #[test]
    fn is_linked() {
        assert!(StrategyRegistry::linked().get(ID).is_some());
    }

    #[test]
    fn escalates_when_nothing_is_heard() {
        let mut s = DemoStrategy::new(SessionConfig::default());
        s.handle(Event::Start);
        s.handle(Event::Table(TableObservation::stable(2)));
        assert_eq!(listens(&s.handle(Event::NothingHeard)), vec![ListenMode::Voice]);
        assert_eq!(listens(&s.handle(Event::NothingHeard)), vec![ListenMode::VoiceOrFingers]);
        assert_eq!(listens(&s.handle(Event::NothingHeard)), vec![ListenMode::NumberPad]);
    }
}
