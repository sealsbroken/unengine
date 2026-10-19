import AVFoundation
import Speech

/// One audio engine for the tutor's voice and the microphone, with Apple's voice processing
/// (the echo cancellation FaceTime uses): the tutor's voice, played through this engine, is
/// taken out of what the microphone hears, so the microphone stays open while the tutor speaks
/// and the child can answer over a question (barge-in). Echo cancellation only knows what plays
/// through its own engine, hence the synthesiser's audio is rendered here, not by the system.
/// Without voice processing (the simulator) the microphone is closed while the tutor speaks.
final class AudioIO: @unchecked Sendable {
    static let shared = AudioIO()

    let engine = AVAudioEngine()
    private let player = AVAudioPlayerNode()
    /// What the player plays; the synthesiser's buffers are converted to it.
    let playerFormat = AVAudioFormat(standardFormatWithSampleRate: 22050, channels: 1)!
    private let lock = NSLock()
    private var built = false
    private var tapInstalled = false
    private var microphoneOn = false
    /// The tutor's voice is taken out of the microphone's audio.
    private(set) var echoCancelled = false
    /// Receives the microphone's audio.
    var onInput: ((AVAudioPCMBuffer) -> Void)?
    /// The tutor starts a line.
    var onLine: (() -> Void)?

    /// Builds the engine once and starts it (again after an interruption stopped it). False when
    /// it cannot run at all.
    @discardableResult
    func start() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if built && engine.isRunning { return true }
        AudioSession.configure()
        let input = engine.inputNode
        #if os(macOS)
        useMicrophone(input)
        #endif
        if !built {
            // Voice processing must be switched on before the engine starts; it fails where there
            // is no voice-processing hardware path (the simulator), and then the old way stays.
            // Not on the Mac: there it initialises, then the engine's output fails to start
            // (-10875, a MacBook Air's own speakers and microphone), and switched off again it
            // leaves the output silent.
            #if os(iOS)
            do {
                try input.setVoiceProcessingEnabled(true)
                echoCancelled = true
                // Nothing else plays: no need to turn other audio down.
                input.voiceProcessingOtherAudioDuckingConfiguration =
                    AVAudioVoiceProcessingOtherAudioDuckingConfiguration(enableAdvancedDucking: false, duckingLevel: .min)
            } catch {
                print("voice processing unavailable: \(error)")
            }
            #endif
            engine.attach(player)
            engine.connect(player, to: engine.mainMixerNode, format: playerFormat)
            built = true
        }
        // The tap is set up again for the input's format as it is now (it changes with a
        // reconfigured session).
        if tapInstalled {
            input.removeTap(onBus: 0)
            tapInstalled = false
        }
        let format = input.outputFormat(forBus: 0)
        // No microphone (the simulator, say): installing a tap would raise an Objective-C
        // exception, which cannot pass through the engine's Rust frames.
        microphoneOn = format.channelCount > 0 && format.sampleRate > 0
        if microphoneOn {
            input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
                guard let self else { return }
                if !self.echoCancelled && AudioSession.isTutorSpeaking { return }
                self.onInput?(buffer)
            }
            tapInstalled = true
        }
        engine.prepare()
        do {
            try engine.start()
        } catch {
            print("audio engine: \(error)")
            // Voice processing joins the microphone and the speaker in one unit: when it cannot
            // start (no microphone access, a device it does not support), neither can the
            // tutor's voice. Without it the tutor still speaks, and the microphone closes while
            // it does, as on the simulator.
            guard echoCancelled else { return false }
            if tapInstalled {
                input.removeTap(onBus: 0)
                tapInstalled = false
            }
            do {
                try input.setVoiceProcessingEnabled(false)
            } catch {
                print("voice processing off: \(error)")
                return false
            }
            echoCancelled = false
            speechDebug("voice processing failed to start; running without echo cancellation")
            lock.unlock()
            defer { lock.lock() }
            return start()
        }
        speechDebug("permissions: camera \(AVCaptureDevice.authorizationStatus(for: .video).rawValue), microphone \(AVCaptureDevice.authorizationStatus(for: .audio).rawValue), speech \(SFSpeechRecognizer.authorizationStatus().rawValue) (3 = granted, 2 = denied, 0 = not asked)")
        speechDebug("audio started (microphone \(microphoneOn ? "\(format.sampleRate) Hz" : "none"), echo cancellation \(echoCancelled ? "on" : "off"))")
        return true
    }

    var hasMicrophone: Bool {
        lock.lock(); defer { lock.unlock() }
        return microphoneOn && engine.isRunning
    }

    /// Plays one synthesised buffer; `done` once it has been heard.
    func play(_ buffer: AVAudioPCMBuffer, done: @escaping () -> Void) {
        player.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { _ in done() }
        if !player.isPlaying { player.play() }
    }

    func stopPlaying() { player.stop() }

    #if os(macOS)
    /// Points the engine's input at the microphone picked in Settings (else the default input).
    /// Only while the engine is stopped.
    private func useMicrophone(_ input: AVAudioInputNode) {
        guard var device = MediaDevices.microphoneDeviceID(), let unit = input.audioUnit else { return }
        let status = AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                          &device, UInt32(MemoryLayout<AudioDeviceID>.size))
        if status != noErr { print("microphone \(device): \(status)") }
    }

    /// Settings picked another microphone: a running engine stops and starts again on it.
    func microphoneChanged() {
        lock.lock()
        let running = built && engine.isRunning
        if running { engine.stop() }
        lock.unlock()
        if running { start() }
    }
    #endif
}

