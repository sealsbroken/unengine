import SwiftUI

/// A child's curriculum as a board-game trail: rows of two adventures that snake down the page.
/// Nodes: done (green tick), up next (big orange numeral, pulsing), paused here (orange ring and a
/// pause badge), not yet (quiet, still tappable). The next adventure is also the hero card and
/// the pinned "Continue: ..." button.
struct TrailScreen: View {
    @ObservedObject var model: LessonModel

    var body: some View {
        let ru = model.language == .russian
        let t = Copy(ru: ru)
        if let child = model.child {
            let next = model.nextLesson(for: child)
            let paused = next.map { child.paused == $0.id } ?? false
            let done = model.lessons.filter { child.completed.contains($0.id) }.count
            VStack(spacing: 0) {
                header(child: child, done: done, t: t)
                ScrollView {
                    VStack(spacing: 16) {
                        CameraStatusCard(model: model).padding(.horizontal, 16)
                        if let next {
                            hero(next, paused: paused, ru: ru, t: t).padding(.horizontal, 16)
                        }
                        Trail(model: model, child: child, next: next?.id)
                            .padding(.horizontal, 24)
                        Text(t("cur.hint")).font(.system(size: 12.5)).foregroundStyle(Un.text2)
                            .multilineTextAlignment(.center).padding(.horizontal, 24)
                    }
                    .padding(.top, 8)
                    .padding(.bottom, 96)
                }
            }
            .background(Un.surface.ignoresSafeArea())
            .safeAreaInset(edge: .bottom) {
                if let next {
                    PrimaryButton(title: t("cur.cta", ["lesson": LessonMeta.of(next.id, in: model.lessons, ru: ru).title])) {
                        model.start(next.id)
                    }
                    .frame(maxWidth: 520)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .shadow(color: Un.blue.opacity(0.25), radius: 12, y: 9)
                }
            }
        }
    }

