//! Turning transcripts into numbers and count questions.
//!
//! The vocabulary tables deliberately include common misrecognitions and children's
//! pronunciation slips so the parser is forgiving. The *last* number in a transcript
//! wins: a child who says "four… no, five" means five.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::language::{Language, Numerals};

const ENGLISH_EXTRA: &[(&str, u32)] = &[
    ("zero", 0),
    ("none", 0),
    ("nothing", 0),
    ("no cubes", 0),
    ("no cube", 0),
    ("not any", 0),
    ("nope", 0),
    // Misrecognitions and kid slips.
    ("won", 1),
    ("wan", 1),
    ("to", 2),
    ("too", 2),
    ("tree", 3),
    ("free", 3),
    ("fee", 3),
    ("for", 4),
    ("fore", 4),
    ("fife", 5),
    ("fine", 5),
    ("sex", 6),
    ("sick", 6),
    ("sicks", 6),
    ("sevin", 7),
    ("ate", 8),
    ("eat", 8),
    ("nein", 9),
    ("tan", 10),
    ("a leven", 11),
    ("a lemon", 11),
    ("thirty", 13),
    ("forty", 14),
    ("sixty", 16),
    ("seventy", 17),
    ("eighty", 18),
    ("ninety", 19),
    ("twenny", 20),
];

const RUSSIAN_EXTRA: &[(&str, u32)] = &[
    ("ноль", 0),
    ("нуль", 0),
    ("нет", 0),
    ("ничего", 0),
    ("ни одного", 0),
    ("ни одной", 0),
    ("нисколько", 0),
    ("одна", 1),
    ("одно", 1),
    ("две", 2),
    // Misrecognitions and kid slips.
    ("адин", 1),
    ("адна", 1),
    ("дво", 2),
    ("тры", 3),
    ("четыри", 4),
    ("читыре", 4),
    ("читыри", 4),
    ("пядь", 5),
    ("пяць", 5),
    ("шесь", 6),
    ("шест", 6),
    ("сем", 7),
    ("восим", 8),
    ("восем", 8),
    ("девить", 9),
    ("девядь", 9),
    ("десить", 10),
    ("десеть", 10),
    ("одинадцать", 11),
    ("одинацать", 11),
    ("двенацать", 12),
    ("тринацать", 13),
    ("четырнацать", 14),
    ("пятнацать", 15),
    ("шестнацать", 16),
    ("семнацать", 17),
    ("восемнацать", 18),
    ("девятнацать", 19),
    ("двацать", 20),
];

/// Lowercases, folds "ё" to "е", and splits on anything that is not a letter or digit.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in text.chars().flat_map(|c| c.to_lowercase()) {
        let ch = if ch == 'ё' { 'е' } else { ch };
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

struct Table {
    lookup: HashMap<String, u32>,
    max_words: usize,
}

impl Table {
    fn build(entries: &[(String, u32)]) -> Table {
        let mut lookup = HashMap::new();
        let mut max_words = 1;
        for (phrase, value) in entries {
            let words = tokenize(phrase);
            if !words.is_empty() {
                max_words = max_words.max(words.len());
                lookup.insert(words.join(" "), *value);
            }
        }
        Table { lookup, max_words }
    }
}

/// Table-driven vocabulary of number words (0...20) per language.
pub struct NumberWords;

impl NumberWords {
    /// Every (phrase, value) pair, canonical words first.
    pub fn entries(language: Language) -> Vec<(String, u32)> {
        let (canonical, extra): (&[&str], &[(&str, u32)]) = match language {
            Language::English => (&Numerals::ENGLISH, ENGLISH_EXTRA),
            Language::Russian => (&Numerals::RUSSIAN, RUSSIAN_EXTRA),
        };
        let mut entries: Vec<(String, u32)> = Vec::new();
        for (phrase, value) in extra.iter().filter(|(_, v)| *v == 0) {
            entries.push((phrase.to_string(), *value));
        }
        for (n, word) in canonical.iter().enumerate().skip(1) {
            entries.push((word.to_string(), n as u32));
        }
        for (phrase, value) in extra.iter().filter(|(_, v)| *v != 0) {
            entries.push((phrase.to_string(), *value));
        }
        entries
    }

    fn table(language: Language) -> &'static Table {
        static EN: OnceLock<Table> = OnceLock::new();
        static RU: OnceLock<Table> = OnceLock::new();
        match language {
            Language::English => EN.get_or_init(|| Table::build(&Self::entries(Language::English))),
            Language::Russian => RU.get_or_init(|| Table::build(&Self::entries(Language::Russian))),
        }
    }

    /// Every phrase the parser recognises, without duplicates (for recogniser biasing).
    pub fn words(language: Language) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        Self::entries(language).into_iter().map(|(p, _)| p).filter(|p| seen.insert(p.clone())).collect()
    }

    /// The canonical spoken form of `n` (0...20); digits outside that range.
    pub fn spoken(n: u32, language: Language) -> String {
        Numerals::word(n, language)
    }
}

