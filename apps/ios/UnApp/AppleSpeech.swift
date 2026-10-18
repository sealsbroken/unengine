import AVFoundation
import Speech

/// The tutor's voice through AVSpeechSynthesizer. `speak` blocks the engine's effect thread until done.
final class AppleSpeechOutput: NSObject, SpeechOutputCallback, AVSpeechSynthesizerDelegate {
    private let synthesizer = AVSpeechSynthesizer()
    private var finished = DispatchSemaphore(value: 0)

    override init() {
        super.init()
        synthesizer.delegate = self
    }

    func prepare(language: UnLanguage) throws { AudioSession.configure() }

    func speak(text: String, language: UnLanguage) throws {
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = AVSpeechSynthesisVoice(language: language == .russian ? "ru-RU" : "en-US")
        // The voice's own pace (speed 1).
        utterance.rate = AVSpeechUtteranceDefaultSpeechRate
        // A child's voice: the system has no child voice for Russian, so the pitch goes up.
        utterance.pitchMultiplier = 1.45
        finished = DispatchSemaphore(value: 0)
        AudioSession.tutorSpeaking(true)
        DispatchQueue.main.async { self.synthesizer.speak(utterance) }
        finished.wait()
        AudioSession.tutorSpeaking(false)
    }

    func stopSpeaking() {
        DispatchQueue.main.async { self.synthesizer.stopSpeaking(at: .immediate) }
    }

    func playChime() { AudioServicesPlaySystemSound(1113) }

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) { finished.signal() }
    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didCancel utterance: AVSpeechUtterance) { finished.signal() }
}

/// The child's words through SFSpeechRecognizer (on the device when the language allows).
/// `listenForNumber` returns as soon as a number is in the transcript; `listenForText` returns the
/// new words once the child has paused. The engine parses numbers itself too (0...20), the local
/// parser only decides when to stop listening.
///
/// One recognition request runs continuously; each call hands over the words added since the
/// last one, once the transcript has stood still for a moment. Cutting a request at every pause
/// lost the word the child had already begun (a count finger by finger came out "один, три,
/// пять"), and starting the next one over again heard words twice. The request is replaced only
/// when the tutor speaks (its voice is not the child's), when the recogniser ends it, or after
/// about fifty quiet seconds (a request has a limit).
final class AppleSpeechInput: SpeechInputCallback, @unchecked Sendable {
    private let audioEngine = AVAudioEngine()
    private let lock = NSLock()
    private var tapInstalled = false
    /// Bumped by every cancel (the tutor is about to speak); the request is then dropped.
    private var cancelGeneration = 0

    private final class Stream {
        let request = SFSpeechAudioBufferRecognitionRequest()
        var task: SFSpeechRecognitionTask?
        let language: UnLanguage
        let started = Date()
        var transcript = ""
        var changedAt = Date()
        var ended = false
        /// Words of the transcript already handed over.
        var handedOver = 0
        var pendingWords: Int { max(0, AppleSpeechInput.words(transcript).count - handedOver) }
        init(language: UnLanguage) { self.language = language }
    }
    private var stream: Stream?
    /// How long the transcript must stand still before new words are handed over.
    private let pause: TimeInterval = 1.2
    private let maxAge: TimeInterval = 50

    func prepare(language: UnLanguage) throws {
        let status = SFSpeechRecognizer.authorizationStatus()
        guard status == .authorized || status == .notDetermined else {
            throw UnError.Failed(reason: "speech recognition not authorised")
        }
        if status == .notDetermined { SFSpeechRecognizer.requestAuthorization { _ in } }
        AVAudioApplication.requestRecordPermission { _ in }
        AudioSession.configure()
    }

    func listenForNumber(language: UnLanguage, timeoutSeconds: Double) -> UnHeard? {
        var heard: UnHeard?
        _ = listen(language: language, timeoutSeconds: timeoutSeconds) { text in
            guard let value = NumberWords.parse(text, language: language) else { return false }
            heard = UnHeard(value: value, transcript: text)
            return true
        }
        return heard
    }

    func listenForText(language: UnLanguage, timeoutSeconds: Double) -> String? {
        listen(language: language, timeoutSeconds: timeoutSeconds) { _ in false }
    }