/// The tutor's voice through AVSpeechSynthesizer, rendered into the shared audio engine (see
/// [`AudioIO`]). `speak` blocks the engine's effect thread until the line has been heard or stopped.
final class AppleSpeechOutput: NSObject, SpeechOutputCallback, @unchecked Sendable {
    private let synthesizer = AVSpeechSynthesizer()
    private let lock = NSLock()
    private var line: Line?
    private var converters: [String: AVAudioConverter] = [:]

    /// One line being said: done once the synthesiser has written it all and every buffer has
    /// been played, or when stopped.
    private final class Line {
        let finished = DispatchSemaphore(value: 0)
        var pending = 0
        var written = false
        var over = false
    }

    func prepare(language: UnLanguage) throws {
        AudioIO.shared.start()
        #if os(iOS)
        if language == .russian { PiperVoice.russian.load() }
        #endif
    }

    func speak(text: String, language: UnLanguage) throws {
        #if os(iOS)
        // Russian in the bundled Piper voice; Apple's voice when it is missing or says nothing.
        if language == .russian, let buffer = PiperVoice.russian.synthesize(text) {
            say(buffer, text: text)
            return
        }
        #endif
        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = AVSpeechSynthesisVoice(language: language == .russian ? "ru-RU" : "en-US")
        // The voice's own pace (speed 1).
        utterance.rate = AVSpeechUtteranceDefaultSpeechRate
        // A child's voice: the system has no child voice for Russian, so the pitch goes up.
        utterance.pitchMultiplier = 1.45
        let line = Line()
        lock.lock(); self.line = line; lock.unlock()
        AudioIO.shared.onLine?()
        AudioSession.tutorSpeaking(true)
        defer { AudioSession.tutorSpeaking(false) }
        guard AudioIO.shared.start() else {
            speechDebug("say \"\(text)\": the audio engine did not start")
            return
        }
        speechDebug("say \"\(text)\" (voice \(utterance.voice?.identifier ?? "none"))")
        var buffers = 0
        // Not on the main thread: the app may be waiting there for the lesson to close, and this
        // line (a goodbye) would then never be said (it froze the lesson screen).
        do {
            synthesizer.write(utterance) { [weak self] buffer in
                guard let self else { return }
                guard let pcm = buffer as? AVAudioPCMBuffer, pcm.frameLength > 0 else {
                    speechDebug("said: \(buffers) buffers, the last \(type(of: buffer)) \(buffer.format)")
                    // The end of the line.
                    self.lock.lock(); line.written = true; self.lock.unlock()
                    self.check(line)
                    return
                }
                buffers += 1
                guard let converted = self.convert(pcm) else {
                    speechDebug("could not convert \(pcm.format)")
                    return
                }
                self.lock.lock(); line.pending += 1; self.lock.unlock()
                AudioIO.shared.play(converted) { [weak self] in
                    guard let self else { return }
                    self.lock.lock(); line.pending -= 1; self.lock.unlock()
                    self.check(line)
                }
            }
        }
        line.finished.wait()
    }