/// Extracts the number (0...20) a child meant from a speech transcript.
pub struct NumberParser;

impl NumberParser {
    pub const MAX: u32 = 20;

    /// The *last* number mentioned in `transcript`.
    pub fn parse(transcript: &str, language: Language) -> Option<u32> {
        Self::all(transcript, language).last().copied()
    }

    /// Every number mentioned in `transcript`, in order: "one, two, three" is `[1, 2, 3]`.
    pub fn all(transcript: &str, language: Language) -> Vec<u32> {
        let tokens = tokenize(transcript);
        let mut found = Vec::new();
        if tokens.is_empty() {
            return found;
        }
        let table = NumberWords::table(language);
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            if token.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(value) = token.parse::<u32>() {
                    if value <= Self::MAX {
                        found.push(value);
                        i += 1;
                        continue;
                    }
                }
            }
            let mut matched = false;
            let longest = table.max_words.min(tokens.len() - i);
            for length in (1..=longest).rev() {
                let key = tokens[i..i + length].join(" ");
                if let Some(value) = table.lookup.get(&key) {
                    found.push(*value);
                    i += length;
                    matched = true;
                    break;
                }
            }
            if !matched {
                i += 1;
            }
        }
        found
    }
}

/// A spoken "how many …?" question about one kind of toy (`label`), or about all of them (`None`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountQuestion {
    pub label: Option<String>,
}

