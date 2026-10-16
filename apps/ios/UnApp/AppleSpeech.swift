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
    private var sampleRate: Double = 48_000
    /// The last few seconds of audio, by sample time: an utterance is handed over a moment after
    /// the child stopped, and the next word may already have begun ("один" ... "два" said 1.5 s
    /// later, while the recogniser still shows "один"). The next request starts from the end of
    /// the last word handed over, so that word is neither lost nor heard twice.
    private var recent: [(time: AVAudioFramePosition, buffer: AVAudioPCMBuffer)] = []
    private let recentSeconds: Double = 4
    /// Where the next request should start: the end of the last word handed over, in sample time;
    /// nil after a cancel (the tutor spoke) or a timeout, when nothing is carried over.
    private var resumeAt: AVAudioFramePosition?
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
        // The tutor's voice can reconfigure the audio session, which stops the engine; the tap is
        // set up again for the input's format as it is now.
        if tapInstalled {
            input.removeTap(onBus: 0)
            tapInstalled = false
        }
        recent.removeAll()
        resumeAt = nil
        let format = input.outputFormat(forBus: 0)
        // No microphone (the simulator, say): installing a tap would raise an Objective-C
        // exception, which cannot pass through the engine's Rust frames.
        guard format.channelCount > 0, format.sampleRate > 0 else { return false }
        sampleRate = format.sampleRate
        debug("microphone started (\(format.sampleRate) Hz)")
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, when in
            self?.heard(buffer, at: when.sampleTime)
        }
        tapInstalled = true
        audioEngine.prepare()
        return (try? audioEngine.start()) != nil
    }

    /// One buffer from the microphone: kept for a few seconds, and fed to the request if any.
    private func heard(_ buffer: AVAudioPCMBuffer, at time: AVAudioFramePosition) {
        guard let copy = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: buffer.frameLength) else { return }
        copy.frameLength = buffer.frameLength
        if let from = buffer.floatChannelData, let to = copy.floatChannelData {
            for channel in 0..<Int(buffer.format.channelCount) {
                to[channel].update(from: from[channel], count: Int(buffer.frameLength))
            }
        } else {
            return
        }
        lock.lock()
        recent.append((time, copy))
        let oldest = time - AVAudioFramePosition(recentSeconds * sampleRate)
        while let first = recent.first, first.time < oldest { recent.removeFirst() }
        let request = self.request
        lock.unlock()
        request?.append(copy)
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
        var wordsEnd: TimeInterval = 0
        var changedAt = Date()
        var over = false
        let task = recognizer.recognitionTask(with: request) { result, error in
            state.lock(); defer { state.unlock() }
            if let result = result {
                let text = result.bestTranscription.formattedString
                if text != transcript { transcript = text; changedAt = Date() }
                if let last = result.bestTranscription.segments.last { wordsEnd = last.timestamp + last.duration }
                if result.isFinal || (!text.isEmpty && done(text)) { over = true; finished.signal() }
            }
            if let error { self.debug("recognition ended: \(error.localizedDescription)"); over = true; finished.signal() }
        }
        // The request's audio starts where the last utterance's words ended (what was said since
        // is in `recent`), or now.
        lock.lock()
        let carried = resumeAt.map { start in recent.filter { $0.time >= start } } ?? []
        let requestStart = carried.first?.time ?? recent.last.map { $0.time + AVAudioFramePosition($0.buffer.frameLength) } ?? 0
        for item in carried { request.append(item.buffer) }
        self.request = request
        self.task = task
        cancelled = false
        resumeAt = nil
        lock.unlock()
        if !carried.isEmpty { debug("carried over \(String(format: "%.2f", Double(carried.count) * 1024 / sampleRate)) s") }
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
        state.lock(); let text = transcript.trimmingCharacters(in: .whitespacesAndNewlines); let end = wordsEnd; state.unlock()
        lock.lock()
        if self.request === request { self.request = nil; self.task = nil }
        let wasCancelled = cancelled
        // The next request takes over from the end of these words (a little after, so the last
        // word is not heard twice): whatever the child began meanwhile is carried over.
        if !wasCancelled && !text.isEmpty {
            resumeAt = requestStart + AVAudioFramePosition((end + 0.15) * sampleRate)
        }
        lock.unlock()
        request.endAudio()
        task.cancel()
        if !text.isEmpty || wasCancelled { debug("heard \(text.debugDescription)\(wasCancelled ? " (cancelled)" : "")") }
        return wasCancelled || text.isEmpty ? nil : text
    }

    private func debug(_ line: String) {
        #if DEBUG
        FileHandle.standardError.write(("speech: " + line + "\n").data(using: .utf8)!)
        #endif
    }

    /// Ends the recognition in progress; the microphone itself keeps running. Nothing said before
    /// the next request is carried over (the tutor is about to speak).
    func cancelListening() {
        lock.lock()
        cancelled = true
        resumeAt = nil
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
