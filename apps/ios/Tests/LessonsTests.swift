import XCTest

/// The product's lessons driven end to end through the Swift bindings: a scripted child, a
/// simulated table, the tutor's lines written down. No camera, no microphone; the engine's own
/// timers run, so a count takes the beat and a half the tutor waits after the last number.
final class LessonsTests: XCTestCase {
    private static let registered: Void = un_mobile_init()

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
        // The course opens with counting (its exact id changes with the course: count-to-five, count-to-three).
        XCTAssertTrue(ids.first?.hasPrefix("unmath-count-to-") == true, "\(ids)")
        XCTAssertTrue(ids.allSatisfy { $0.hasPrefix("unmath-") }, "\(ids)")
        XCTAssertFalse(ids.contains("demo"), "the engine's demo is not a product lesson")
    }

    /// Every lesson in the list starts and speaks, in both languages. What each lesson says next
    /// is the lesson repository's own business (its tests); here the list and the bindings are.
    func testEveryLessonStartsSpeaking() throws {
        _ = LessonsTests.registered
        for info in availableStrategies() {
            for language in [UnLanguage.russian, .english] {
                let lesson = try Lesson(info.id, language: language)
                lesson.session.start()
                let deadline = Date().addingTimeInterval(5)
                while lesson.voice.spoken.isEmpty && Date() < deadline { Thread.sleep(forTimeInterval: 0.05) }
                XCTAssertFalse(lesson.voice.spoken.isEmpty, "\(info.id) says nothing at start in \(language)")
                lesson.shutdown()
            }
        }
    }
}
