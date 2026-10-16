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
        utterance.rate = AVSpeechUtteranceDefaultSpeechRate * 0.9
        finished = DispatchSemaphore(value: 0)
        DispatchQueue.main.async { self.synthesizer.speak(utterance) }
        finished.wait()
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
/// transcript once the child has paused. The engine parses numbers itself too (0...20), the local
/// parser only decides when to stop listening.
///
/// The audio engine runs for the whole session; only the recognition requests come and go. A
/// child counting slowly, finger by finger, says the next number right after the last one was
/// handed over, and restarting the microphone for every utterance lost it ("один", then
/// nothing). Between requests (and while the tutor speaks) the audio goes nowhere.
final class AppleSpeechInput: SpeechInputCallback, @unchecked Sendable {
    private let audioEngine = AVAudioEngine()
    private let lock = NSLock()
    /// The request the microphone's audio goes to, if any.
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var cancelled = false
    private var tapInstalled = false
    /// How long the transcript must stay unchanged before the utterance counts as finished.
    private let pause: TimeInterval = 1.2

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
        let format = input.outputFormat(forBus: 0)
        // No microphone (the simulator, say): installing a tap would raise an Objective-C
        // exception, which cannot pass through the engine's Rust frames.
        guard format.channelCount > 0, format.sampleRate > 0 else { return false }
        if !tapInstalled {
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
                guard let self else { return }
                self.lock.lock(); let request = self.request; self.lock.unlock()
                request?.append(buffer)
            }
            tapInstalled = true
        }
        audioEngine.prepare()
        return (try? audioEngine.start()) != nil
    }

    /// Recognises until `done` accepts a partial transcript, the child pauses after saying
    /// something, the recogniser finishes, or the timeout; returns the transcript, nil when
    /// nothing was said.
    private func listen(language: UnLanguage, timeoutSeconds: Double, done: @escaping (String) -> Bool) -> String? {
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language == .russian ? "ru-RU" : "en-US")),
              recognizer.isAvailable, microphone() else { return nil }
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = recognizer.supportsOnDeviceRecognition
        request.taskHint = .dictation
        let finished = DispatchSemaphore(value: 0)
        let state = NSLock()
        var transcript = ""
        var changedAt = Date()
        var over = false
        let task = recognizer.recognitionTask(with: request) { result, error in
            state.lock(); defer { state.unlock() }
            if let result = result {
                let text = result.bestTranscription.formattedString
                if text != transcript { transcript = text; changedAt = Date() }
                if result.isFinal || (!text.isEmpty && done(text)) { over = true; finished.signal() }
            }
            if error != nil { over = true; finished.signal() }
        }
        lock.lock(); self.request = request; self.task = task; cancelled = false; lock.unlock()
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while true {
            if finished.wait(timeout: .now() + 0.1) == .success { break }
            state.lock()
            let paused = !transcript.isEmpty && Date().timeIntervalSince(changedAt) > pause
            let stop = over || paused || Date() >= deadline
            state.unlock()
            if stop { break }
            lock.lock(); let wasCancelled = cancelled; lock.unlock()
            if wasCancelled { break }
        }
        lock.lock()
        if self.request === request { self.request = nil; self.task = nil }
        let wasCancelled = cancelled
        lock.unlock()
        request.endAudio()
        task.cancel()
        state.lock(); let text = transcript.trimmingCharacters(in: .whitespacesAndNewlines); state.unlock()
        return wasCancelled || text.isEmpty ? nil : text
    }

    /// Ends the recognition in progress; the microphone itself keeps running.
    func cancelListening() {
        lock.lock()
        cancelled = true
        let task = self.task
        lock.unlock()
        task?.cancel()
    }
}

/// One audio session for the tutor's voice and the microphone: play and record at once, out of
/// the loudspeaker (not the earpiece), Bluetooth allowed.
enum AudioSession {
    private static var configured = false
    private static let lock = NSLock()

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
