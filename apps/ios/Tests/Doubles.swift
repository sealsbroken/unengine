import Foundation

/// A child who says what the test tells it to, when the test tells it to: `say` queues a line,
/// and the engine's next listen returns it. Nothing queued means a quiet room: the listen
/// waits out its timeout (or a cancel) and returns nil, as the real recogniser would.
final class ScriptedSpeech: SpeechInputCallback, @unchecked Sendable {
    private let lock = NSCondition()
    private var lines: [String] = []
    private var cancelled = false

    func say(_ text: String) {
        lock.lock()
        lines.append(text)
        lock.broadcast()
        lock.unlock()
    }

    func prepare(language: UnLanguage) throws {}

    func listenForNumber(language: UnLanguage, timeoutSeconds: Double) -> UnHeard? {
        guard let text = listenForText(language: language, timeoutSeconds: timeoutSeconds) else { return nil }
        return NumberWords.parse(text, language: language).map { UnHeard(value: $0, transcript: text) }
    }

    func listenForText(language: UnLanguage, timeoutSeconds: Double) -> String? {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        lock.lock()
        defer { lock.unlock() }
        cancelled = false
        while lines.isEmpty && !cancelled {
            if !lock.wait(until: min(deadline, Date().addingTimeInterval(0.05))) && Date() >= deadline { break }
            if Date() >= deadline { break }
        }
        if cancelled || lines.isEmpty { return nil }
        return lines.removeFirst()
    }

    func bargeIn() -> Bool { false }

    func cancelListening() {
        lock.lock()
        cancelled = true
        lock.broadcast()
        lock.unlock()
    }
}

/// The tutor's voice, written down instead of spoken.
final class RecordedVoice: SpeechOutputCallback, @unchecked Sendable {
    private let lock = NSLock()
    private var lines: [String] = []

    var spoken: [String] {
        lock.lock(); defer { lock.unlock() }
        return lines
    }

    func prepare(language: UnLanguage) throws {}

    func speak(text: String, language: UnLanguage) throws {
        lock.lock()
        lines.append(text)
        lock.unlock()
    }

    func stopSpeaking() {}
    func playChime() {}
}
