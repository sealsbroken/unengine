import XCTest

/// Lesson 1 driven end to end through the Swift bindings: a scripted child, a simulated table,
/// the tutor's lines written down. No camera, no microphone; the engine's own timers run, so a
/// count takes the beat and a half the tutor waits after the last number.
final class Lesson1Tests: XCTestCase {
    private static let registered: Void = unmethod_mobile_init()

    /// One lesson with its doubles and the helpers that play the child.
    final class Lesson {
        let session: UnSession
        let child = ScriptedSpeech()
        let voice = RecordedVoice()
        private var heard = 0

        init(language: UnLanguage, child name: String) throws {
            _ = Lesson1Tests.registered
            var config = defaultSessionConfig()
            config.language = language
            config.childName = name
            config.noun = "toy"
            config.seed = 1
            session = try UnSession(strategyId: "lesson-01", config: config, speechInput: child, speechOutput: voice, chime: false, alwaysListen: false)
        }

        func shutdown() { session.shutdown() }

        // ---- the table

        func observe(count: UInt32?, stable: Bool, groups: UInt32) {
            session.observe(count: count, isStable: stable, groups: groups)
        }

        /// A settled table with `count` objects in `groups` groups.
        func settled(_ count: UInt32, groups: UInt32) {
            observe(count: count, stable: true, groups: groups)
        }

        /// Toys being moved: the count not settled.
        func moving(_ count: UInt32) {
            observe(count: count, stable: false, groups: 0)
        }

        /// Lays `count` objects in `groups` groups: toys move, then the table settles.
        func lay(_ count: UInt32, groups: UInt32) {
            moving(count)
            settled(count, groups: groups)
        }

        // ---- the voice

        /// Waits for the tutor's next line that satisfies `test`; fails the test otherwise.
        @discardableResult
        func wait(_ what: String, timeout: TimeInterval = 15, file: StaticString = #filePath, line: UInt = #line,
                  _ test: (String) -> Bool) -> String? {
            let deadline = Date().addingTimeInterval(timeout)
            while Date() < deadline {
                let spoken = voice.spoken
                while heard < spoken.count {
                    let text = spoken[heard]
                    heard += 1
                    if test(text) { return text }
                }
                Thread.sleep(forTimeInterval: 0.05)
            }
            XCTFail("did not hear \(what); heard \(voice.spoken)", file: file, line: line)
            return nil
        }

        func wait(for prefix: String, timeout: TimeInterval = 15, file: StaticString = #filePath, line: UInt = #line) {
            wait(prefix, timeout: timeout, file: file, line: line) { $0.hasPrefix(prefix) }
        }

        /// Counts `count` objects aloud, one number at a time, the tutor quiet throughout.
        func count(_ count: UInt32, numbers: [String], file: StaticString = #filePath, line: UInt = #line) {
            let before = voice.spoken.count
            for word in numbers.prefix(Int(count)) {
                child.say(word)
                Thread.sleep(forTimeInterval: 0.4)
            }
            XCTAssertEqual(voice.spoken.count, before, "the tutor does not interrupt a count", file: file, line: line)
        }
    }

    private let ru = ["один", "два", "три", "четыре", "пять"]

