import SwiftUI

/// The Un design: Onym tokens for the grown-up layer, toy accents and the camera-overlay tokens
/// for the child's layer (the "Un Design Board" in Claude Design). Colours follow light and dark
/// mode, except the overlay ones: the camera picture looks the same in both.
enum Un {
    // Grown-up layer (Onym).
    static let surface = Color(light: 0xF5F5F7, dark: 0x0E0E10)
    static let surface2 = Color(light: 0xFFFFFF, dark: 0x17171A)
    static let surface3 = Color(light: 0xEBEBEF, dark: 0x1F1F23)
    static let text = Color(light: 0x0A0A0C, dark: 0xF2F2F4)
    static let text2 = Color(light: 0x0A0A0C, dark: 0xF2F2F4, opacity: 0.62)
    static let text3 = Color(light: 0x0A0A0C, dark: 0xF2F2F4, opacity: 0.42)
    static let hairline = Color(light: 0x000000, dark: 0xFFFFFF, opacity: 0.08)
    static let blue = Color(light: 0x1F86E0, dark: 0x3FA8FF)
    static let green = Color(light: 0x1FA84A, dark: 0x34C759)
    static let red = Color(light: 0xE5392E, dark: 0xFF453A)
    static let onAccent = Color(light: 0xFFFFFF, dark: 0x000000)
    // Child layer: toy accents.
    static let orange = Color(light: 0xE85F2A, dark: 0xFF7A45)
    static let purple = Color(light: 0x8B4DEB, dark: 0xB278FF)
    static let yellow = Color(light: 0xD9A400, dark: 0xFFC93C)
    static let pink = Color(light: 0xE03253, dark: 0xFF4D6D)
    static let accentGreen = Color(light: 0x1FA84A, dark: 0x3DD66E)
    static let accentBlue = Color(light: 0x1F86E0, dark: 0x3FA8FF)
    static let gray = Color(hex: 0x8E8E93)
    /// Each child's colour, by their place in the list.
    static let accents: [Color] = [orange, accentBlue, purple, accentGreen, pink, yellow]
    // Camera overlay: fixed in both themes.
    static let badgeIdle = Color.white
    static let badgeLit = Color(hex: 0xFFC93C)
    static let badgeCounted = Color(hex: 0x1F86E0)
    static let badgeCorrect = Color(hex: 0x1FA84A)
    static let correctStroke = Color(hex: 0x34C759)
    static let halo = Color.black.opacity(0.55)

    static func rounded(_ size: CGFloat, _ weight: Font.Weight = .heavy) -> Font {
        .system(size: size, weight: weight, design: .rounded)
    }
}

extension Color {
    init(hex: UInt32, opacity: Double = 1) {
        self.init(.sRGB, red: Double(hex >> 16 & 0xFF) / 255, green: Double(hex >> 8 & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: opacity)
    }

    init(light: UInt32, dark: UInt32, opacity: Double = 1) {
        func ui(_ hex: UInt32) -> UIColor {
            UIColor(red: CGFloat(hex >> 16 & 0xFF) / 255, green: CGFloat(hex >> 8 & 0xFF) / 255,
                    blue: CGFloat(hex & 0xFF) / 255, alpha: opacity)
        }
        self.init(UIColor { $0.userInterfaceStyle == .dark ? ui(dark) : ui(light) })
    }
}

/// Liquid Glass where the system has it (iOS 26), a thick material otherwise.
struct GlassBackground<S: Shape>: ViewModifier {
    let shape: S
    var strong = false

    func body(content: Content) -> some View {
        if #available(iOS 26, *), !strong {
            content.glassEffect(.regular, in: shape)
        } else {
            content.background(strong ? AnyShapeStyle(.thickMaterial) : AnyShapeStyle(.ultraThinMaterial), in: shape)
                .overlay(shape.stroke(Color.white.opacity(0.35), lineWidth: 0.5))
        }
    }
}

extension View {
    /// Glass in `shape`; `strong` (text over the camera) is the opaque "glass-strong" surface.
    func unGlass<S: Shape>(_ shape: S, strong: Bool = false) -> some View {
        modifier(GlassBackground(shape: shape, strong: strong))
    }
}

