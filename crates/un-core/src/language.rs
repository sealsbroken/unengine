//! Languages, number words and plural agreement.
//!
//! No LLM anywhere: wording is template-based, predictable and testable. This
//! module holds the linguistic infrastructure every strategy shares; what the tutor
//! actually says belongs to the strategy.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    English,
    Russian,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Russian];

    /// BCP-47 identifier used for speech recognition and synthesis.
    pub fn locale(self) -> &'static str {
        match self {
            Language::English => "en-US",
            Language::Russian => "ru-RU",
        }
    }

    /// Two-letter code used in file and model names.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Russian => "ru",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Russian => "Русский",
        }
    }

    /// Accepts `en`, `eng`, `english`, `en-US`, `ru`, `rus`, `russian`, `ru_RU`...
    pub fn parse(text: &str) -> Option<Language> {
        match text.trim().to_lowercase().replace('_', "-").as_str() {
            "en" | "eng" | "english" | "en-us" | "en-gb" => Some(Language::English),
            "ru" | "rus" | "russian" | "ru-ru" | "русский" => Some(Language::Russian),
            _ => None,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One countable object in both languages: English singular/plural, Russian 1 / 2-4 / 5+ forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Noun {
    pub en_one: String,
    pub en_many: String,
    pub ru_one: String,
    pub ru_few: String,
    /// Genitive plural, after 5+ and "сколько": "кубиков".
    pub ru_many: String,
    /// "ни одного кубика" / "ни одной крышки" (gender agreement).
    pub ru_none: String,
    /// Nominative/accusative plural, "считать кубики".
    pub ru_plural: String,
}

impl Noun {
    pub fn new(en_one: &str, en_many: &str, ru_one: &str, ru_few: &str, ru_many: &str, ru_none: &str, ru_plural: &str) -> Self {
        Self {
            en_one: en_one.into(),
            en_many: en_many.into(),
            ru_one: ru_one.into(),
            ru_few: ru_few.into(),
            ru_many: ru_many.into(),
            ru_none: ru_none.into(),
            ru_plural: ru_plural.into(),
        }
    }

    pub fn cube() -> Self {
        Noun::new("cube", "cubes", "кубик", "кубика", "кубиков", "ни одного кубика", "кубики")
    }

    /// Russian grammatical gender, read off the "not a single" form: "ни одной крышки" is feminine.
    pub fn ru_feminine(&self) -> bool {
        self.ru_none.starts_with("ни одной")
    }

    /// The noun for a detector label; unknown labels get a plain English plural.
    pub fn for_label(label: &str) -> Noun {
        match label {
            "" | "cube" => Noun::cube(),
            "circle" => Noun::new("circle", "circles", "кружок", "кружка", "кружков", "ни одного кружка", "кружки"),
            "cap" => Noun::new("cap", "caps", "крышка", "крышки", "крышек", "ни одной крышки", "крышки"),
            "coin" => Noun::new("coin", "coins", "монета", "монеты", "монет", "ни одной монеты", "монеты"),
            "ball" => Noun::new("ball", "balls", "мяч", "мяча", "мячей", "ни одного мяча", "мячи"),
            "car" => Noun::new("car", "cars", "машинка", "машинки", "машинок", "ни одной машинки", "машинки"),
            "pencil" => Noun::new("pencil", "pencils", "карандаш", "карандаша", "карандашей", "ни одного карандаша", "карандаши"),
            "fruit" => Noun::new("fruit", "fruits", "фрукт", "фрукта", "фруктов", "ни одного фрукта", "фрукты"),
            "stone" => Noun::new("stone", "stones", "камень", "камня", "камней", "ни одного камня", "камни"),
            "object" => Noun::new("object", "objects", "предмет", "предмета", "предметов", "ни одного предмета", "предметы"),
            other => Noun::new(other, &format!("{other}s"), other, other, other, &format!("ни одного {other}"), other),
        }
    }
}

/// Number words and the noun with correct plural agreement, 0...20.
pub struct Numerals;

impl Numerals {
    pub const ENGLISH: [&'static str; 21] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    pub const RUSSIAN: [&'static str; 21] = [
        "ноль",
        "один",
        "два",
        "три",
        "четыре",
        "пять",
        "шесть",
        "семь",
        "восемь",
        "девять",
        "десять",
        "одиннадцать",
        "двенадцать",
        "тринадцать",
        "четырнадцать",
        "пятнадцать",
        "шестнадцать",
        "семнадцать",
        "восемнадцать",
        "девятнадцать",
        "двадцать",
    ];

    /// "four" / "четыре". Falls back to digits above twenty.
    pub fn word(n: u32, language: Language) -> String {
        let table: &[&str] = match language {
            Language::English => &Self::ENGLISH,
            Language::Russian => &Self::RUSSIAN,
        };
        table.get(n as usize).map(|w| w.to_string()).unwrap_or_else(|| n.to_string())
    }

    /// "cube"/"cubes" or "кубик"/"кубика"/"кубиков" agreeing with `n`.
    pub fn noun(n: u32, language: Language, noun: &Noun) -> String {
        match language {
            Language::English => {
                if n == 1 {
                    noun.en_one.clone()
                } else {
                    noun.en_many.clone()
                }
            }
            Language::Russian => {
                let (mod10, mod100) = (n % 10, n % 100);
                if mod10 == 1 && mod100 != 11 {
                    noun.ru_one.clone()
                } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
                    noun.ru_few.clone()
                } else {
                    noun.ru_many.clone()
                }
            }
        }
    }

    /// "four cubes" / "четыре кубика". Zero becomes "no cubes" / "ни одного кубика". Russian
    /// "one" and "two" agree with the noun's gender: "одна крышка", "две крышки".
    pub fn objects(n: u32, language: Language, noun: &Noun) -> String {
        if n == 0 {
            return match language {
                Language::English => format!("no {}", noun.en_many),
                Language::Russian => noun.ru_none.clone(),
            };
        }
        let number = match (language, noun.ru_feminine(), n % 10, n % 100) {
            (Language::Russian, true, 1, m) if m != 11 => Self::word(n, language).replace("один", "одна"),
            (Language::Russian, true, 2, m) if m != 12 => Self::word(n, language).replace("два", "две"),
            _ => Self::word(n, language),
        };
        format!("{number} {}", Self::noun(n, language, noun))
    }

    /// "one, two, three" / "один, два, три" for counting aloud.
    pub fn counting_sequence(n: u32, language: Language) -> String {
        if n < 1 {
            return Self::word(0, language);
        }
        (1..=n).map(|i| Self::word(i, language)).collect::<Vec<_>>().join(", ")
    }
}

/// Upper-cases the first character.
pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Language::{English as EN, Russian as RU};

    #[test]
    fn locales_and_parsing() {
        assert_eq!(EN.locale(), "en-US");
        assert_eq!(RU.locale(), "ru-RU");
        assert_eq!(Language::parse("ru"), Some(RU));
        assert_eq!(Language::parse("en-US"), Some(EN));
        assert_eq!(Language::parse("Russian"), Some(RU));
        assert_eq!(Language::parse("de"), None);
    }

    #[test]
    fn russian_plurals() {
        let cube = Noun::cube();
        assert_eq!(Numerals::objects(1, RU, &cube), "один кубик");
        assert_eq!(Numerals::objects(2, RU, &cube), "два кубика");
        assert_eq!(Numerals::objects(4, RU, &cube), "четыре кубика");
        assert_eq!(Numerals::objects(5, RU, &cube), "пять кубиков");
        assert_eq!(Numerals::objects(11, RU, &cube), "одиннадцать кубиков");
        assert_eq!(Numerals::objects(12, RU, &cube), "двенадцать кубиков");
        assert_eq!(Numerals::objects(0, RU, &cube), "ни одного кубика");
        assert_eq!(Numerals::noun(21, RU, &cube), "кубик");
        assert_eq!(Numerals::noun(22, RU, &cube), "кубика");
    }

    #[test]
    fn english_plurals() {
        let cube = Noun::cube();
        assert_eq!(Numerals::objects(1, EN, &cube), "one cube");
        assert_eq!(Numerals::objects(4, EN, &cube), "four cubes");
        assert_eq!(Numerals::objects(0, EN, &cube), "no cubes");
    }

    #[test]
    fn counting_sequence() {
        assert_eq!(Numerals::counting_sequence(3, EN), "one, two, three");
        assert_eq!(Numerals::counting_sequence(4, RU), "один, два, три, четыре");
    }

    #[test]
    fn other_nouns() {
        let cap = Noun::for_label("cap");
        assert_eq!(Numerals::objects(2, EN, &cap), "two caps");
        assert_eq!(Numerals::objects(5, RU, &cap), "пять крышек");
        assert_eq!(Numerals::objects(0, RU, &cap), "ни одной крышки");
        assert_eq!(Numerals::objects(1, RU, &cap), "одна крышка");
        assert_eq!(Numerals::objects(2, RU, &cap), "две крышки");
        assert_eq!(Numerals::objects(12, RU, &cap), "двенадцать крышек");
        assert_eq!(Numerals::objects(2, RU, &Noun::cube()), "два кубика");
        assert_eq!(Numerals::objects(3, EN, &Noun::for_label("widget")), "three widgets");
        let stone = Noun::for_label("stone");
        assert_eq!(Numerals::objects(1, RU, &stone), "один камень");
        assert_eq!(Numerals::objects(3, RU, &stone), "три камня");
        assert_eq!(Numerals::objects(7, RU, &stone), "семь камней");
        assert_eq!(Numerals::objects(7, EN, &stone), "seven stones");
        assert_eq!(capitalize("четыре кубика"), "Четыре кубика");
    }
}