    func testTheWholeScriptRunsThroughInRussian() throws {
        let lesson = try Lesson(language: .russian, child: "Вася")
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait(for: "Привет, Вася! Сегодня будем считать с помощью игрушек. Готов?")
        lesson.settled(0, groups: 0)
        lesson.child.say("да")
        lesson.wait(for: "Сколько у тебя глаз? Выложи столько же игрушек.")

        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь посчитай игрушки вслух: по одному числу на каждую игрушку.")
        XCTAssertEqual(lesson.session.snapshot().phase, .asking)
        lesson.count(2, numbers: ru)
        lesson.wait(for: "А сколько у тебя ушей? Выложи столько же игрушек отдельно от глаз.")

        lesson.lay(4, groups: 2)
        lesson.wait(for: "Теперь посчитай все игрушки вместе. Называй по одному числу на каждую.")
        lesson.count(4, numbers: ru)
        lesson.wait(for: "А сколько у тебя ртов? Добавь столько же игрушек.")

        lesson.lay(5, groups: 3)
        lesson.wait(for: "Посчитай все игрушки. Не забывай: по одному числу на каждую.")
        lesson.count(5, numbers: ru)
        lesson.wait(for: "Теперь убери игрушки. Попробуем по-другому.")

        lesson.lay(0, groups: 0)
        lesson.wait(for: "Выложи число глаз.")
        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь прибавь к нему число ртов. Выложи их отдельно.")
        lesson.lay(3, groups: 2)
        lesson.wait(for: "Посчитай, что получилось. Считай игрушки по одной.")
        lesson.count(3, numbers: ru)
        lesson.wait(for: "Теперь прибавь число ушей.")
        lesson.lay(5, groups: 3)
        lesson.wait(for: "Посчитай все игрушки вместе, по одному числу на каждую.")
        lesson.count(5, numbers: ru)
        lesson.wait(for: "Сегодня мы считали глаза, уши и рот с помощью игрушек. До встречи!")

        let deadline = Date().addingTimeInterval(5)
        while lesson.session.snapshot().summary == nil && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        let snapshot = lesson.session.snapshot()
        XCTAssertEqual(snapshot.phase, .finished)
        XCTAssertEqual(snapshot.summary?.tasksCompleted, 12)
        XCTAssertEqual(snapshot.summary?.correct, 12)
    }

    func testWordsOffTheScriptGetAScriptedLine() throws {
        let lesson = try Lesson(language: .russian, child: "Вася")
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait(for: "Привет, Вася!")
        lesson.settled(0, groups: 0)
        lesson.child.say("ты какашка")
        lesson.wait(for: "Ну что, начнём считать?")
        lesson.child.say("да")
        lesson.wait(for: "Сколько у тебя глаз?")
        lesson.child.say("а ты любишь собак")
        lesson.wait(for: "Я тебя слышу. А теперь покажи на столе: выложи столько игрушек, сколько у тебя глаз.")
        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь посчитай игрушки вслух")
    }

    func testASkippedNumberGetsOneMoreTryThenCountingTogether() throws {
        let lesson = try Lesson(language: .russian, child: "Вася")
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait(for: "Привет, Вася!")
        lesson.settled(0, groups: 0)
        lesson.child.say("да")
        lesson.wait(for: "Сколько у тебя глаз?")
        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь посчитай игрушки вслух")
        lesson.count(2, numbers: ru)
        lesson.wait(for: "А сколько у тебя ушей?")
        lesson.lay(4, groups: 2)
        lesson.wait(for: "Теперь посчитай все игрушки вместе.")
        // "One, two, four": three is skipped.
        for word in ["один", "два", "четыре"] {
            lesson.child.say(word)
        }
        lesson.wait(for: "Ой, тут ошибочка: одно число пропущено. Ничего страшного. Давай ещё раз: считай игрушки по одной вслух.")
        // "One, two, three, three": still wrong, so the tutor counts together and moves on.
        for word in ["один", "два", "три", "три"] {
            lesson.child.say(word)
        }
        // Not the expected number, so the tutor waits the full quiet window before taking the
        // count as finished.
        lesson.wait(for: "Снова ошибочка: одно число сказано два раза. Ничего страшного. Давай посчитаем вместе: один, два, три, четыре. Четыре игрушки!")
        lesson.wait(for: "А сколько у тебя ртов?")
        let snapshot = lesson.session.snapshot()
        XCTAssertEqual(snapshot.tasksCompleted, 4)
        XCTAssertEqual(snapshot.correct, 3)
    }

    func testTheFirstStepsInEnglish() throws {
        let lesson = try Lesson(language: .english, child: "Mia")
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait("the opening") { $0.hasPrefix("Hi, Mia!") && $0.hasSuffix("Ready?") }
        lesson.settled(0, groups: 0)
        lesson.child.say("yes")
        lesson.wait(for: "How many eyes do you have? Lay out that many toys.")
        lesson.lay(2, groups: 1)
        lesson.wait("the counting instruction") { $0.contains("out loud") }
        lesson.count(2, numbers: ["one", "two"])
        lesson.wait("the ears") { $0.contains("ears") }
    }
}