    /// Plays a line synthesised in full beforehand (the Piper voice) and waits until it has been heard or stopped.
    private func say(_ buffer: AVAudioPCMBuffer, text: String) {
        let line = Line()
        line.written = true
        line.pending = 1
        lock.lock(); self.line = line; lock.unlock()
        AudioIO.shared.onLine?()
        AudioSession.tutorSpeaking(true)
        defer { AudioSession.tutorSpeaking(false) }
        guard AudioIO.shared.start() else {
            speechDebug("say \"\(text)\": the audio engine did not start")
            return
        }
        speechDebug("say \"\(text)\" (Piper voice)")
        lock.lock(); let stopped = line.over; lock.unlock()
        if stopped { return }
        AudioIO.shared.play(buffer) { [weak self] in
            guard let self else { return }
            self.lock.lock(); line.pending -= 1; self.lock.unlock()
            self.check(line)
        }
        line.finished.wait()
    }

    /// Ends `line` once it has been written and played in full.
    private func check(_ line: Line) {
        lock.lock()
        let done = line.written && line.pending == 0 && !line.over
        if done { line.over = true }
        lock.unlock()
        if done { line.finished.signal() }
    }

    /// The synthesiser's buffer in the player's format.
    private func convert(_ buffer: AVAudioPCMBuffer) -> AVAudioPCMBuffer? {
        let target = AudioIO.shared.playerFormat
        if buffer.format == target { return buffer }
        let key = buffer.format.description
        lock.lock()
        let converter = converters[key] ?? AVAudioConverter(from: buffer.format, to: target)
        converters[key] = converter
        lock.unlock()
        guard let converter else { return nil }
        let capacity = AVAudioFrameCount(Double(buffer.frameLength) * target.sampleRate / buffer.format.sampleRate) + 64
        guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return nil }
        var given = false
        var error: NSError?
        converter.convert(to: out, error: &error) { _, status in
            if given {
                status.pointee = .noDataNow
                return nil
            }
            given = true
            status.pointee = .haveData
            return buffer
        }
        return error == nil ? out : nil
    }

    func stopSpeaking() {
        synthesizer.stopSpeaking(at: .immediate)
        AudioIO.shared.stopPlaying()
        lock.lock()
        let line = self.line
        let stop = line.map { !$0.over } ?? false
        line?.over = true
        lock.unlock()
        if stop { line?.finished.signal() }
    }

    func playChime() { AudioServicesPlaySystemSound(1113) }
}

/// The child's words through SFSpeechRecognizer (on the device when the language allows).
/// `listenForNumber` returns as soon as a number is in the transcript; `listenForText` returns the
/// new words once the child has paused. The engine parses numbers itself too (0...20), the local
/// parser only decides when to stop listening.
///
/// One recognition request runs continuously; each call hands over the words added since the
/// last one, once the transcript has stood still for a moment. Cutting a request at every pause
/// lost the word the child had already begun (a count finger by finger came out "один, три,
/// пять"), and starting the next one over again heard words twice. With echo cancellation the
/// request runs on through the tutor's lines (the child may answer over them); without it, it is
/// replaced when the tutor speaks. It is also replaced when the recogniser ends it, or after
/// about fifty quiet seconds (a request has a limit).
final class AppleSpeechInput: SpeechInputCallback, @unchecked Sendable {
    private let lock = NSLock()
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

    init() {
        AudioIO.shared.onInput = { [weak self] buffer in
            guard let self else { return }
            self.lock.lock(); let stream = self.stream; self.lock.unlock()
            stream?.request.append(buffer)
        }
        // Every tutor line starts a fresh request (it hears the child over the line all the
        // same): one request kept running through the lines, with what is left of the tutor's
        // voice in it, stopped handing over the child's words.
        AudioIO.shared.onLine = { [weak self] in
            guard let self else { return }
            self.lock.lock()
            let old = self.stream
            self.stream = nil
            self.lock.unlock()
            old?.request.endAudio()
            old?.task?.cancel()
        }
    }

