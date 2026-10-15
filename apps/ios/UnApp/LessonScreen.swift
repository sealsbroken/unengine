import SwiftUI

/// One screen: the tutor's line, status, the camera's picture, and the number pad.
/// Perception runs through the shared Rust models (`UnPerception`); speech through Apple's APIs.
struct LessonScreen: View {
    @StateObject private var model = LessonModel()

    var body: some View {
        VStack(spacing: 12) {
            Text(model.snapshot?.lastSpoken ?? "…").font(.title2).multilineTextAlignment(.center).padding()
            Text(model.statusLine).font(.footnote).foregroundStyle(.secondary)
            if let camera = model.camera {
                ZStack {
                    CameraPreview(session: camera.session)
                    TableOverlay(table: model.table, frameSize: model.frameSize, highlight: model.snapshot?.highlightCount)
                }
                .clipShape(RoundedRectangle(cornerRadius: 16))
                .padding(.horizontal)
            } else {
                Spacer()
            }
            if model.snapshot?.showNumberPad == true {
                LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 5)) {
                    ForEach(0...10, id: \.self) { n in
                        Button("\(n)") { model.tap(UInt32(n)) }.buttonStyle(.borderedProminent).font(.title2)
                    }
                }.padding()
            }
            HStack {
                Button(model.running ? "End lesson" : "Start lesson") { model.running ? model.stop() : model.start() }
                Button("Repeat") { model.repeatLine() }
                Button("Skip") { model.skip() }
            }.buttonStyle(.bordered).padding(.bottom)
        }
        // The lesson starts by itself once the permissions are answered; the button remains for
        // starting over after "End lesson".
        .onAppear { Permissions.request { if !model.running { model.start() } } }
        .onDisappear { model.close() }
    }
}

@MainActor
final class LessonModel: ObservableObject {
    @Published var snapshot: UnSnapshot?
    @Published var running = false
    private var session: UnSession?
    private var perception: UnPerception?
    @Published private(set) var camera: CameraSource?
    /// The latest table state and the size of the frame it came from, for the overlay.
    @Published private(set) var table: UnTableState?
    @Published private(set) var frameSize: CGSize = .zero
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
                modelsDir: modelsDir, detector: "caps", confidence: 0.25, hands: true, countLabel: nil, mirrored: false, roi: nil))
            let strategies = availableStrategies()
            let lesson = strategies.first { $0.id == "lesson-01" } ?? strategies.first
            session = try UnSession(
                strategyId: lesson?.id ?? "demo", config: config,
                speechInput: AppleSpeechInput(), speechOutput: AppleSpeechOutput(), chime: true)
        } catch {
            print("start failed: \(error)")
            return
        }
        camera = CameraSource { [weak self] y, uv, w, h, ys, uvs, t in
            guard let self, let perception = self.perception else { return }
            if let state = try? perception.processNv12(y: y, uv: uv, width: w, height: h, yStride: ys, uvStride: uvs, timestamp: t) {
                self.session?.observe(
                    count: state.count, handsPresent: state.handsPresent, isStable: state.isStable, fingerCount: state.fingerCount,
                    groups: state.groups, pointedAt: state.pointedAt)
                Task { @MainActor in
                    self.table = state
                    self.frameSize = CGSize(width: CGFloat(w), height: CGFloat(h))
                }
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
    func repeatLine() { session?.`repeat`() }
    func skip() { session?.skip() }

    func close() {
        timer?.invalidate()
        camera?.stop()
        camera = nil
        table = nil
        session?.close()
        session = nil
    }
}
