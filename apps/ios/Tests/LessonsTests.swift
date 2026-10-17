import XCTest

/// The product's lessons driven end to end through the Swift bindings: a scripted child, a
/// simulated table, the tutor's lines written down. No camera, no microphone; the engine's own
/// timers run, so a count takes the beat and a half the tutor waits after the last number.
final class LessonsTests: XCTestCase {
    private static let registered: Void = unmethod_mobile_init()

    /// One lesson with its doubles and the helpers that play the child.
    final class Lesson {
        let session: UnSession
        let child = ScriptedSpeech()
        let voice = RecordedVoice()
        private var heard = 0

        init(_ id: String, language: UnLanguage) throws {
            _ = LessonsTests.registered
            var config = defaultSessionConfig()
            config.language = language
            config.noun = "toy"
            config.seed = 1
            session = try UnSession(strategyId: id, config: config, speechInput: child, speechOutput: voice, chime: false, alwaysListen: false)
        }

        func shutdown() { session.shutdown() }

        /// A settled table with `count` toys.
        func settled(_ count: UInt32) {
            session.observe(count: count, isStable: true, groups: count > 0 ? 1 : 0)
        }

        /// Waits for the tutor's next line that starts with `prefix`; fails the test otherwise.
        func wait(for prefix: String, timeout: TimeInterval = 15, file: StaticString = #filePath, line: UInt = #line) {
            let deadline = Date().addingTimeInterval(timeout)
            while Date() < deadline {
                let spoken = voice.spoken
                while heard < spoken.count {
                    let text = spoken[heard]
                    heard += 1
                    if text.hasPrefix(prefix) { return }
                }
                Thread.sleep(forTimeInterval: 0.05)
            }
            XCTFail("did not hear \(prefix); heard \(voice.spoken)", file: file, line: line)
        }
    }

    func testTheListIsTheProductsLessonsOnly() {
        _ = LessonsTests.registered
        let ids = availableStrategies().map(\.id)
        XCTAssertEqual(ids.first, "unmath-count-to-five")
        XCTAssertTrue(ids.contains("unmath-add-to-five"), "\(ids)")
        XCTAssertFalse(ids.contains("demo"), "the engine's demo is not a product lesson")
        XCTAssertFalse(ids.contains("lesson-01"), "unmethod's lesson 1 is no longer in the app")
    }

    func testCountToFiveRunsThroughInRussian() throws {
        let lesson = try Lesson("unmath-count-to-five", language: .russian)
        defer { lesson.shutdown() }
        lesson.session.start()
        lesson.wait(for: "Можешь посчитать от одного до пяти?")
        lesson.child.say("один два три четыре пять")
        lesson.wait(for: "Положи на стол 5")
        lesson.settled(5)
        lesson.wait(for: "Посчитай эти игрушки вслух.")
        lesson.child.say("один два три четыре пять")
        lesson.wait(for: "Сколько всего игрушек?")
        lesson.child.say("пять")
        lesson.wait(for: "Можешь снова посчитать от одного до пяти самостоятельно?")
        lesson.child.say("один два три четыре пять")

        let deadline = Date().addingTimeInterval(10)
        while lesson.session.snapshot().summary == nil && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        let snapshot = lesson.session.snapshot()
        XCTAssertEqual(snapshot.phase, .finished)
        XCTAssertEqual(snapshot.summary?.paused, false)
    }

    func testAddToFiveStartsInEnglish() throws {
        let lesson = try Lesson("unmath-add-to-five", language: .english)
        defer { lesson.shutdown() }
        lesson.session.start()
        let deadline = Date().addingTimeInterval(5)
        while lesson.voice.spoken.isEmpty && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
        XCTAssertFalse(lesson.voice.spoken.isEmpty, "the lesson says something at start")
    }
}
