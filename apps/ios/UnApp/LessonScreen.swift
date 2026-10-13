import SwiftUI

/// One screen: camera preview placeholder, the tutor's line, status, and the number pad.
/// Perception runs through the shared Rust models (`UnPerception`); speech through Apple's APIs.
struct LessonScreen: View {
    @StateObject private var model = LessonModel()

    var body: some View {
        VStack(spacing: 12) {
            Text(model.snapshot?.lastSpoken ?? "…").font(.title2).multilineTextAlignment(.center).padding()
            Text(model.statusLine).font(.footnote).foregroundStyle(.secondary)
            Spacer()
            if model.snapshot?.showNumberPad == true {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 5)) {
                    ForEach(0...10, id: \.self) { n in
                        Button("\(n)") { model.tap(UInt32(n)) }.buttonStyle(.borderedProminent).font(.title2)
                    }
                }.padding()
            }
            HStack {
                Button(model.running ? "End lesson" : "Start lesson") { model.running ? model.stop() : model.start() }
                Button("Repeat") { model.repeat() }
                Button("Skip") { model.skip() }
            }.buttonStyle(.bordered).padding(.bottom)
        }
        .onDisappear { model.close() }
    }
}

@MainActor
final class LessonModel: ObservableObject {
    @Published var snapshot: UnSnapshot?
    @Published var running = false
    private var session: UnSession?
    private var perception: UnPerception?
    private var camera: CameraSource?
    private var timer: Timer?

    var statusLine: String {
        guard let s = snapshot else { return "idle" }
        return "\(s.status)  level \(s.level + 1)/\(s.levelCount)  \(s.correct)✓/\(s.tasksCompleted)"
    }

    func start() {
        let modelsDir = Bundle.main.resourceURL!.appendingPathComponent("models").path
        var config = defaultSessionConfig()
        config.language = Locale.current.language.languageCode?.identifier == "ru" ? .russian : .english
        do {
            perception = try UnPerception(config: UnPerceptionConfig(
                modelsDir: modelsDir, detector: "detector", confidence: 0.25, hands: true, countLabel: nil, mirrored: false, roi: nil))
            session = try UnSession(
                strategyId: availableStrategies().first?.id ?? "demo", config: config,
                speechInput: AppleSpeechInput(), speechOutput: AppleSpeechOutput(), chime: true)
        } catch {
            print("start failed: \(error)")
            return
        }
        camera = CameraSource { [weak self] y, uv, w, h, ys, uvs, t in
            guard let self, let perception = self.perception else { return }
            if let state = try? perception.processNv12(y: y, uv: uv, width: w, height: h, yStride: ys, uvStride: uvs, timestamp: t) {
                self.session?.observe(count: state.count, handsPresent: state.handsPresent, isStable: state.isStable, fingerCount: state.fingerCount)
            }
        }
        camera?.start()
        session?.start()
        running = true
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.snapshot = self?.session?.snapshot() }
        }
    }

    func stop() { session?.stop(); running = false }
    func tap(_ n: UInt32) { session?.tap(number: n) }
    func repeat() { session?.repeat() }
    func skip() { session?.skip() }

    func close() {
        timer?.invalidate()
        camera?.stop()
        session?.close()
        session = nil
    }
}
