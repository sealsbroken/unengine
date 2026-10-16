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
            session = try UnSession(strategyId: "lesson-01", config: config, speechInput: child, speechOutput: voice, chime: false, alwaysListen: false, chatModel: nil)
        }

        func shutdown() { session.shutdown() }

        // ---- the table

        func observe(count: UInt32?, hands: Bool, stable: Bool, groups: UInt32, pointedAt: UInt32? = nil) {
            session.observe(count: count, handsPresent: hands, isStable: stable, fingerCount: nil, groups: groups, pointedAt: pointedAt)
        }

        /// A settled, hands-free table with `count` objects in `groups` groups.
        func settled(_ count: UInt32, groups: UInt32) {
            observe(count: count, hands: false, stable: true, groups: groups)
        }

        /// A hand over the table, the count not settled.
        func hands(_ count: UInt32) {
            observe(count: count, hands: true, stable: false, groups: 0)
        }

        /// A finger on object `number` of `count`.
        func touch(_ count: UInt32, _ number: UInt32) {
            observe(count: count, hands: true, stable: false, groups: 0, pointedAt: number)
        }

        /// Lays `count` objects in `groups` groups: hands come in, then the table settles.
        func lay(_ count: UInt32, groups: UInt32) {
            hands(count)
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

        /// Counts `count` objects the way the lesson wants it: a finger on each, the number said,
        /// the tutor quiet throughout, then the hand lifted off the table.
        func count(_ count: UInt32, numbers: [String], groups: UInt32,
                   file: StaticString = #filePath, line: UInt = #line) {
            let before = voice.spoken.count
            for (i, word) in numbers.prefix(Int(count)).enumerated() {
                touch(count, UInt32(i + 1))
                child.say(word)
                Thread.sleep(forTimeInterval: 0.4)
            }
            XCTAssertEqual(voice.spoken.count, before, "the tutor does not interrupt a count", file: file, line: line)
            hands(count)
            // The tutor waits a beat after the last number before taking the count as finished,
            // then for the hand to leave.
            Thread.sleep(forTimeInterval: 2.0)
            settled(count, groups: groups)
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
        lesson.wait(for: "Теперь покажи пальцем на каждую игрушку и посчитай вслух.")
        XCTAssertEqual(lesson.session.snapshot().phase, .asking)
        lesson.count(2, numbers: ru, groups: 1)
        lesson.wait(for: "А сколько у тебя ушей? Выложи столько же игрушек отдельно от глаз.")

        lesson.lay(4, groups: 2)
        lesson.wait(for: "Теперь посчитай все игрушки вместе. Показывай пальцем на каждую и называй число.")
        lesson.count(4, numbers: ru, groups: 2)
        lesson.wait(for: "А сколько у тебя ртов? Добавь столько же игрушек.")

        lesson.lay(5, groups: 3)
        lesson.wait(for: "Посчитай все игрушки. Не забывай показывать пальцем на каждую.")
        lesson.count(5, numbers: ru, groups: 3)
        lesson.wait(for: "Теперь убери игрушки. Попробуем по-другому.")

        lesson.lay(0, groups: 0)
        lesson.wait(for: "Выложи число глаз.")
        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь прибавь к нему число ртов. Выложи их отдельно.")
        lesson.lay(3, groups: 2)
        lesson.wait(for: "Посчитай, что получилось. Показывай на игрушки по одной.")
        lesson.count(3, numbers: ru, groups: 2)
        lesson.wait(for: "Теперь прибавь число ушей.")
        lesson.lay(5, groups: 3)
        lesson.wait(for: "Посчитай все игрушки вместе, показывая на каждую.")
        lesson.count(5, numbers: ru, groups: 3)
        lesson.wait(for: "Сегодня мы считали глаза, уши и рот с помощью игрушек. До встречи!")

        let deadline = Date().addingTimeInterval(5)
        while lesson.session.snapshot().summary == nil && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        let snapshot = lesson.session.snapshot()
        XCTAssertEqual(snapshot.phase, .finished)
        XCTAssertEqual(snapshot.summary?.tasksCompleted, 12)
        XCTAssertEqual(snapshot.summary?.correct, 12)
    }

    func testCountingWithoutPointingIsAskedAgainWithTheFinger() throws {
        let lesson = try Lesson(language: .russian, child: "Вася")
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait(for: "Привет, Вася!")
        lesson.settled(0, groups: 0)
        lesson.child.say("да")
        lesson.wait(for: "Сколько у тебя глаз?")
        lesson.lay(2, groups: 1)
        lesson.wait(for: "Теперь покажи пальцем")
        // The right numbers, but only a hand over the table, never a finger on a toy.
        lesson.hands(2)
        lesson.child.say("один")
        lesson.child.say("два")
        Thread.sleep(forTimeInterval: 2.0)
        lesson.settled(2, groups: 1)
        lesson.wait(for: "Числа правильные, но есть ошибочка: пальцем на игрушки не показано. Давай ещё раз, показывая пальцем на каждую игрушку.")
        XCTAssertEqual(lesson.session.snapshot().phase, .asking)
        // Properly this time.
        lesson.count(2, numbers: ru, groups: 1)
        lesson.wait(for: "А сколько у тебя ушей?")
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
        lesson.wait(for: "Теперь покажи пальцем")
        lesson.count(2, numbers: ru, groups: 1)
        lesson.wait(for: "А сколько у тебя ушей?")
        lesson.lay(4, groups: 2)
        lesson.wait(for: "Теперь посчитай все игрушки вместе.")
        // "One, two, four": three is skipped. No verdict until the hand is off the table.
        for (i, word) in ["один", "два", "четыре"].enumerated() {
            lesson.touch(4, UInt32(i + 1))
            lesson.child.say(word)
        }
        lesson.hands(4)
        Thread.sleep(forTimeInterval: 2.0)
        lesson.settled(4, groups: 2)
        lesson.wait(for: "Ой, тут ошибочка, ничего страшного. Давай ещё раз: показывай на игрушки по одной и называй число.")
        // "One, two, three, three": still wrong, so the tutor counts together and moves on.
        for (i, word) in ["один", "два", "три", "три"].enumerated() {
            lesson.touch(4, UInt32(min(i + 1, 3)))
            lesson.child.say(word)
        }
        lesson.hands(4)
        // Not the expected number, so the tutor waits the full quiet window before taking the
        // count as finished.
        Thread.sleep(forTimeInterval: 5.5)
        lesson.settled(4, groups: 2)
        lesson.wait(for: "Снова ошибочка, ничего страшного. Давай посчитаем вместе: один, два, три, четыре. Четыре игрушки!")
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
        lesson.wait("the counting instruction") { $0.contains("point") }
        lesson.count(2, numbers: ["one", "two"], groups: 1)
        lesson.wait("the ears") { $0.contains("ears") }
    }
}
