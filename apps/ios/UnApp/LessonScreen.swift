import SwiftUI

/// The children, a child's lessons, then the lesson: the tutor's line, status, the camera's
/// picture, and the number pad. Perception runs through the shared Rust models (`UnPerception`);
/// speech through Apple's APIs. The lessons are whatever the linked library offers
/// (`availableStrategies`); each child's progress through them is kept (`UnProfiles`).
struct LessonScreen: View {
    @StateObject private var model = LessonModel()

    var body: some View {
        Group {
            if model.running {
                lesson
            } else if model.child == nil {
                ChildrenList(model: model)
            } else {
                LessonList(model: model)
            }
        }
        .onAppear { model.reloadLessons(); model.reloadChildren(); model.preparePerception(); Permissions.request {} }
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
                    CameraStatus(model: model)
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
                Button(model.language == .russian ? "Пауза" : "Pause") { model.pause() }
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

/// While the detector loads (or when it failed), a card the grown-up can read: the camera does
/// not count yet. After an install or update iOS compiles the model for the Neural Engine,
/// which takes up to a minute; later launches take seconds.
struct CameraStatus: View {
    @ObservedObject var model: LessonModel

    var body: some View {
        let ru = model.language == .russian
        if model.preparingCamera {
            VStack(spacing: 8) {
                ProgressView().controlSize(.large)
                Text(ru ? "Готовим камеру…" : "Preparing the camera…").font(.headline)
                Text(ru ? "После установки или обновления это занимает до минуты. Урок можно начинать: игрушки будут видны, как только камера будет готова."
                        : "After an install or update this takes up to a minute. You can start a lesson: the toys are seen as soon as the camera is ready.")
                    .font(.footnote).multilineTextAlignment(.center).foregroundStyle(.secondary)
            }
            .padding()
            .frame(maxWidth: 420)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16))
            .padding()
        } else if model.detectorFailed {
            Label(ru ? "Камера не смогла загрузить распознавание игрушек." : "The camera could not load toy recognition.",
                  systemImage: "exclamationmark.triangle")
                .font(.headline).foregroundStyle(.red)
                .padding()
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16))
                .padding()
        }
    }
}

/// The children who use the app, each with their progress; a tap goes to that child's lessons.
/// A grown-up adds a child by name and can remove one.
struct ChildrenList: View {
    @ObservedObject var model: LessonModel
    @State private var adding = false
    @State private var name = ""
    @State private var removing: UnProfile?

    var body: some View {
        let ru = model.language == .russian
        NavigationStack {
            List {
                ForEach(model.children, id: \.id) { child in
                    Button { model.childId = child.id } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(child.name).font(.title3.weight(.semibold))
                            Text(progress(child, ru: ru)).font(.subheadline).foregroundStyle(.secondary)
                        }
                    }
                    .foregroundStyle(.primary)
                    .swipeActions {
                        Button(ru ? "Удалить" : "Remove", role: .destructive) { removing = child }
                    }
                }
            }
            .overlay {
                if model.children.isEmpty {
                    ContentUnavailableView {
                        Label(ru ? "Добавьте ребёнка" : "Add a child", systemImage: "person.crop.circle.badge.plus")
                    } description: {
                        Text(ru ? "У каждого ребёнка свой прогресс по урокам." : "Each child goes through the lessons at their own pace.")
                    } actions: {
                        Button(ru ? "Добавить" : "Add") { adding = true }.buttonStyle(.borderedProminent)
                    }
                }
            }
            .safeAreaInset(edge: .bottom) { CameraStatus(model: model) }
            .navigationTitle(ru ? "Дети" : "Children")
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Picker("Language", selection: $model.language) {
                        Text("EN").tag(UnLanguage.english)
                        Text("RU").tag(UnLanguage.russian)
                    }.pickerStyle(.segmented).frame(width: 100)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { adding = true } label: { Image(systemName: "plus") }
                        .accessibilityLabel(ru ? "Добавить ребёнка" : "Add a child")
                }
            }
            .alert(ru ? "Новый ребёнок" : "New child", isPresented: $adding) {
                TextField(ru ? "Имя" : "Name", text: $name)
                Button(ru ? "Добавить" : "Add") { model.addChild(name); name = "" }
                Button(ru ? "Отмена" : "Cancel", role: .cancel) { name = "" }
            } message: {
                Text(ru ? "Тьютор будет обращаться к ребёнку по имени." : "The tutor calls the child by this name.")
            }
            .alert(ru ? "Удалить \(removing?.name ?? "")?" : "Remove \(removing?.name ?? "")?", isPresented: Binding(
                get: { removing != nil }, set: { if !$0 { removing = nil } })) {
                Button(ru ? "Удалить" : "Remove", role: .destructive) {
                    if let id = removing?.id { model.removeChild(id) }
                    removing = nil
                }
                Button(ru ? "Отмена" : "Cancel", role: .cancel) { removing = nil }
            } message: {
                Text(ru ? "Прогресс ребёнка будет удалён." : "The child's progress is deleted.")
            }
        }
    }

    private func progress(_ child: UnProfile, ru: Bool) -> String {
        let done = model.lessons.filter { child.completed.contains($0.id) }.count
        let total = model.lessons.count
        var text = ru ? "Пройдено уроков: \(done) из \(total)" : "\(done) of \(total) lessons done"
        if let next = model.nextLesson(for: child) {
            text += ru ? " · дальше: \(next.title)" : " · next: \(next.title)"
        }
        return text
    }
}

