import SwiftUI

/// The app: the children, a child's adventures (the trail), and the lesson over them. Perception
/// runs through the shared Rust models (`UnPerception`); speech through Apple's APIs. The lessons
/// are whatever the linked library offers (`availableStrategies`); each child's progress through
/// them is kept (`UnProfiles`). The look is the "Un Design Board" (Claude Design, Onym tokens).
struct LessonScreen: View {
    @StateObject private var model = LessonModel()

    var body: some View {
        Group {
            if model.child == nil {
                ChildrenScreen(model: model)
            } else {
                TrailScreen(model: model)
            }
        }
        .animation(.easeInOut(duration: 0.25), value: model.childId)
        // No swipe-back mid-lesson: the lesson leaves by Pause, "пауза" or its end.
        .lessonCover(isPresented: $model.running) { LessonView(model: model) }
        .onAppear {
            model.reloadLessons()
            model.reloadChildren()
            model.preparePerception()
            #if DEBUG
            // Screenshots in the simulator, where speech permission can't be granted from outside.
            if ProcessInfo.processInfo.environment["UN_SKIP_PERMISSIONS"] != nil {
                if ProcessInfo.processInfo.environment["UN_START_LESSON"] != nil { model.start() }
                return
            }
            #endif
            Permissions.request {}
        }
        .onDisappear { model.close() }
    }
}

@MainActor
final class LessonModel: ObservableObject {
    @Published var snapshot: UnSnapshot?
    @Published var running = false
    private var session: UnSession?
    /// Set from a background thread once the detector is loaded (its first CoreML compile takes
    /// about a minute; later launches load the cached one in seconds). Frames wait until then.
    nonisolated(unsafe) private var perception: UnPerception?
    /// The detector is still loading.
    @Published private(set) var preparingCamera = false
    /// The detector did not load; the lesson runs without the camera's count.
    @Published private(set) var detectorFailed = false
    #if DEBUG
    nonisolated(unsafe) static var debugFrames = 0
    nonisolated(unsafe) static var debugWritten = 0.0
    nonisolated(unsafe) static var debugFirstStable = -1.0
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

    /// The children and their progress, in Application Support (kept with the app's data).
    let profiles = UnProfiles(path: LessonModel.profilesPath())
    @Published private(set) var children: [UnProfile] = []
    /// The child the lessons are for, remembered between runs; nil shows the children.
    @Published var childId: String? = UserDefaults.standard.string(forKey: "child") {
        didSet { UserDefaults.standard.set(childId, forKey: "child") }
    }
    var child: UnProfile? { children.first { $0.id == childId } }
    /// The running lesson's end has been recorded for the child.
    private var recorded = false
    /// When the running lesson was completed (the end screens follow it).
    @Published private(set) var endedAt: Date?
    /// "No" to "Продолжим?" tapped (the goodbye screen).
    @Published private(set) var declined = false

    enum EndPhase { case celebrate, ask, bye }

    /// After a completed lesson: the stamped ticket (3 s), then "Продолжим?" (when there is a
    /// next lesson), then the goodbye after a no. A paused lesson goes back to the trail instead.
    var endPhase: EndPhase? {
        guard let s = snapshot, let summary = s.summary, !summary.paused, leavingSince == nil, !s.pauseRequested,
              let endedAt else { return nil }
        if declined || s.nextLessonDeclined { return .bye }
        if Date().timeIntervalSince(endedAt) < 3.5 { return .celebrate }
        return nextLessonId != nil ? .ask : .bye
    }

    /// The tutor's state for the panel and the orb.
    enum TutorState { case speaking, listening, watching, celebrating, muted }

    var tutorState: TutorState {
        guard let s = snapshot else { return .watching }
        if let c = s.celebratedSecondsAgo, c < 3 { return .celebrating }
        if s.showNumberPad { return .muted }
        switch s.status {
        case .speaking: return .speaking
        case .listening: return .listening
        default: return .watching
        }
    }