    func prepare(language: UnLanguage) throws {
        let status = SFSpeechRecognizer.authorizationStatus()
        guard status == .authorized || status == .notDetermined else {
            throw UnError.Failed(reason: "speech recognition not authorised")
        }
        if status == .notDetermined { SFSpeechRecognizer.requestAuthorization { _ in } }
        AVAudioApplication.requestRecordPermission { _ in }
        AudioIO.shared.start()
    }

    /// The tutor's voice is taken out of the microphone: the child may answer over it.
    func bargeIn() -> Bool { AudioIO.shared.echoCancelled }

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
            speechDebug("the recogniser is not available")
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
                if text != fresh.transcript {
                    fresh.transcript = text
                    fresh.changedAt = Date()
                    speechDebug("partial: \(text.debugDescription)")
                }
                if result.isFinal { fresh.ended = true }
            }
            if let error {
                fresh.ended = true
                speechDebug("recognition ended: \(error.localizedDescription)")
            }
        }
        lock.lock(); stream = fresh; lock.unlock()
        speechDebug("listening")
        return fresh
    }

    /// Waits for new words: handed over once the transcript has stood still for `pause` (at once
    /// when `done` accepts them); nil on timeout, or when cancelled.
    private func listen(language: UnLanguage, timeoutSeconds: Double, done: @escaping (String) -> Bool) -> String? {
        lock.lock(); let generation = cancelGeneration; lock.unlock()
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        let gated = !AudioIO.shared.echoCancelled
        // Without echo cancellation, never start recognising over the tutor's own voice.
        while gated && AudioSession.isTutorSpeaking {
            if Date() >= deadline { return nil }
            Thread.sleep(forTimeInterval: 0.05)
        }
        guard AudioIO.shared.start(), AudioIO.shared.hasMicrophone else {
            speechDebug("the microphone did not start")
            Thread.sleep(forTimeInterval: 0.5)
            return nil
        }
        guard let stream = currentStream(language) else {
            speechDebug("no recogniser for \(language)")
            Thread.sleep(forTimeInterval: 0.5)
            return nil
        }
        while true {
            Thread.sleep(forTimeInterval: 0.1)
            lock.lock()
            // Cancelled, the tutor speaking (without echo cancellation), or a new line replaced
            // the request: the next listen begins on the new one.
            if cancelGeneration != generation || (gated && AudioSession.isTutorSpeaking) || self.stream !== stream {
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
                speechDebug("heard \(text.debugDescription)")
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

    /// The listen in progress ends. Without echo cancellation the tutor is about to speak: what
    /// is being recognised now is dropped and a fresh request starts after the line. With it,
    /// the request runs on (the child may be answering over the line).
    func cancelListening() {
        lock.lock()
        cancelGeneration += 1
        let old = AudioIO.shared.echoCancelled ? nil : stream
        if old != nil { stream = nil }
        lock.unlock()
        old?.request.endAudio()
        old?.task?.cancel()
    }
}

func speechDebug(_ line: String) {
    #if DEBUG
    FileHandle.standardError.write(("speech: " + line + "\n").data(using: .utf8)!)
    #endif
}

/// One audio session for the tutor's voice and the microphone: play and record at once, out of
/// the loudspeaker (not the earpiece), Bluetooth allowed.
enum AudioSession {
    private static var configured = false
    private static let lock = NSLock()
    nonisolated(unsafe) private static var speakingUntil: Date?
    nonisolated(unsafe) private static var speaking = false

    /// The tutor's voice is playing (or ended a moment ago, while its echo dies away). Without
    /// echo cancellation the microphone's audio is then the tutor, not the child.
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
        #if os(iOS)
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playAndRecord, mode: .default, options: [.defaultToSpeaker, .allowBluetoothHFP, .allowBluetoothA2DP])
            try session.setActive(true)
            configured = true
        } catch {
            print("audio session: \(error)")
        }
        #else
        configured = true  // the Mac has no audio session: the default input and output are used
        #endif
    }
}