/// The chosen child's lessons, in the library's order: the ones they completed are ticked and
/// the one they go on with is marked; a tap starts any of them.
struct LessonList: View {
    @ObservedObject var model: LessonModel

    var body: some View {
        let ru = model.language == .russian
        let child = model.child
        let next = child.flatMap { model.nextLesson(for: $0) }
        NavigationStack {
            List {
                if let next {
                    Section {
                        Button { model.start(next.id) } label: {
                            Label((ru ? "Продолжить: " : "Continue: ") + next.title, systemImage: "play.fill")
                                .font(.headline)
                        }
                    }
                }
                Section {
                    ForEach(model.lessons, id: \.id) { lesson in
                        Button { model.start(lesson.id) } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(lesson.title).font(.headline)
                                    Text(lesson.description).font(.subheadline).foregroundStyle(.secondary)
                                }
                                Spacer()
                                if child?.completed.contains(lesson.id) == true {
                                    Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                                        .accessibilityLabel(ru ? "пройден" : "done")
                                } else if lesson.id == next?.id {
                                    Image(systemName: "arrow.right.circle").foregroundStyle(.tint)
                                        .accessibilityLabel(ru ? "дальше" : "next")
                                }
                            }
                        }
                        .foregroundStyle(.primary)
                    }
                }
            }
            .overlay {
                if model.lessons.isEmpty { Text(ru ? "Уроков нет" : "No lessons").foregroundStyle(.secondary) }
            }
            .safeAreaInset(edge: .bottom) { CameraStatus(model: model) }
            .navigationTitle(child?.name ?? (ru ? "Уроки" : "Lessons"))
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button { model.childId = nil } label: { Label(ru ? "Дети" : "Children", systemImage: "chevron.left") }
                        .labelStyle(.titleAndIcon)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    // The tutor's language for the next lesson.
                    Picker("Language", selection: $model.language) {
                        Text("EN").tag(UnLanguage.english)
                        Text("RU").tag(UnLanguage.russian)
                    }.pickerStyle(.segmented).frame(width: 100)
                }
            }
        }
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

    var statusLine: String {
        let title = lessons.first { $0.id == lessonId }?.title ?? lessonId
        guard let s = snapshot else { return title }
        return "\(title)  \(s.status)  level \(s.level + 1)/\(s.levelCount)  \(s.correct)✓/\(s.tasksCompleted)"
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
        camera = CameraSource { [weak self] y, uv, w, h, ys, uvs, t in
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
        camera?.start()
        // After a completed lesson the tutor asks "Продолжим?"; a yes starts the next one.
        session?.offerNextLesson(offer: nextLessonId != nil)
        session?.start()
        running = true
        // The phone lies on the table for minutes with nobody touching it: no auto-lock while
        // a lesson runs.
        UIApplication.shared.isIdleTimerDisabled = true
        timer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor in
                guard let self else { return }
                self.snapshot = self.session?.snapshot()
                // The lesson's end, recorded for the child once: progress, what it remembered.
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
                    self.close()
                    self.start(next)
                }
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
        UIApplication.shared.isIdleTimerDisabled = false
    }
    func restart() { close(); start() }
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

    func close() {
        UIApplication.shared.isIdleTimerDisabled = false
        timer?.invalidate()
        camera?.stop()
        camera = nil
        table = nil
        // Torn down off the main thread: shutting down waits for the engine's threads, which may
        // be finishing a line.
        let closing = session
        session = nil
        DispatchQueue.global(qos: .userInitiated).async { closing?.shutdown() }
    }
}