/// The ten adventures as the app shows them: name and skill in both languages, by place in the
/// curriculum (the library's titles are English-only "Lesson N: ..." strings).
struct LessonMeta {
    let number: Int
    let title: String
    let skill: String

    private static let table: [(en: String, ru: String, sen: String, sru: String)] = [
        ("Three passengers", "Три пассажира", "Counting to 3", "Счёт до 3"),
        ("Picnic baskets", "Корзинки для пикника", "Adding to 5", "Сложение до 5"),
        ("Cave train", "Поезд в пещере", "Taking away", "Вычитание"),
        ("Bridge builders", "Строители мостов", "Comparing", "Сравнение"),
        ("Parcel detectives", "Детективы посылок", "Numbers and amounts", "Число и количество"),
        ("Firefly gates", "Ворота светлячков", "Counting to 10", "Счёт до 10"),
        ("Supply launch", "Запуск припасов", "Adding to 10", "Сложение до 10"),
        ("Homeward trail", "Тропа домой", "Taking away to 10", "Вычитание до 10"),
        ("Ten-seat ferry", "Паром на десять мест", "Making ten", "Состав числа 10"),
        ("Fair-share cafe", "Кафе «Всем поровну»", "Sharing equally", "Делим поровну"),
    ]

    static func of(_ id: String, in lessons: [UnStrategyInfo], ru: Bool) -> LessonMeta {
        let index = lessons.firstIndex { $0.id == id } ?? 0
        if index < table.count {
            let row = table[index]
            return LessonMeta(number: index + 1, title: ru ? row.ru : row.en, skill: ru ? row.sru : row.sen)
        }
        let title = lessons.first { $0.id == id }?.title ?? id
        let short = title.split(separator: ":", maxSplits: 1).last.map { $0.trimmingCharacters(in: .whitespaces) } ?? title
        return LessonMeta(number: index + 1, title: short, skill: "")
    }
}

/// Every string the screens use, English and Russian, from the design's one copy source. The app
/// addresses the grown-up formally (вы); names stay in the nominative.
struct Copy {
    let ru: Bool

    func callAsFunction(_ key: String, _ args: [String: String] = [:]) -> String {
        var text = (ru ? Self.ruTable : Self.enTable)[key] ?? key
        for (k, v) in args { text = text.replacingOccurrences(of: "{\(k)}", with: v) }
        return text
    }

    private static let enTable: [String: String] = [
        "children.title": "Children", "children.progress": "{n} of {total} lessons done", "children.next": "Next: {lesson}",
        "children.resume": "Paused in: {lesson}", "children.cont": "Continue", "children.add": "Add a child",
        "children.more": "Options for {name}", "children.emptyTitle": "Who's counting today?",
        "children.emptyBody": "Add a child to begin the first adventure. A name is all Un needs.",
        "add.title": "New child", "add.placeholder": "Name", "add.helper": "The tutor will call them by this name.",
        "add.confirm": "Add", "add.cancel": "Cancel",
        "remove.title": "Remove {name}?", "remove.body": "Their progress on this device will be deleted. This can't be undone.",
        "remove.confirm": "Remove", "remove.cancel": "Cancel",
        "cur.sub": "{n} of {total} done", "cur.nextLabel": "Next adventure", "cur.pausedLabel": "Continue where you stopped",
        "cur.lessonN": "Lesson {n}", "cur.done": "Done", "cur.next": "Up next", "cur.paused": "Paused here",
        "cur.cta": "Continue: {lesson}", "cur.hint": "Tap any adventure to start it.", "cur.back": "Children",
        "lesson.speaking": "Speaking", "lesson.listening": "Listening", "lesson.watching": "Watching the table",
        "lesson.celebrating": "Correct!", "lesson.muted": "Tap a number", "lesson.heard": "Heard",
        "lesson.numpadHint": "The microphone can't hear right now.", "lesson.back": "Back to adventures", "lesson.pause": "Pause", "lesson.repeat": "Repeat",
        "lesson.skip": "Skip", "lesson.language": "Language", "lesson.toy": "Toy {n}, {state}",
        "lesson.idle": "not counted yet", "lesson.lit": "counting now", "lesson.counted": "counted", "lesson.done": "correct",
        "prep.title": "Getting the camera ready",
        "prep.body": "Only on first launch — up to a minute. Meanwhile, put a few toys on the table.",
        "prep.step": "Loading toy recognition",
        "error.title": "Toy recognition didn't load",
        "error.body": "Progress is saved. Try again — if it keeps failing, restart the app.",
        "error.retry": "Try again", "error.back": "Back to adventures",
        "end.hooray": "Hooray, {name}!", "end.done": "{lesson} — done!", "end.ask": "Shall we go on?", "end.yes": "Yes!",
        "end.no": "No", "end.rest": "Rest for today", "end.bye": "See you next time, {name}!",
        "end.nextTime": "Next time: {lesson}", "end.home": "Back to children",
    ]

