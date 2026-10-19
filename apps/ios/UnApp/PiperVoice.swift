#if os(iOS)
import AVFoundation

/// The Russian tutor's voice: a Piper voice (VITS) through sherpa-onnx's C library, bundled as
/// `models/voice-ru` (`model.onnx`, `tokens.txt`, `espeak-ng-data`; tools/ios.sh stages it).
/// Loaded once, on first use, and shared by every lesson. Without it in the bundle the tutor keeps
/// Apple's voice.
final class PiperVoice: @unchecked Sendable {
    static let russian = PiperVoice(folder: "models/voice-ru")

    private let folder: String
    private let lock = NSLock()
    private var tts: OpaquePointer?
    private var tried = false

    init(folder: String) { self.folder = folder }

    /// Loads the voice the first time (about a second); false when it is not in the bundle or
    /// does not load.
    @discardableResult
    func load() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if tried { return tts != nil }
        tried = true
        guard let model = Bundle.main.path(forResource: "model", ofType: "onnx", inDirectory: folder),
              let tokens = Bundle.main.path(forResource: "tokens", ofType: "txt", inDirectory: folder),
              let data = Bundle.main.resourcePath.map({ "\($0)/\(folder)/espeak-ng-data" }),
              FileManager.default.fileExists(atPath: data) else {
            speechDebug("no Piper voice in \(folder)")
            return false
        }
        let strings = [model, tokens, data, "cpu"].map { strdup($0) }
        defer { strings.forEach { free($0) } }
        var config = SherpaOnnxOfflineTtsConfig()
        config.model.vits.model = UnsafePointer(strings[0])
        config.model.vits.tokens = UnsafePointer(strings[1])
        config.model.vits.data_dir = UnsafePointer(strings[2])
        // As on the desktop (un-speech's Voice).
        config.model.vits.noise_scale = 0.667
        config.model.vits.noise_scale_w = 0.8
        config.model.vits.length_scale = 1.0
        config.model.num_threads = 2
        config.model.provider = UnsafePointer(strings[3])
        tts = SherpaOnnxCreateOfflineTts(&config)
        speechDebug(tts == nil ? "the Piper voice in \(folder) did not load" : "Piper voice: \(folder)")
        return tts != nil
    }

    /// `text` said in this voice, in the player's format; nil when there is no voice or nothing came out.
    func synthesize(_ text: String) -> AVAudioPCMBuffer? {
        guard load() else { return nil }
        lock.lock(); defer { lock.unlock() }
        guard let tts, let audio = SherpaOnnxOfflineTtsGenerate(tts, text, 0, 1.0) else { return nil }
        defer { SherpaOnnxDestroyOfflineTtsGeneratedAudio(audio) }
        let generated = audio.pointee
        let format = AudioIO.shared.playerFormat
        guard generated.n > 0, let samples = generated.samples, Double(generated.sample_rate) == format.sampleRate,
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(generated.n)),
              let channel = buffer.floatChannelData?[0] else { return nil }
        channel.update(from: samples, count: Int(generated.n))
        buffer.frameLength = AVAudioFrameCount(generated.n)
        return buffer
    }
}
#endif
