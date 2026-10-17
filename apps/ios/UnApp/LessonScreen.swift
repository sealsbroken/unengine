import SwiftUI

/// The lesson list, then the lesson: the tutor's line, status, the camera's picture, and the
/// number pad. Perception runs through the shared Rust models (`UnPerception`); speech through
/// Apple's APIs. The list is whatever lessons the linked library offers (`availableStrategies`).
struct LessonScreen: View {
    @StateObject private var model = LessonModel()

    var body: some View {
        Group {
            if model.running {
                lesson
            } else {
                LessonList(model: model)
            }
        }
        .onAppear { model.reloadLessons(); Permissions.request {} }
        .onDisappear { model.close() }
    }

    private var lesson: some View {
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
                Button("End lesson") { model.end() }
                Button("Repeat") { model.repeatLine() }
                Button("Skip") { model.skip() }
                // The tutor's language; a running lesson starts over in the new one.
                Picker("Language", selection: $model.language) {
                    Text("EN").tag(UnLanguage.english)
                    Text("RU").tag(UnLanguage.russian)
                }.pickerStyle(.segmented).frame(width: 100)
            }.buttonStyle(.bordered).padding(.bottom)
        }
    }
}

/// Every lesson the app offers, in the library's order; a tap starts one. The last lesson
/// played is ticked.
struct LessonList: View {
    @ObservedObject var model: LessonModel

    var body: some View {
        NavigationStack {
            List(model.lessons, id: \.id) { lesson in
                Button { model.start(lesson.id) } label: {
                    HStack {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(lesson.title).font(.headline)
                            Text(lesson.description).font(.subheadline).foregroundStyle(.secondary)
                        }
                        Spacer()
                        if lesson.id == model.lessonId {
                            Image(systemName: "checkmark").foregroundStyle(.tint)
                        }
                    }
                }
                .foregroundStyle(.primary)
            }
            .overlay {
                if model.lessons.isEmpty { Text(model.language == .russian ? "Уроков нет" : "No lessons").foregroundStyle(.secondary) }
            }
            .navigationTitle(model.language == .russian ? "Уроки" : "Lessons")
            .toolbar {
                // The tutor's language for the next lesson.
                Picker("Language", selection: $model.language) {
                    Text("EN").tag(UnLanguage.english)
                    Text("RU").tag(UnLanguage.russian)
                }.pickerStyle(.segmented).frame(width: 100)
            }
        }
    }
}

@MainActor
final class LessonModel: ObservableObject {
    @Published var snapshot: UnSnapshot?
    @Published var running = false
    private var session: UnSession?
    private var perception: UnPerception?
    #if DEBUG
    nonisolated(unsafe) static var debugFrames = 0
    nonisolated(unsafe) static var debugWritten = 0.0
    #endif
    @Published private(set) var camera: CameraSource?
    /// The latest table state and the size of the frame it came from, for the overlay.
    @Published private(set) var table: UnTableState?
    @Published private(set) var frameSize: CGSize = .zero
    /// The tutor's language, remembered between runs; the device's language the first time.
    /// Changing it during a lesson starts the lesson over in the new language.
    @Published var language: UnLanguage = LessonModel.savedLanguage() {
        didSet {
            UserDefaults.standard.set(language == .russian ? "ru" : "en", forKey: "language")
            if running && language != oldValue { restart() }
        }
    }
    private var timer: Timer?
    /// The lessons the library offers, in its order.
    @Published private(set) var lessons: [UnStrategyInfo] = []
    /// The lesson last started, remembered between runs.
    @Published private(set) var lessonId: String = UserDefaults.standard.string(forKey: "lesson") ?? ""

    func reloadLessons() {
        lessons = availableStrategies()
    }

    private static func savedLanguage() -> UnLanguage {
        switch UserDefaults.standard.string(forKey: "language") {
        case "ru": return .russian
        case "en": return .english
        default: return Locale.current.language.languageCode?.identifier == "ru" ? .russian : .english
        }
    }

    var statusLine: String {
        let title = lessons.first { $0.id == lessonId }?.title ?? lessonId
        guard let s = snapshot else { return title }
        return "\(title)  \(s.status)  level \(s.level + 1)/\(s.levelCount)  \(s.correct)✓/\(s.tasksCompleted)"
    }