    /// Starts the microphone once (again after an interruption stopped it); false without one.
    private func microphone() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if tapInstalled && audioEngine.isRunning { return true }
        AudioSession.configure()
        let input = audioEngine.inputNode
        // The tutor's voice can reconfigure the audio session, which stops the engine; the tap is
        // set up again for the input's format as it is now.
        if tapInstalled {
            input.removeTap(onBus: 0)
            tapInstalled = false
        }
        let format = input.outputFormat(forBus: 0)
        // No microphone (the simulator, say): installing a tap would raise an Objective-C
        // exception, which cannot pass through the engine's Rust frames.
        guard format.channelCount > 0, format.sampleRate > 0 else { return false }
        debug("microphone started (\(format.sampleRate) Hz)")
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let self, !AudioSession.isTutorSpeaking else { return }
            self.lock.lock(); let stream = self.stream; self.lock.unlock()
            stream?.request.append(buffer)
        }
        tapInstalled = true
        audioEngine.prepare()
        return (try? audioEngine.start()) != nil
    }

    /// The running stream, or a new one when there is none (or it ended, is too old and quiet, or
    /// is in another language).
    private func currentStream(_ language: UnLanguage) -> Stream? {
        lock.lock()
        if let stream, !stream.ended, stream.language == language,
           Date().timeIntervalSince(stream.started) < maxAge || stream.pendingWords > 0 {
            lock.unlock()
            return stream
        }
        let old = stream
        stream = nil
        lock.unlock()
        old?.request.endAudio()
        old?.task?.cancel()
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language == .russian ? "ru-RU" : "en-US")),
              recognizer.isAvailable else {
            debug("the recogniser is not available")
            return nil
        }
        let fresh = Stream(language: language)
        fresh.request.shouldReportPartialResults = true
        fresh.request.requiresOnDeviceRecognition = recognizer.supportsOnDeviceRecognition
        fresh.request.taskHint = .dictation
        fresh.task = recognizer.recognitionTask(with: fresh.request) { [weak self, weak fresh] result, error in
            guard let self, let fresh else { return }
            self.lock.lock(); defer { self.lock.unlock() }
            if let result {
                let text = result.bestTranscription.formattedString
                if text != fresh.transcript { fresh.transcript = text; fresh.changedAt = Date() }
                if result.isFinal { fresh.ended = true }
            }
            if let error {
                fresh.ended = true
                self.debug("recognition ended: \(error.localizedDescription)")
            }
        }
        lock.lock(); stream = fresh; lock.unlock()
        debug("listening")
        return fresh
    }

    /// Waits for new words: handed over once the transcript has stood still for `pause` (at once
    /// when `done` accepts them); nil on timeout or when the tutor starts to speak.
    private func listen(language: UnLanguage, timeoutSeconds: Double, done: @escaping (String) -> Bool) -> String? {
        lock.lock(); let generation = cancelGeneration; lock.unlock()
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        // Never start recognising over the tutor's own voice.
        while AudioSession.isTutorSpeaking {
            if Date() >= deadline { return nil }
            Thread.sleep(forTimeInterval: 0.05)
        }
        guard microphone() else {
            debug("the microphone did not start")
            Thread.sleep(forTimeInterval: 0.5)
            return nil
        }
        guard let stream = currentStream(language) else {
            debug("no recogniser for \(language)")
            Thread.sleep(forTimeInterval: 0.5)
            return nil
        }
        while true {
            Thread.sleep(forTimeInterval: 0.1)
            lock.lock()
            if cancelGeneration != generation || AudioSession.isTutorSpeaking {
                lock.unlock()
                return nil
            }
            let words = Self.words(stream.transcript)
            let fresh = Array(words.dropFirst(min(stream.handedOver, words.count)))
            let text = fresh.joined(separator: " ")
            let still = Date().timeIntervalSince(stream.changedAt) > pause
            let ready = !fresh.isEmpty && (still || stream.ended || done(text))
            if ready { stream.handedOver = words.count }
            let over = stream.ended && fresh.isEmpty
            lock.unlock()
            if ready {
                debug("heard \(text.debugDescription)")
                return text
            }
            if over { return nil }
            if Date() >= deadline { return nil }
        }
    }

    /// A transcript as words, a fast count written as one number ("123") counted as its digits'
    /// words, so a rewrite of "один два три" to "123" does not hide the words after it.
    fileprivate static func words(_ transcript: String) -> [String] {
        transcript.split(whereSeparator: { $0.isWhitespace }).flatMap { token -> [String] in
            let word = String(token)
            let digits = word.compactMap { $0.wholeNumberValue }
            let rising = digits.count >= 2 && digits.count == word.count && zip(digits, digits.dropFirst()).allSatisfy { $1 > $0 }
            return rising ? digits.map(String.init) : [word]
        }
    }

    private func debug(_ line: String) {
        #if DEBUG
        FileHandle.standardError.write(("speech: " + line + "\n").data(using: .utf8)!)
        #endif
    }

    /// The tutor is about to speak: what is being recognised now is dropped, and a fresh request
    /// starts after it. The microphone itself keeps running.
    func cancelListening() {
        lock.lock()
        cancelGeneration += 1
        let old = stream
        stream = nil
        lock.unlock()
        old?.request.endAudio()
        old?.task?.cancel()
    }
}

/// One audio session for the tutor's voice and the microphone: play and record at once, out of
/// the loudspeaker (not the earpiece), Bluetooth allowed.
enum AudioSession {
    private static var configured = false
    private static let lock = NSLock()
    nonisolated(unsafe) private static var speakingUntil: Date?
    nonisolated(unsafe) private static var speaking = false

    /// The tutor's voice is playing (or ended a moment ago, while its echo dies away): the
    /// microphone's audio is the tutor, not the child.
    static var isTutorSpeaking: Bool {
        lock.lock(); defer { lock.unlock() }
        return speaking || (speakingUntil.map { Date() < $0 } ?? false)
    }

    static func tutorSpeaking(_ on: Bool) {
        lock.lock(); defer { lock.unlock() }
        speaking = on
        if !on { speakingUntil = Date().addingTimeInterval(0.3) }
    }

    static func configure() {
        lock.lock(); defer { lock.unlock() }
        guard !configured else { return }
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker, .allowBluetoothHFP, .allowBluetoothA2DP])
            try session.setActive(true)
            configured = true
        } catch {
            print("audio session: \(error)")
        }
    }
}
