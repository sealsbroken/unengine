import SwiftUI

/// The child's screen: the live camera is the hero. Only three things sit over it: the toy boxes
/// with their number badges, one tutor panel at the bottom, and a small parent toolbar at the top
/// (Pause, Repeat, Skip, language). The number pad appears only when speech fails; the end
/// screens follow a completed lesson.
struct LessonView: View {
    @ObservedObject var model: LessonModel
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        let ru = model.language == .russian
        let t = Copy(ru: ru)
        let state = model.tutorState
        let meta = LessonMeta.of(model.lessonId, in: model.lessons, ru: ru)
        ZStack {
            Color.black.ignoresSafeArea()
            #if targetEnvironment(simulator)
            // No camera here (and attaching a preview to an input-less session hangs): a wooden
            // table stands in, as on the design board.
            LinearGradient(colors: [Color(hex: 0xC48A52), Color(hex: 0xA8703D), Color(hex: 0xCD9660)],
                           startPoint: .leading, endPoint: .trailing).ignoresSafeArea()
            #else
            if let camera = model.camera {
                CameraPreview(session: camera.session).ignoresSafeArea()
            }
            #endif
            TableOverlay(table: model.table, frameSize: model.frameSize, highlight: model.snapshot?.highlightCount,
                         correct: state == .celebrating, copy: t)
                .ignoresSafeArea()
            Color.black.opacity(state == .celebrating ? 0.66 : 0).ignoresSafeArea()
                .animation(.easeOut(duration: 0.6), value: state == .celebrating)
            VStack(spacing: 12) {
                topBar(meta: meta, t: t)
                Spacer()
                if model.snapshot?.showNumberPad == true { NumberPad(model: model, t: t) }
                if model.preparingCamera || model.detectorFailed {
                    CameraStatusCard(model: model, overCamera: true, onBack: { model.end() })
                } else {
                    TutorPanel(model: model, state: state, t: t, compact: sizeClass != .regular)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 8)
            if state == .celebrating, model.endPhase == nil {
                CountTicket(count: model.table?.count ?? model.snapshot?.targetCount ?? 0)
            }
            if let phase = model.endPhase {
                EndView(model: model, phase: phase, meta: meta, t: t)
                    .transition(.opacity)
            }
        }
        .animation(.easeInOut(duration: 0.4), value: model.endPhase)
    }

    private func topBar(meta: LessonMeta, t: Copy) -> some View {
        HStack {
            HStack(spacing: 8) {
                Text("\(meta.number)").font(Un.rounded(14)).foregroundStyle(.white)
                    .frame(width: 26, height: 26).background(Un.orange, in: Circle())
                Text(meta.title).font(.system(size: 15, weight: .semibold)).foregroundStyle(Un.text).lineLimit(1)
            }
            .padding(.leading, 6).padding(.trailing, 14).frame(minHeight: 44)
            .unGlass(Capsule())
            Spacer()
            HStack(spacing: 0) {
                toolButton("pause.fill", t("lesson.pause")) { model.pause() }
                toolButton("arrow.counterclockwise", t("lesson.repeat")) { model.repeatLine() }
                toolButton("forward.end.fill", t("lesson.skip")) { model.skip() }
                Button {
                    model.language = model.language == .russian ? .english : .russian
                } label: {
                    Text(model.language == .russian ? "RU" : "EN").font(.system(size: 13, weight: .bold))
                        .foregroundStyle(Un.text).frame(width: 44, height: 44)
                }
                .accessibilityLabel(t("lesson.language"))
            }
            .padding(.horizontal, 4)
            .unGlass(Capsule())
        }
    }

    private func toolButton(_ symbol: String, _ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 16, weight: .semibold)).foregroundStyle(Un.text)
                .frame(width: 44, height: 44)
        }
        .accessibilityLabel(label)
    }
}

/// One state at a time: orange speaking (ripples), blue listening (mic level and what was
/// heard), purple watching the table (a slowly turning dashed ring), green correct.
struct TutorPanel: View {
    @ObservedObject var model: LessonModel
    let state: LessonModel.TutorState
    let t: Copy
    let compact: Bool