    private func header(child: UnProfile, done: Int, t: Copy) -> some View {
        ZStack {
            VStack(spacing: 2) {
                Text(child.name).font(.system(size: 17, weight: .semibold))
                Text(t("cur.sub", ["n": "\(done)", "total": "\(model.lessons.count)"])).font(.system(size: 12.5))
                    .foregroundStyle(Un.text2)
            }
            HStack {
                Button { model.childId = nil } label: {
                    Image(systemName: "chevron.left").font(.system(size: 18, weight: .semibold)).foregroundStyle(Un.text)
                        .frame(width: 44, height: 44)
                        .unGlass(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(t("cur.back"))
                Spacer()
                LanguageSwitch(language: $model.language)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    private func hero(_ lesson: UnStrategyInfo, paused: Bool, ru: Bool, t: Copy) -> some View {
        let meta = LessonMeta.of(lesson.id, in: model.lessons, ru: ru)
        return Button { model.start(lesson.id) } label: {
            HStack(spacing: 14) {
                LessonArt(number: meta.number).frame(width: 64, height: 64)
                VStack(alignment: .leading, spacing: 3) {
                    Text((paused ? t("cur.pausedLabel") : t("cur.nextLabel")).uppercased())
                        .font(.system(size: 12, weight: .semibold)).tracking(0.4).foregroundStyle(Un.text2)
                    Text(meta.title).font(.system(size: 22, weight: .bold)).foregroundStyle(Un.text)
                    Text("\(t("cur.lessonN", ["n": "\(meta.number)"])) · \(meta.skill)").font(.system(size: 15))
                        .foregroundStyle(Un.text2)
                }
                Spacer(minLength: 0)
            }
            .padding(16)
            .background(Un.surface2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

/// Lesson art is a placeholder until an illustrator draws it: the adventure's symbol on stripes.
struct LessonArt: View {
    let number: Int
    private static let symbols = ["bus.fill", "basket.fill", "tram.fill", "road.lanes", "shippingbox.fill",
                                  "sparkles", "paperplane.fill", "house.fill", "ferry.fill", "cup.and.saucer.fill"]

    var body: some View {
        ZStack {
            Canvas { context, size in
                var x: CGFloat = -size.height
                while x < size.width {
                    var stripe = Path()
                    stripe.move(to: CGPoint(x: x, y: size.height))
                    stripe.addLine(to: CGPoint(x: x + size.height, y: 0))
                    context.stroke(stripe, with: .color(Un.text3.opacity(0.35)), lineWidth: 5)
                    x += 12
                }
            }
            Image(systemName: Self.symbols[(number - 1) % Self.symbols.count]).font(.system(size: 26, weight: .semibold))
                .foregroundStyle(Un.orange)
        }
        .background(Un.surface3)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .accessibilityHidden(true)
    }
}

/// The ten nodes on a snaking path: row 0 left to right, row 1 right to left, and so on.
struct Trail: View {
    @ObservedObject var model: LessonModel
    let child: UnProfile
    let next: String?
    private let rowHeight: CGFloat = 150

    var body: some View {
        let ru = model.language == .russian
        let lessons = model.lessons
        let rows = (lessons.count + 1) / 2
        GeometryReader { geo in
            let w = geo.size.width
            let xs = [w * 0.25, w * 0.75]
            let point = { (i: Int) -> CGPoint in
                let row = i / 2
                let col = row % 2 == 0 ? i % 2 : 1 - i % 2
                return CGPoint(x: xs[col], y: CGFloat(row) * rowHeight + 46)
            }
            ZStack(alignment: .topLeading) {
                // The path: straight within a row, a half-loop at the side between rows.
                Path { path in
                    guard !lessons.isEmpty else { return }
                    path.move(to: point(0))
                    for i in 1..<lessons.count {
                        let a = point(i - 1), b = point(i)
                        if a.y == b.y {
                            path.addLine(to: b)
                        } else {
                            let side: CGFloat = a.x > w / 2 ? 1 : -1
                            let r = (b.y - a.y) / 2
                            path.addArc(center: CGPoint(x: a.x, y: a.y + r), radius: r,
                                        startAngle: .degrees(-90), endAngle: .degrees(90), clockwise: side < 0)
                        }
                    }
                }
                .stroke(Un.surface3, style: StrokeStyle(lineWidth: 8, lineCap: .round, dash: [2, 12]))
                ForEach(Array(lessons.enumerated()), id: \.element.id) { i, lesson in
                    let meta = LessonMeta.of(lesson.id, in: lessons, ru: ru)
                    TrailNode(meta: meta, state: state(of: lesson.id), ru: ru) { model.start(lesson.id) }
                        .frame(width: w / 2 - 8)
                        .position(x: point(i).x, y: point(i).y + 34)
                }
            }
        }
        .frame(height: CGFloat(rows) * rowHeight + 30)
    }

    private func state(of id: String) -> TrailNode.State {
        if child.completed.contains(id) { return .done }
        if child.paused == id { return .paused }
        if id == next { return .next }
        return .notYet
    }
}

/// One adventure on the trail, with its 120pt hit area.
struct TrailNode: View {
    enum State { case done, next, paused, notYet }
    let meta: LessonMeta
    let state: State
    let ru: Bool
    let action: () -> Void
    @SwiftUI.State private var pulse = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let t = Copy(ru: ru)
        Button(action: action) {
            VStack(spacing: 8) {
                ZStack {
                    if state == .next || state == .paused, !reduceMotion {
                        Circle().stroke(Un.orange.opacity(pulse ? 0 : 0.5), lineWidth: pulse ? 1 : 10)
                            .frame(width: 72, height: 72).scaleEffect(pulse ? 1.45 : 1)
                    }
                    circle
                    if state == .paused {
                        Image(systemName: "pause.fill").font(.system(size: 11, weight: .bold)).foregroundStyle(.white)
                            .frame(width: 24, height: 24).background(Un.orange, in: Circle())
                            .overlay(Circle().stroke(Un.surface, lineWidth: 2))
                            .offset(x: 26, y: 24)
                    }
                }
                .frame(width: 84, height: 84)
                VStack(spacing: 2) {
                    Text(meta.title).font(.system(size: 15, weight: .semibold)).foregroundStyle(Un.text)
                        .multilineTextAlignment(.center)
                    Text(subtitle(t)).font(.system(size: 12.5, weight: state == .notYet ? .regular : .medium))
                        .foregroundStyle(subtitleColor).multilineTextAlignment(.center)
                }
            }
            .frame(minWidth: 120, minHeight: 120)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(t("cur.lessonN", ["n": "\(meta.number)"])), \(meta.title), \(subtitle(t))")
        .onAppear {
            guard state == .next || state == .paused else { return }
            withAnimation(.easeOut(duration: 1.8).repeatForever(autoreverses: false)) { pulse = true }
        }
    }

    @ViewBuilder private var circle: some View {
        switch state {
        case .done:
            Image(systemName: "checkmark").font(.system(size: 28, weight: .bold)).foregroundStyle(.white)
                .frame(width: 72, height: 72).background(Un.green, in: Circle())
                .shadow(color: .black.opacity(0.12), radius: 0, y: -4)
        case .next:
            Text("\(meta.number)").font(Un.rounded(40)).foregroundStyle(.white)
                .frame(width: 88, height: 88).background(Un.orange, in: Circle())
        case .paused:
            Text("\(meta.number)").font(Un.rounded(36)).foregroundStyle(Un.orange)
                .frame(width: 72, height: 72).background(Un.surface2, in: Circle())
                .overlay(Circle().stroke(Un.orange, lineWidth: 4))
        case .notYet:
            Text("\(meta.number)").font(Un.rounded(30)).foregroundStyle(Un.text2)
                .frame(width: 66, height: 66).background(Un.surface2, in: Circle())
                .overlay(Circle().stroke(Un.hairline, lineWidth: 1))
        }
    }

    private func subtitle(_ t: Copy) -> String {
        switch state {
        case .done: return t("cur.done")
        case .next: return t("cur.next")
        case .paused: return t("cur.paused")
        case .notYet: return meta.skill
        }
    }

    private var subtitleColor: Color {
        switch state {
        case .done: return Un.green
        case .next, .paused: return Un.orange
        case .notYet: return Un.text2
        }
    }
}