    /// Starts lesson `id` (the last one when `nil`, else the first in the list).
    func start(_ id: String? = nil) {
        if lessons.isEmpty { reloadLessons() }
        if let id { lessonId = id }
        guard let lesson = lessons.first(where: { $0.id == lessonId }) ?? lessons.first else { return }
        lessonId = lesson.id
        UserDefaults.standard.set(lesson.id, forKey: "lesson")
        let modelsDir = Bundle.main.resourceURL!.appendingPathComponent("models").path
        var config = defaultSessionConfig()
        config.language = language
        #if DEBUG
        // A debug build skips the introductions (the child's name, the tutor's) and goes straight
        // to the lesson: a known child is greeted by name and asked only whether they are ready.
        config.childName = language == .russian ? "Вася" : "Mia"
        #endif
        do {
            perception = try UnPerception(config: UnPerceptionConfig(
                modelsDir: modelsDir, detector: "toys", confidence: 0.25, countLabel: nil, roi: nil))
            // The microphone stays open for the whole lesson, so the child can answer at any time.
            session = try UnSession(
                strategyId: lesson.id, config: config,
                speechInput: AppleSpeechInput(), speechOutput: AppleSpeechOutput(), chime: true,
                alwaysListen: true)
        } catch {
            print("start failed: \(error)")
            return
        }
        camera = CameraSource { [weak self] y, uv, w, h, ys, uvs, t in
            guard let self, let perception = self.perception else { return }
            let began = Date()
            let result = Result { try perception.processNv12(y: y, uv: uv, width: w, height: h, yStride: ys, uvStride: uvs, timestamp: t) }
            #if DEBUG
            if Self.debugFrames < 5 || Self.debugFrames % 50 == 0 {
                print("frame \(Self.debugFrames): \(String(format: "%.0f", Date().timeIntervalSince(began) * 1000)) ms \(result.map { "raw=\($0.rawCount)" })")
            }
            if case .failure = result { Self.debugFrames += 1 }
            #endif
            if let state = try? result.get() {
                self.session?.observe(count: state.count, isStable: state.isStable, groups: state.groups)
                Task { @MainActor in
                    self.table = state
                    self.frameSize = CGSize(width: CGFloat(w), height: CGFloat(h))
                }
                #if DEBUG
                // Next to the frames: what the detector made of the latest one.
                Self.debugFrames += 1
                if let dir = CameraSource.framesDirectory, t - Self.debugWritten > 2 {
                    Self.debugWritten = t
                    let lines = state.detections.map { d in
                        String(format: "%@ %.2f  %.3f %.3f %.3f %.3f", d.label ?? "-", d.confidence, d.bbox.x1, d.bbox.y1, d.bbox.x2, d.bbox.y2)
                    }
                    let text = "t=\(t) fps=\(String(format: "%.1f", Double(Self.debugFrames) / t)) frame=\(w)x\(h) count=\(String(describing: state.count)) raw=\(state.rawCount)\n" + lines.joined(separator: "\n")
                    try? text.write(to: dir.appendingPathComponent("detections.txt"), atomically: true, encoding: .utf8)
                }
                #endif
            }
        }
        camera?.start()
        session?.start()
        running = true
        // The phone lies on the table for minutes with nobody touching it: no auto-lock while
        // a lesson runs.
        UIApplication.shared.isIdleTimerDisabled = true
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.snapshot = self?.session?.snapshot() }
        }
    }

    func stop() {
        session?.stop()
        running = false
        UIApplication.shared.isIdleTimerDisabled = false
    }
    func restart() { close(); start() }
    /// Ends the lesson and goes back to the list.
    func end() {
        session?.stop()
        close()
        running = false
        snapshot = nil
    }
    func tap(_ n: UInt32) { session?.tap(number: n) }
    func repeatLine() { session?.`repeat`() }
    func skip() { session?.skip() }

    func close() {
        UIApplication.shared.isIdleTimerDisabled = false
        timer?.invalidate()
        camera?.stop()
        camera = nil
        table = nil
        session?.shutdown()
        session = nil
    }
}