    var body: some View {
        let s = model.snapshot
        HStack(alignment: .center, spacing: 20) {
            TutorOrb(state: state, level: Double(s?.micLevel ?? 0), size: compact ? 64 : 84)
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text(label.uppercased()).font(.system(size: 13, weight: .semibold)).tracking(0.6)
                        .foregroundStyle(TutorOrb.color(state))
                    if state == .listening { MicBars(level: Double(s?.micLevel ?? 0)) }
                }
                Text(s?.lastSpoken ?? "…").font(.system(size: compact ? 24 : 32, weight: .bold))
                    .foregroundStyle(Un.text).fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.updatesFrequently)
                if state == .listening, let heard = s?.heardText, !heard.isEmpty {
                    (Text("\(t("lesson.heard")): ").foregroundStyle(Un.text2) + Text("«\(heard)»").bold())
                        .font(.system(size: 15)).lineLimit(2)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(24)
        .unGlass(RoundedRectangle(cornerRadius: 22, style: .continuous), strong: true)
        .animation(.easeInOut(duration: 0.18), value: state)
    }

    private var label: String {
        switch state {
        case .speaking: return t("lesson.speaking")
        case .listening: return t("lesson.listening")
        case .watching: return t("lesson.watching")
        case .celebrating: return t("lesson.celebrating")
        case .muted: return t("lesson.muted")
        }
    }
}

/// The tutor's orb: colour, icon and motion say the state, never colour alone.
struct TutorOrb: View {
    let state: LessonModel.TutorState
    var level: Double = 0
    var size: CGFloat = 84
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static func color(_ state: LessonModel.TutorState) -> Color {
        switch state {
        case .speaking: return Un.orange
        case .listening: return Un.blue
        case .watching: return Un.purple
        case .celebrating: return Un.green
        case .muted: return Un.gray
        }
    }

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let time = context.date.timeIntervalSinceReferenceDate
            ZStack {
                switch state {
                case .speaking:
                    ForEach(0..<2) { i in
                        let phase = (time / 1.8 + Double(i) * 0.5).truncatingRemainder(dividingBy: 1)
                        Circle().stroke(Un.orange, lineWidth: 3).scaleEffect(1 + phase * 0.75).opacity(0.65 * (1 - phase))
                    }
                case .listening:
                    // The ring follows the microphone (1.0 to 1.3); a breath when it reports nothing.
                    let breath = level > 0.02 ? level : 0.35 + 0.35 * sin(time * 4)
                    Circle().fill(Un.blue.opacity(0.18)).scaleEffect(1.1 + 0.3 * breath)
                    Circle().fill(Un.blue.opacity(0.28)).scaleEffect(1.05 + 0.15 * breath)
                case .watching:
                    Circle().stroke(Un.purple, style: StrokeStyle(lineWidth: 3, dash: [8, 6]))
                        .scaleEffect(1.2).rotationEffect(.degrees(time / 9 * 360))
                case .celebrating:
                    let phase = (time / 1.2).truncatingRemainder(dividingBy: 1)
                    Circle().fill(Un.green.opacity(0.35)).scaleEffect(0.95 + phase * 0.65).opacity(1 - phase)
                case .muted:
                    EmptyView()
                }
                Circle().fill(Self.color(state))
                    .overlay(Circle().stroke(Color.black.opacity(0.14), lineWidth: 4).offset(y: 2).mask(Circle()))
                Image(systemName: symbol).font(.system(size: size * 0.4, weight: .bold)).foregroundStyle(.white)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }

    private var symbol: String {
        switch state {
        case .speaking: return "speaker.wave.2.fill"
        case .listening: return "mic.fill"
        case .watching: return "eye.fill"
        case .celebrating: return "star.fill"
        case .muted: return "mic.slash.fill"
        }
    }
}

/// Five bars next to "Listening", moving with the microphone.
struct MicBars: View {
    let level: Double

    var body: some View {
        TimelineView(.animation) { context in
            let time = context.date.timeIntervalSinceReferenceDate
            HStack(spacing: 2) {
                ForEach(0..<5) { i in
                    let wave = 0.5 + 0.5 * sin(time * 9 + Double(i) * 1.3)
                    let height = 4 + 12 * (level > 0.02 ? level : 0.45) * wave
                    Capsule().fill(Un.blue).frame(width: 3, height: height)
                }
            }
            .frame(height: 16)
        }
        .accessibilityHidden(true)
    }
}