impl CountQuestion {
    pub const EVERYTHING_WORDS: [&'static str; 11] =
        ["object", "objects", "thing", "things", "item", "items", "toy", "toys", "altogether", "total", "all"];

    /// Words that name each detector label, singular and plural, including common mis-hearings.
    pub fn default_vocabulary() -> BTreeMap<String, Vec<String>> {
        let entries: [(&str, &[&str]); 6] = [
            ("cube", &["cube", "cubes", "block", "blocks", "brick", "bricks", "cue", "cues", "queue", "queues"]),
            ("car", &["car", "cars", "truck", "trucks", "bus", "buses"]),
            ("ball", &["ball", "balls"]),
            ("bear", &["bear", "bears", "teddy", "teddies", "teddy bear", "teddy bears"]),
            ("doll", &["doll", "dolls"]),
            ("animal", &["animal", "animals", "dinosaur", "dinosaurs", "dino", "dinos"]),
        ];
        entries.iter().map(|(k, v)| (k.to_string(), v.iter().map(|s| s.to_string()).collect())).collect()
    }

    pub fn everything() -> Self {
        Self { label: None }
    }

    pub fn about(label: &str) -> Self {
        Self { label: Some(label.to_string()) }
    }

    /// Finds "how many <toys>" or "count <toys>" in `transcript`. English only for now.
    pub fn parse(transcript: &str, language: Language, vocabulary: Option<&BTreeMap<String, Vec<String>>>) -> Option<Self> {
        if language != Language::English {
            return None;
        }
        let default = Self::default_vocabulary();
        let vocabulary = vocabulary.unwrap_or(&default);
        let words: Vec<String> = tokenize(transcript).into_iter().filter(|w| w.chars().any(|c| c.is_alphabetic())).collect();
        let triggers: [&[&str]; 2] = [&["how", "many"], &["count"]];
        let start = triggers.iter().filter_map(|t| after(&words, t)).min()?;
        let tail = &words[start..];
        for index in 0..tail.len() {
            if Self::EVERYTHING_WORDS.contains(&tail[index].as_str()) {
                return Some(Self::everything());
            }
            for (label, names) in vocabulary {
                let mut names: Vec<&String> = names.iter().collect();
                names.sort_by_key(|n| std::cmp::Reverse(n.len()));
                for name in names {
                    let parts: Vec<&str> = name.split_whitespace().collect();
                    if tail.len() >= index + parts.len()
                        && tail[index..index + parts.len()].iter().zip(&parts).all(|(a, b)| a == b)
                    {
                        return Some(Self::about(label));
                    }
                }
            }
        }
        None
    }

    /// Phrases worth biasing a recogniser towards.
    pub fn contextual_strings(vocabulary: Option<&BTreeMap<String, Vec<String>>>) -> Vec<String> {
        let default = Self::default_vocabulary();
        let vocabulary = vocabulary.unwrap_or(&default);
        let mut out: Vec<String> = vec!["how many".into(), "count".into()];
        out.extend(Self::EVERYTHING_WORDS.iter().map(|s| s.to_string()));
        out.extend(vocabulary.values().flatten().cloned());
        out
    }

    /// The spoken answer given the count of each label on the table (hands are not toys). The catch-all
    /// `toy` label is "other toys" next to named kinds.
    pub fn answer(&self, counts: &BTreeMap<String, u32>) -> String {
        if let Some(label) = &self.label {
            return sentence(counts.get(label).copied().unwrap_or(0), label);
        }
        let mut present: Vec<(u32, &String)> = counts.iter().filter(|(_, v)| **v > 0).map(|(k, v)| (*v, k)).collect();
        present.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
        let total: u32 = present.iter().map(|(v, _)| v).sum();
        match present.len() {
            0 => "There are no toys.".to_string(),
            1 => sentence(present[0].0, present[0].1),
            _ => {
                let parts: Vec<String> = present
                    .iter()
                    .map(
                        |(v, k)| {
                            if k.as_str() == "toy" {
                                phrase(*v, k).replacen(" toy", " other toy", 1)
                            } else {
                                phrase(*v, k)
                            }
                        },
                    )
                    .collect();
                let (last, rest) = parts.split_last().unwrap();
                format!("There are {total} toys: {} and {last}.", rest.join(", "))
            }
        }
    }
}

fn after(words: &[String], trigger: &[&str]) -> Option<usize> {
    let n = trigger.len();
    (0..words.len().saturating_sub(n - 1))
        .find(|&i| i + n <= words.len() && words[i..i + n].iter().zip(trigger).all(|(a, b)| a == b))
        .map(|i| i + n)
}

fn sentence(count: u32, label: &str) -> String {
    match count {
        0 => format!("There are no {label}s."),
        1 => format!("There is one {label}."),
        n => format!("There are {n} {label}s."),
    }
}

fn phrase(count: u32, label: &str) -> String {
    if count == 1 {
        format!("one {label}")
    } else {
        format!("{count} {label}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Language::{English as EN, Russian as RU};

    fn en(t: &str) -> Option<u32> {
        NumberParser::parse(t, EN)
    }
    fn ru(t: &str) -> Option<u32> {
        NumberParser::parse(t, RU)
    }

    #[test]
    fn english_digits() {
        for (t, e) in [("4", 4), ("14", 14), ("15", 15), ("0", 0), ("20", 20), ("I see 7 cubes", 7)] {
            assert_eq!(en(t), Some(e), "{t}");
        }
    }

    #[test]
    fn canonical_words_round_trip() {
        for n in 0..=20 {
            assert_eq!(en(&NumberWords::spoken(n, EN)), Some(n));
            assert_eq!(ru(&NumberWords::spoken(n, RU)), Some(n));
        }
    }

    #[test]
    fn english_sentences() {
        for (t, e) in [
            ("there are four cubes", 4),
            ("There are FOUR cubes!", 4),
            ("um five", 5),
            ("I think it's twelve", 12),
            ("Seventeen.", 17),
            ("twenty cubes", 20),
            ("FOUR", 4),
        ] {
            assert_eq!(en(t), Some(e), "{t}");
        }
    }

    #[test]
    fn last_number_wins() {
        assert_eq!(en("four… no, five"), Some(5));
        assert_eq!(en("four no five"), Some(5));
        assert_eq!(en("one two three"), Some(3));
        assert_eq!(en("3 or maybe 4"), Some(4));
        assert_eq!(ru("четыре нет пять"), Some(5));
        assert_eq!(ru("четыре… нет, пять!"), Some(5));
        assert_eq!(ru("три четыре"), Some(4));
    }

    #[test]
    fn english_zero_synonyms() {
        for t in ["zero", "none", "nothing", "no cubes", "there are no cubes", "not any"] {
            assert_eq!(en(t), Some(0), "{t}");
        }
        assert_eq!(en("no"), None);
        assert_eq!(en("four, no"), Some(4));
    }

    #[test]
    fn english_slips_and_misses() {
        for (t, e) in [
            ("for", 4),
            ("to", 2),
            ("too", 2),
            ("won", 1),
            ("ate", 8),
            ("tree", 3),
            ("free", 3),
            ("sex", 6),
            ("sick", 6),
            ("fife", 5),
            ("I ate", 8),
            ("there are for", 4),
        ] {
            assert_eq!(en(t), Some(e), "{t}");
        }
        assert_eq!(en(""), None);
        assert_eq!(en("hello there"), None);
        assert_eq!(en("a cube"), None);
        assert_eq!(en("twenty five"), Some(5));
        assert_eq!(en("25"), None);
        assert_eq!(en("100"), None);
    }

    #[test]
    fn russian_sentences_zero_and_slips() {
        for (t, e) in [
            ("на столе четыре кубика", 4),
            ("На столе ЧЕТЫРЕ кубика.", 4),
            ("один кубик", 1),
            ("одна", 1),
            ("одно яблоко", 1),
            ("две", 2),
            ("тут пятнадцать", 15),
            ("15", 15),
            ("двадцать", 20),
            ("э-э… семь", 7),
            ("четыри", 4),
            ("читыре", 4),
            ("адин", 1),
            ("пядь", 5),
            ("шесь", 6),
            ("сем", 7),
            ("восим", 8),
            ("девить", 9),
            ("десить", 10),
            ("двацать", 20),
            ("одинадцать", 11),
        ] {
            assert_eq!(ru(t), Some(e), "{t}");
        }
        for t in ["ноль", "нет", "ничего", "ни одного", "ни одной", "нуль", "ничего нет"]
        {
            assert_eq!(ru(t), Some(0), "{t}");
        }
        assert_eq!(tokenize("Ёлка ещё"), vec!["елка", "еще"]);
        assert_eq!(ru("всё, пять"), Some(5));
        assert_eq!(ru(""), None);
        assert_eq!(ru("привет"), None);
        assert_eq!(ru("тридцать"), None);
    }

    #[test]
    fn vocabulary_is_consistent() {
        assert_eq!(NumberWords::spoken(1, EN), "one");
        assert_eq!(NumberWords::spoken(42, EN), "42");
        for lang in [EN, RU] {
            let words = NumberWords::words(lang);
            let unique: std::collections::HashSet<&String> = words.iter().collect();
            assert_eq!(unique.len(), words.len());
            for n in 0..=20 {
                assert!(words.contains(&NumberWords::spoken(n, lang)));
            }
            for (_, value) in NumberWords::entries(lang) {
                assert!(value <= NumberParser::MAX);
            }
        }
        assert_eq!(tokenize("Hello, World! 15."), vec!["hello", "world", "15"]);
        assert!(tokenize("   ").is_empty());
    }

    fn label(text: &str) -> Option<String> {
        CountQuestion::parse(text, EN, None).and_then(|q| q.label)
    }

    #[test]
    fn recognises_questions() {
        for (t, e) in [
            ("How many cars?", "car"),
            ("how many trucks are there", "car"),
            ("How many teddy bears are on the table?", "bear"),
            ("How many teddies do I have", "bear"),
            ("How many dolls?", "doll"),
            ("Count the dinosaurs", "animal"),
            ("how many cubes", "cube"),
            ("How many blocks are there?", "cube"),
            ("Hey, how many cubes are there now?", "cube"),
            ("HOW MANY BALLS", "ball"),
        ] {
            assert_eq!(label(t).as_deref(), Some(e), "{t}");
        }
        for t in ["How many objects?", "how many things are on the table", "Count all the toys", "How many altogether?"] {
            assert_eq!(CountQuestion::parse(t, EN, None), Some(CountQuestion::everything()), "{t}");
        }
        for t in ["cars", "How many?", "I like dolls", "how much is it", ""] {
            assert_eq!(label(t), None, "{t}");
        }
        assert_eq!(label("cars and dolls, how many dolls").as_deref(), Some("doll"));
        assert_eq!(CountQuestion::parse("сколько машинок", RU, None), None);
    }

    #[test]
    fn answers() {
        let cars = CountQuestion::about("car");
        let counts = |pairs: &[(&str, u32)]| pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect::<BTreeMap<_, _>>();
        assert_eq!(cars.answer(&counts(&[])), "There are no cars.");
        assert_eq!(cars.answer(&counts(&[("car", 1), ("doll", 4)])), "There is one car.");
        assert_eq!(cars.answer(&counts(&[("car", 3)])), "There are 3 cars.");
        let all = CountQuestion::everything();
        assert_eq!(all.answer(&counts(&[])), "There are no toys.");
        assert_eq!(all.answer(&counts(&[("car", 2), ("doll", 0)])), "There are 2 cars.");
        assert_eq!(all.answer(&counts(&[("car", 3), ("doll", 1)])), "There are 4 toys: 3 cars and one doll.");
        assert_eq!(
            all.answer(&counts(&[("cube", 2), ("car", 2), ("toy", 1)])),
            "There are 5 toys: 2 cars, 2 cubes and one other toy."
        );
        assert_eq!(all.answer(&counts(&[("toy", 3)])), "There are 3 toys.");
    }
}
