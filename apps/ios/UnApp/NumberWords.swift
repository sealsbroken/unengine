import Foundation

/// A small forgiving number parser mirroring `un_core::NumberParser` for the Apple recogniser path.
enum NumberWords {
    static let english: [String: UInt32] = [
        "zero": 0, "none": 0, "nothing": 0, "one": 1, "won": 1, "two": 2, "to": 2, "too": 2, "three": 3, "tree": 3, "free": 3,
        "four": 4, "for": 4, "five": 5, "six": 6, "seven": 7, "eight": 8, "ate": 8, "nine": 9, "ten": 10, "eleven": 11,
        "twelve": 12, "thirteen": 13, "fourteen": 14, "fifteen": 15, "sixteen": 16, "seventeen": 17, "eighteen": 18,
        "nineteen": 19, "twenty": 20,
    ]
    static let russian: [String: UInt32] = [
        "ноль": 0, "нет": 0, "ничего": 0, "один": 1, "одна": 1, "одно": 1, "два": 2, "две": 2, "три": 3, "четыре": 4,
        "пять": 5, "шесть": 6, "семь": 7, "восемь": 8, "девять": 9, "десять": 10, "одиннадцать": 11, "двенадцать": 12,
        "тринадцать": 13, "четырнадцать": 14, "пятнадцать": 15, "шестнадцать": 16, "семнадцать": 17, "восемнадцать": 18,
        "девятнадцать": 19, "двадцать": 20,
    ]

    /// The last number in the transcript, like the engine's parser.
    static func parse(_ text: String, language: UnLanguage) -> UInt32? {
        let table = language == .russian ? russian : english
        var last: UInt32?
        for raw in text.lowercased().replacingOccurrences(of: "ё", with: "е").split(whereSeparator: { !$0.isLetter && !$0.isNumber }) {
            let token = String(raw)
            if let digits = UInt32(token), digits <= 20 { last = digits } else if let value = table[token] { last = value }
        }
        return last
    }
}
