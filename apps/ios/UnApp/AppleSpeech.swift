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
final class AppleSpeechInput: SpeechInputCallback {
    private let audioEngine = AVAudioEngine()
    private var task: SFSpeechRecognitionTask?
    private let lock = NSLock()
    private var cancelled = false
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

    /// Records until `done` accepts a partial transcript, the child pauses after saying something,
    /// the recogniser finishes, or the timeout; returns the transcript, nil when nothing was said.
    private func listen(language: UnLanguage, timeoutSeconds: Double, done: @escaping (String) -> Bool) -> String? {
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: language == .russian ? "ru-RU" : "en-US")),
              recognizer.isAvailable else { return nil }
        AudioSession.configure()
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = recognizer.supportsOnDeviceRecognition
        request.taskHint = .dictation
        let input = audioEngine.inputNode
        let format = input.outputFormat(forBus: 0)
        // No microphone (the simulator, say): installing a tap would raise an Objective-C
        // exception, which cannot pass through the engine's Rust frames.
        guard format.channelCount > 0, format.sampleRate > 0 else { return nil }
        input.removeTap(onBus: 0)
        input.installTap(onBus: 0, bufferSize: 2048, format: format) { buffer, _ in request.append(buffer) }
        audioEngine.prepare()
        guard (try? audioEngine.start()) != nil else { return nil }
        defer {
            audioEngine.stop()
            input.removeTap(onBus: 0)
        }
        lock.lock(); cancelled = false; lock.unlock()
        let finished = DispatchSemaphore(value: 0)
        let state = NSLock()
        var transcript = ""
        var changedAt = Date()
        var over = false
        task = recognizer.recognitionTask(with: request) { result, error in
            state.lock(); defer { state.unlock() }
            if let result = result {
                let text = result.bestTranscription.formattedString
                if text != transcript { transcript = text; changedAt = Date() }
                if result.isFinal || (!text.isEmpty && done(text)) { over = true; finished.signal() }
            }
            if error != nil { over = true; finished.signal() }
        }
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while true {
            if finished.wait(timeout: .now() + 0.1) == .success { break }
            state.lock()
            let paused = !transcript.isEmpty && Date().timeIntervalSince(changedAt) > pause
            let stop = over || paused || Date() >= deadline
            state.unlock()
            if stop { break }
        }
        task?.cancel()
        request.endAudio()
        lock.lock(); let wasCancelled = cancelled; lock.unlock()
        state.lock(); let text = transcript.trimmingCharacters(in: .whitespacesAndNewlines); state.unlock()
        return wasCancelled || text.isEmpty ? nil : text
    }

    func cancelListening() {
        lock.lock(); cancelled = true; lock.unlock()
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