    /// The yes/no cards under "Продолжим?": the same as saying it.
    func answerNext(_ yes: Bool) {
        session?.answerNextLesson(yes: yes)
        if yes, let next = childId.flatMap({ profiles.nextLesson(id: $0) }) ?? nextLessonId {
            close(keepCamera: true)
            start(next)
        } else if !yes {
            declined = true
        }
    }

    /// "Back to children" after the goodbye.
    func goHome() {
        end()
        childId = nil
    }

    private static func profilesPath() -> String {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        return dir.appendingPathComponent("profiles.json").path
    }

    func reloadChildren() {
        children = profiles.list()
        if child == nil { childId = nil }
    }

    func addChild(_ name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        let added = profiles.add(name: name)
        reloadChildren()
        childId = added.id
    }

    func removeChild(_ id: String) {
        profiles.remove(id: id)
        reloadChildren()
    }

    /// The lesson `child` goes on with.
    func nextLesson(for child: UnProfile) -> UnStrategyInfo? {
        let id = profiles.nextLesson(id: child.id)
        return lessons.first { $0.id == id }
    }

    /// Loads the detector in the background, from the lesson list on: by the time a lesson
    /// starts the camera already counts the toys that are out (loaded at the start, the first
    /// question was asked before the camera saw anything).
    func preparePerception() {
        guard perception == nil, !preparingCamera else { return }
        // The compiled CoreML model is kept in Caches, which survives between launches.
        if let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first {
            setenv("UN_CACHE_DIR", caches.path, 1)
        }
        let modelsDir = Bundle.main.resourceURL!.appendingPathComponent("models").path
        preparingCamera = true
        detectorFailed = false
        let began = Date()
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            defer { print(String(format: "timing: detector loaded in %.1f s", Date().timeIntervalSince(began))) }
            let loaded = try? UnPerception(config: UnPerceptionConfig(
                modelsDir: modelsDir, detector: "toys", confidence: 0.25, countLabel: nil, roi: nil))
            if loaded == nil { print("the toy detector failed to load") }
            self?.perception = loaded
            let failed = loaded == nil
            Task { @MainActor in
                self?.preparingCamera = false
                self?.detectorFailed = failed
            }
        }
    }

    private static func savedLanguage() -> UnLanguage {
        switch UserDefaults.standard.string(forKey: "language") {
        case "ru": return .russian
        case "en": return .english
        // The device's own language (its first preferred one), not `Locale.current`: that one is
        // the app's localisation, English unless the app ships a Russian one.
        default: return (Locale.preferredLanguages.first ?? "").hasPrefix("ru") ? .russian : .english
        }
    }

    /// Starts lesson `id` for the chosen child (the one they go on with when `nil`).
    func start(_ id: String? = nil) {
        if lessons.isEmpty { reloadLessons() }
        if let id {
            lessonId = id
        } else if let childId, let next = profiles.nextLesson(id: childId) {
            lessonId = next
        }
        guard let lesson = lessons.first(where: { $0.id == lessonId }) ?? lessons.first else { return }
        lessonId = lesson.id
        UserDefaults.standard.set(lesson.id, forKey: "lesson")
        var config = defaultSessionConfig()
        config.language = language
        // The child's name (the tutor greets them and does not ask it) and what their lessons
        // remembered: each child goes on where they stopped.
        if let childId { config = profiles.sessionConfig(id: childId, base: config) }
        recorded = false
        endedAt = nil
        declined = false
        do {
            // The microphone stays open for the whole lesson, so the child can answer at any time.
            session = try UnSession(
                strategyId: lesson.id, config: config,
                speechInput: AppleSpeechInput(), speechOutput: AppleSpeechOutput(), chime: true,
                alwaysListen: true)
        } catch {
            print("start failed: \(error)")
            return
        }
        preparePerception()
        // One camera for the whole visit: the next lesson keeps it. (A new capture session started
        // while the old one was still stopping crashed AVFoundation as the preview moved over.)
        if camera == nil { camera = makeCamera() }
        camera?.start()
        // After a completed lesson the tutor asks "Продолжим?"; a yes starts the next one.
        session?.offerNextLesson(offer: nextLessonId != nil)
        session?.start()
        running = true
        // The phone lies on the table for minutes with nobody touching it: no auto-lock while
        // a lesson runs.
        ScreenAwake.set(true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.snapshot = self.session?.snapshot()
                // The lesson's end, recorded for the child once: progress, what it remembered.
                if self.snapshot?.summary != nil, self.endedAt == nil { self.endedAt = Date() }
                if let summary = self.snapshot?.summary, !self.recorded, let childId = self.childId {
                    self.recorded = true
                    self.profiles.record(id: childId, lessonId: self.lessonId, summary: summary)
                    self.reloadChildren()
                }
                // Paused (the button, or "пауза" said aloud): back to the list once the goodbye
                // has been said and the lesson recorded, at the latest after a few seconds.
                if let s = self.snapshot, s.pauseRequested || self.leavingSince != nil {
                    if self.leavingSince == nil { self.leavingSince = Date() }
                    let said = s.status == .finished && (self.recorded || self.childId == nil)
                    if said || Date().timeIntervalSince(self.leavingSince!) > 8 {
                        self.end()
                        return
                    }
                }
                if self.snapshot?.nextLessonRequested == true,
                   let next = self.childId.flatMap({ self.profiles.nextLesson(id: $0) }) ?? self.nextLessonId {
                    self.timer?.invalidate()
                    self.close(keepCamera: true)
                    self.start(next)
                }
            }
        }
    }

    /// The camera, its frames counted by the shared perception and handed to whichever lesson runs.
    private func makeCamera() -> CameraSource {
        CameraSource { [weak self] y, uv, w, h, ys, uvs, t in
            guard let self, let perception = self.perception else { return }
            let began = Date()
            let result = Result { try perception.processNv12(y: y, uv: uv, width: w, height: h, yStride: ys, uvStride: uvs, timestamp: t) }
            #if DEBUG
            if Self.debugFrames < 5 || Self.debugFrames % 50 == 0 {
                print("frame \(Self.debugFrames) at \(String(format: "%.2f", t)) s: \(String(format: "%.0f", Date().timeIntervalSince(began) * 1000)) ms \(result.map { "raw=\($0.rawCount)" })")
            }
            if case .failure = result { Self.debugFrames += 1 }
            #endif
            if let state = try? result.get() {
                #if DEBUG
                if Self.debugFirstStable < 0, state.isStable {
                    Self.debugFirstStable = t
                    print(String(format: "timing: first settled count %@ at %.2f s after the camera started (%d frames)", String(describing: state.count), t, Self.debugFrames))
                }
                #endif
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
    }

    /// The lesson after the current one in the list, if any.
    var nextLessonId: String? {
        guard let i = lessons.firstIndex(where: { $0.id == lessonId }), i + 1 < lessons.count else { return nil }
        return lessons[i + 1].id
    }

    func stop() {
        session?.stop()
        running = false
        ScreenAwake.set(false)
    }
    func restart() { close(keepCamera: true); start() }
    /// When the grown-up (or the child, saying "пауза") asked to stop: the lesson ends as paused
    /// and the app goes back to the list once the tutor has said goodbye.
    private var leavingSince: Date?

    /// Stops the lesson as paused (the child goes on with it next time) and goes back to the
    /// list after the tutor's goodbye, as "пауза" said aloud does.
    func pause() {
        guard leavingSince == nil else { return end() }
        leavingSince = Date()
        session?.stop()
    }

    /// Ends the lesson at once and goes back to the list.
    func end() {
        leavingSince = nil
        close()
        running = false
        snapshot = nil
    }
    func tap(_ n: UInt32) { session?.tap(number: n) }
    func repeatLine() { session?.`repeat`() }
    func skip() { session?.skip() }

    /// Ends the session; the camera too unless another lesson follows at once.
    func close(keepCamera: Bool = false) {
        ScreenAwake.set(false)
        timer?.invalidate()
        if !keepCamera {
            camera?.stop()
            camera = nil
            table = nil
        }
        // Torn down off the main thread: shutting down waits for the engine's threads, which may
        // be finishing a line.
        let closing = session
        session = nil
        DispatchQueue.global(qos: .userInitiated).async { closing?.shutdown() }
    }
}