    private static let ruTable: [String: String] = [
        "children.title": "Дети", "children.progress": "Пройдено {n} из {total} уроков", "children.next": "Дальше: {lesson}",
        "children.resume": "Остановились на: {lesson}", "children.cont": "Продолжить", "children.add": "Добавить ребёнка",
        "children.more": "Действия: {name}", "children.emptyTitle": "Кто сегодня считает?",
        "children.emptyBody": "Добавьте ребёнка, чтобы начать первое приключение. Нужно только имя.",
        "add.title": "Новый ребёнок", "add.placeholder": "Имя", "add.helper": "Так наставник будет обращаться к ребёнку.",
        "add.confirm": "Добавить", "add.cancel": "Отмена",
        "remove.title": "Удалить профиль «{name}»?", "remove.body": "Прогресс на этом устройстве будет удалён. Это действие нельзя отменить.",
        "remove.confirm": "Удалить", "remove.cancel": "Отмена",
        "cur.sub": "{n} из {total} пройдено", "cur.nextLabel": "Следующее приключение", "cur.pausedLabel": "Продолжить с места остановки",
        "cur.lessonN": "Урок {n}", "cur.done": "Пройдено", "cur.next": "Следующий", "cur.paused": "На паузе",
        "cur.cta": "Продолжить: {lesson}", "cur.hint": "Любое приключение можно начать — просто нажмите на него.", "cur.back": "Дети",
        "lesson.speaking": "Говорит", "lesson.listening": "Слушает", "lesson.watching": "Смотрит на стол",
        "lesson.celebrating": "Верно!", "lesson.muted": "Нажми на число", "lesson.heard": "Услышано",
        "lesson.numpadHint": "Микрофон сейчас не слышит.", "lesson.back": "К приключениям", "lesson.pause": "Пауза", "lesson.repeat": "Повторить",
        "lesson.skip": "Пропустить", "lesson.language": "Язык", "lesson.toy": "Игрушка {n}, {state}",
        "lesson.idle": "ещё не посчитана", "lesson.lit": "считаем сейчас", "lesson.counted": "посчитана", "lesson.done": "верно",
        "prep.title": "Готовим камеру",
        "prep.body": "Только при первом запуске — до минуты. А пока поставьте на стол несколько игрушек.",
        "prep.step": "Загружаем распознавание игрушек",
        "error.title": "Не удалось загрузить распознавание игрушек",
        "error.body": "Прогресс сохранён. Попробуйте ещё раз, а если не получится — перезапустите приложение.",
        "error.retry": "Попробовать снова", "error.back": "К приключениям",
        "end.hooray": "Ура, {name}!", "end.done": "«{lesson}» — пройдено!", "end.ask": "Продолжим?", "end.yes": "Да!",
        "end.no": "Нет", "end.rest": "Отдохнём", "end.bye": "До встречи, {name}!",
        "end.nextTime": "В следующий раз: {lesson}", "end.home": "К списку детей",
    ]
}