/// The fallback when the microphone can't hear: numbers to tap (keys of 52pt or more).
struct NumberPad: View {
    @ObservedObject var model: LessonModel
    let t: Copy

    var body: some View {
        VStack(spacing: 10) {
            Text(t("lesson.numpadHint")).font(.system(size: 13, weight: .medium)).foregroundStyle(Un.text2)
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: 6), spacing: 8) {
                ForEach(0...10, id: \.self) { n in
                    Button { model.tap(UInt32(n)) } label: {
                        Text("\(n)").font(Un.rounded(28)).foregroundStyle(Un.text)
                            .frame(maxWidth: .infinity, minHeight: 56)
                            .background(Un.surface2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .padding(16)
        .frame(maxWidth: 560)
        .unGlass(RoundedRectangle(cornerRadius: 22, style: .continuous), strong: true)
    }
}

/// A correct count: the stamped ticket with the big green numeral and a burst of confetti.
struct CountTicket: View {
    let count: UInt32
    @State private var shown = false

    var body: some View {
        ZStack {
            Confetti()
            VStack(spacing: 10) {
                Text("\(count)").font(Un.rounded(132)).foregroundStyle(Un.badgeCorrect)
                Rectangle().fill(.clear).frame(height: 1)
                    .overlay(Line().stroke(Color.black.opacity(0.15), style: StrokeStyle(lineWidth: 1.5, dash: [5, 5])))
                HStack(spacing: 8) {
                    ForEach([Color(hex: 0xD8402F), Color(hex: 0xF2BF2A), Color(hex: 0x3A6CD0)], id: \.self) {
                        RoundedRectangle(cornerRadius: 5).fill($0).frame(width: 22, height: 16)
                    }
                }
            }
            .padding(.horizontal, 36).padding(.vertical, 22)
            .background(Color.white, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay(alignment: .topTrailing) {
                Image(systemName: "checkmark").font(.system(size: 18, weight: .heavy)).foregroundStyle(Un.orange)
                    .frame(width: 40, height: 40).overlay(Circle().stroke(Un.orange, lineWidth: 2.5))
                    .rotationEffect(.degrees(-12)).padding(12)
            }
            .shadow(color: .black.opacity(0.3), radius: 20, y: 10)
            .scaleEffect(shown ? 1 : 0.6).opacity(shown ? 1 : 0)
        }
        .onAppear { withAnimation(.spring(response: 0.45, dampingFraction: 0.6)) { shown = true } }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(count)")
    }
}

private struct Line: Shape {
    func path(in rect: CGRect) -> Path {
        Path { $0.move(to: CGPoint(x: rect.minX, y: rect.midY)); $0.addLine(to: CGPoint(x: rect.maxX, y: rect.midY)) }
    }
}

/// Sixteen pieces flying out over 2.4 s (none with Reduce Motion).
struct Confetti: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var start = Date()
    private let colors = [0xFFC93C, 0xE85F2A, 0x1F86E0, 0x1FA84A, 0x8B4DEB, 0xE03253].map { Color(hex: UInt32($0)) }

    var body: some View {
        if !reduceMotion {
            TimelineView(.animation) { context in
                let p = min(1, context.date.timeIntervalSince(start) / 2.4)
                ZStack {
                    ForEach(0..<16, id: \.self) { i in
                        let angle = Angle.degrees(Double(i) * 22.5 + Double(i % 2) * 7)
                        RoundedRectangle(cornerRadius: 2).fill(colors[i % colors.count])
                            .frame(width: 8, height: i % 3 == 0 ? 24 : 14)
                            .rotationEffect(.degrees(p * 160))
                            .offset(x: cos(angle.radians) * 210 * p, y: sin(angle.radians) * 210 * p)
                            .opacity(p < 0.7 ? 1 : (1 - p) / 0.3)
                    }
                }
            }
            .allowsHitTesting(false)
        }
    }
}

/// The end of a completed lesson, in three beats: the stamped ticket with three stars,
/// "Продолжим?" with picture buttons that mirror what the tutor hears, and a calm goodbye.
struct EndView: View {
    @ObservedObject var model: LessonModel
    let phase: LessonModel.EndPhase
    let meta: LessonMeta
    let t: Copy

    var body: some View {
        let name = model.child?.name ?? ""
        ZStack {
            Un.surface.ignoresSafeArea()
            VStack(spacing: 22) {
                Spacer()
                switch phase {
                case .celebrate:
                    ZStack {
                        Confetti()
                        StarTicket(meta: meta)
                    }
                    Text(t("end.hooray", ["name": name])).font(Un.rounded(52)).multilineTextAlignment(.center)
                    Text(t("end.done", ["lesson": meta.title])).font(.system(size: 17)).foregroundStyle(Un.text2)
                    TrailDots(model: model)
                case .ask:
                    TutorOrb(state: .listening, size: 84)
                    Text(t("end.ask")).font(Un.rounded(52)).multilineTextAlignment(.center)
                    HStack(spacing: 20) {
                        answerCard(t("end.yes"), symbol: "checkmark", tint: Un.green) { model.answerNext(true) }
                        answerCard(t("end.no"), symbol: "moon.stars.fill", tint: Un.gray) { model.answerNext(false) }
                    }
                    .frame(maxWidth: 520)
                case .bye:
                    Image(systemName: "moon.stars.fill").font(.system(size: 72)).foregroundStyle(Un.purple)
                    Text(t("end.bye", ["name": name])).font(Un.rounded(40)).multilineTextAlignment(.center)
                    if let child = model.child, let next = model.nextLesson(for: child) {
                        Text(t("end.nextTime", ["lesson": LessonMeta.of(next.id, in: model.lessons, ru: t.ru).title]))
                            .font(.system(size: 17)).foregroundStyle(Un.text2)
                    }
                    PrimaryButton(title: t("end.home")) { model.goHome() }.frame(maxWidth: 360)
                }
                Spacer()
            }
            .padding(24)
        }
    }

    private func answerCard(_ title: String, symbol: String, tint: Color, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 14) {
                Image(systemName: symbol).font(.system(size: 54, weight: .bold))
                Text(title).font(Un.rounded(30))
            }
            .foregroundStyle(.white)
            .frame(maxWidth: .infinity, minHeight: 200)
            .background(tint, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

/// The lesson-end ticket: three stars over the lesson's chip.
struct StarTicket: View {
    let meta: LessonMeta
    @State private var shown = false

    var body: some View {
        VStack(spacing: 14) {
            HStack(spacing: 12) {
                ForEach(0..<3) { i in
                    Image(systemName: "star.fill").font(.system(size: i == 1 ? 46 : 36)).foregroundStyle(Color(hex: 0xFFC93C))
                }
            }
            Rectangle().fill(.clear).frame(height: 1)
                .overlay(Line().stroke(Color.black.opacity(0.15), style: StrokeStyle(lineWidth: 1.5, dash: [5, 5])))
            HStack(spacing: 8) {
                Text("\(meta.number)").font(Un.rounded(13)).foregroundStyle(.white)
                    .frame(width: 24, height: 24).background(Un.orange, in: Circle())
                Text(meta.title).font(.system(size: 15, weight: .semibold)).foregroundStyle(Color(hex: 0x0A0A0C).opacity(0.7))
            }
        }
        .padding(.horizontal, 32).padding(.vertical, 22)
        .frame(minWidth: 280)
        .background(Color.white, in: RoundedRectangle(cornerRadius: 22, style: .continuous))
        .shadow(color: .black.opacity(0.12), radius: 16, y: 8)
        .scaleEffect(shown ? 1 : 0.6).opacity(shown ? 1 : 0)
        .onAppear { withAnimation(.spring(response: 0.5, dampingFraction: 0.6)) { shown = true } }
    }
}

/// The child's progress as ten dots, the completed ones green.
struct TrailDots: View {
    @ObservedObject var model: LessonModel

    var body: some View {
        HStack(spacing: 6) {
            ForEach(model.lessons, id: \.id) { lesson in
                let done = model.child?.completed.contains(lesson.id) == true
                Capsule().fill(done ? Un.green : Un.surface3).frame(width: done ? 22 : 10, height: 10)
            }
        }
        .accessibilityHidden(true)
    }
}