/// The capsule EN/RU switch next to the wordmark and in the lesson toolbar.
struct LanguageSwitch: View {
    @Binding var language: UnLanguage

    var body: some View {
        HStack(spacing: 2) {
            ForEach([(UnLanguage.english, "EN"), (UnLanguage.russian, "RU")], id: \.1) { lang, label in
                Button { language = lang } label: {
                    Text(label).font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(language == lang ? Un.text : Un.text2)
                        .frame(minWidth: 40, minHeight: 32)
                        .background(language == lang ? Un.surface2 : .clear, in: Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(3)
        .background(Un.surface3, in: Capsule())
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Language")
    }
}

/// Onym's primary button: full-width blue, 50pt, 14pt corners.
struct PrimaryButton: View {
    let title: String
    var icon: String?
    var tint: Color = Un.blue
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                if let icon { Image(systemName: icon) }
                Text(title).multilineTextAlignment(.center)
            }
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(Un.onAccent)
            .frame(maxWidth: .infinity, minHeight: 50)
            .padding(.horizontal, 12)
            .background(tint, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

/// The first-launch card: the detector is compiling (up to a minute), or failed to load.
struct CameraStatusCard: View {
    @ObservedObject var model: LessonModel
    /// Over the camera (glass) or over the lists (inline card).
    var overCamera = false
    var onBack: (() -> Void)?

    var body: some View {
        let t = Copy(ru: model.language == .russian)
        if model.preparingCamera {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 5) {
                    ForEach([Un.orange, Un.blue, Un.green], id: \.self) { Circle().fill($0).frame(width: 9, height: 9) }
                }
                Text(t("prep.title")).font(.system(size: 20, weight: .bold))
                Text(t("prep.body")).font(.system(size: 15)).foregroundStyle(Un.text2).fixedSize(horizontal: false, vertical: true)
                ShimmerBar()
                Text(t("prep.step")).font(.system(size: 12.5)).foregroundStyle(Un.text2)
            }
            .padding(20)
            .frame(maxWidth: 520, alignment: .leading)
            .modifier(CardSurface(overCamera: overCamera))
            .accessibilityElement(children: .combine)
        } else if model.detectorFailed {
            VStack(alignment: .leading, spacing: 12) {
                Label(t("error.title"), systemImage: "camera.fill").font(.system(size: 18, weight: .bold))
                    .foregroundStyle(Un.text)
                Text(t("error.body")).font(.system(size: 15)).foregroundStyle(Un.text2).fixedSize(horizontal: false, vertical: true)
                PrimaryButton(title: t("error.retry"), icon: "arrow.counterclockwise") { model.preparePerception() }
                if let onBack {
                    Button(t("error.back"), action: onBack).font(.system(size: 15, weight: .medium)).foregroundStyle(Un.blue)
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
            }
            .padding(20)
            .frame(maxWidth: 520, alignment: .leading)
            .modifier(CardSurface(overCamera: overCamera))
        }
    }
}

/// A card's surface: a plain Onym card over the lists, strong glass over the camera.
struct CardSurface: ViewModifier {
    let overCamera: Bool

    func body(content: Content) -> some View {
        if overCamera {
            content.unGlass(RoundedRectangle(cornerRadius: 22, style: .continuous), strong: true)
        } else {
            content.background(Un.surface2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }
}

/// An indeterminate progress bar: a blue sheen sweeping along the track (the compile reports no
/// percentage). A plain bar with Reduce Motion.
struct ShimmerBar: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        TimelineView(.animation(paused: reduceMotion)) { context in
            let phase = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.6) / 1.6
            GeometryReader { geo in
                Capsule().fill(Un.surface3)
                    .overlay(alignment: .leading) {
                        Capsule().fill(Un.blue)
                            .frame(width: geo.size.width * 0.35)
                            .offset(x: reduceMotion ? 0 : (geo.size.width * 1.35) * phase - geo.size.width * 0.35)
                    }
                    .clipShape(Capsule())
            }
        }
        .frame(height: 6)
        .accessibilityLabel(Text("Loading"))
        .accessibilityAddTraits(.updatesFrequently)
    }
}
