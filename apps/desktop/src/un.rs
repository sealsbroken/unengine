//! The Un design on the desktop (the Claude Design "Un Desktop Board"): the iOS app's tokens,
//! copy and component shapes, with what a window needs instead of glass and SF fonts. Inter
//! (UI text) and Nunito Black (wordmark, numerals) are bundled, OFL (fonts/).

use std::f32::consts::PI;
use std::sync::Arc;

use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, Ui, Vec2,
};
use un_core::Language;

// ------------------------------------------------------------------ tokens

/// The colours of one appearance (light or dark follow the system, as on iOS).
#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub surface: Color32,
    pub surface2: Color32,
    pub surface3: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub hairline: Color32,
    pub hairline_strong: Color32,
    pub blue: Color32,
    pub green: Color32,
    pub red: Color32,
    pub on_accent: Color32,
    pub orange: Color32,
    pub purple: Color32,
    pub yellow: Color32,
    pub pink: Color32,
    pub accent_green: Color32,
    pub accent_blue: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Theme {
    pub fn of(ui: &Ui) -> Self {
        Self::new(ui.visuals().dark_mode)
    }

    pub fn new(dark: bool) -> Self {
        let pick = |l: u32, d: u32| if dark { rgb(d) } else { rgb(l) };
        let ink = if dark { (242, 242, 244) } else { (10, 10, 12) };
        let alpha = |a: f32| Color32::from_rgba_unmultiplied(ink.0, ink.1, ink.2, (a * 255.0) as u8);
        Self {
            dark,
            surface: pick(0xF5F5F7, 0x0E0E10),
            surface2: pick(0xFFFFFF, 0x17171A),
            surface3: pick(0xEBEBEF, 0x1F1F23),
            text: pick(0x0A0A0C, 0xF2F2F4),
            text2: alpha(0.62),
            text3: alpha(0.42),
            hairline: if dark { Color32::from_white_alpha(18) } else { Color32::from_black_alpha(15) },
            hairline_strong: if dark { Color32::from_white_alpha(31) } else { Color32::from_black_alpha(31) },
            blue: pick(0x1F86E0, 0x3FA8FF),
            green: pick(0x1FA84A, 0x34C759),
            red: pick(0xE5392E, 0xFF453A),
            on_accent: pick(0xFFFFFF, 0x000000),
            orange: pick(0xE85F2A, 0xFF7A45),
            purple: pick(0x8B4DEB, 0xB278FF),
            yellow: pick(0xD9A400, 0xFFC93C),
            pink: pick(0xE03253, 0xFF4D6D),
            accent_green: pick(0x1FA84A, 0x3DD66E),
            accent_blue: pick(0x1F86E0, 0x3FA8FF),
        }
    }

    /// Each child's colour, by their place in the list.
    pub fn accent(&self, index: usize) -> Color32 {
        [self.orange, self.accent_blue, self.purple, self.accent_green, self.pink, self.yellow][index % 6]
    }

    /// The glass of iOS on the desktop: surface2 at 96%.
    pub fn panel(&self) -> Color32 {
        self.surface2.gamma_multiply(0.96)
    }
}

/// The camera overlay: fixed in both themes (the picture looks the same).
pub mod overlay_colors {
    use egui::Color32;
    pub const BADGE_LIT: Color32 = Color32::from_rgb(0xFF, 0xC9, 0x3C);
    pub const BADGE_COUNTED: Color32 = Color32::from_rgb(0x1F, 0x86, 0xE0);
    pub const BADGE_CORRECT: Color32 = Color32::from_rgb(0x1F, 0xA8, 0x4A);
    pub const CORRECT_STROKE: Color32 = Color32::from_rgb(0x34, 0xC7, 0x59);
    pub const INK: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x0C);
    pub const LETTERBOX: Color32 = Color32::from_rgb(0x0E, 0x0E, 0x10);

    pub fn halo() -> Color32 {
        Color32::from_black_alpha(140)
    }
}

// ------------------------------------------------------------------ fonts

/// Inter for UI text, Nunito Black for the wordmark and rounded numerals; the default fonts stay
/// behind them for anything else.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut family = |name: &str, files: [(&str, &'static [u8]); 2]| {
        let mut chain = vec![];
        for (key, bytes) in files {
            fonts.font_data.insert(key.to_string(), Arc::new(FontData::from_static(bytes)));
            chain.push(key.to_string());
        }
        chain.extend(defaults.clone());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    };
    family("inter-400", [
        ("inter-400-latin", include_bytes!("../fonts/inter-400-latin.ttf")),
        ("inter-400-cyrillic", include_bytes!("../fonts/inter-400-cyrillic.ttf")),
    ]);
    family("inter-600", [
        ("inter-600-latin", include_bytes!("../fonts/inter-600-latin.ttf")),
        ("inter-600-cyrillic", include_bytes!("../fonts/inter-600-cyrillic.ttf")),
    ]);
    family("inter-700", [
        ("inter-700-latin", include_bytes!("../fonts/inter-700-latin.ttf")),
        ("inter-700-cyrillic", include_bytes!("../fonts/inter-700-cyrillic.ttf")),
    ]);
    family("nunito-900", [
        ("nunito-900-latin", include_bytes!("../fonts/nunito-900-latin.ttf")),
        ("nunito-900-cyrillic", include_bytes!("../fonts/nunito-900-cyrillic.ttf")),
    ]);
    // Plain proportional text (egui's own widgets) in Inter too.
    let mut body = vec!["inter-400-latin".to_string(), "inter-400-cyrillic".to_string()];
    body.extend(defaults);
    fonts.families.insert(FontFamily::Proportional, body);
    ctx.set_fonts(fonts);
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-400".into()))
}
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-600".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("inter-700".into()))
}
/// Rounded heavy numerals and the wordmark.
pub fn rounded(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("nunito-900".into()))
}

// ------------------------------------------------------------------ copy

/// Every string the window uses, English and Russian (the iOS app's table plus the desktop's).
/// The app addresses the grown-up formally (вы); names stay in the nominative.
pub fn t(ru: bool, key: &str) -> &'static str {
    let (en, rus) = match key {
        "children.title" => ("Children", "Дети"),
        "children.progress" => ("{n} of {total} lessons done", "Пройдено {n} из {total} уроков"),
        "children.next" => ("Next: {lesson}", "Дальше: {lesson}"),
        "children.resume" => ("Paused in: {lesson}", "Остановились на: {lesson}"),
        "children.cont" => ("Continue", "Продолжить"),
        "children.add" => ("Add a child", "Добавить ребёнка"),
        "children.open" => ("Open adventures", "Открыть приключения"),
        "children.remove" => ("Remove…", "Удалить…"),
        "children.emptyTitle" => ("Who's counting today?", "Кто сегодня считает?"),
        "children.emptyBody" => (
            "Add a child to begin the first adventure. A name is all Un needs.",
            "Добавьте ребёнка, чтобы начать первое приключение. Нужно только имя.",
        ),
        "add.title" => ("New child", "Новый ребёнок"),
        "add.placeholder" => ("Name", "Имя"),
        "add.helper" => ("The tutor will call them by this name.", "Так наставник будет обращаться к ребёнку."),
        "add.confirm" => ("Add", "Добавить"),
        "cancel" => ("Cancel", "Отмена"),
        "remove.title" => ("Remove {name}?", "Удалить профиль «{name}»?"),
        "remove.body" => (
            "Their progress on this device will be deleted. This can't be undone.",
            "Прогресс на этом устройстве будет удалён. Это действие нельзя отменить.",
        ),
        "remove.confirm" => ("Remove", "Удалить"),
        "cur.sub" => ("{n} of {total} done", "{n} из {total} пройдено"),
        "cur.nextLabel" => ("Next adventure", "Следующее приключение"),
        "cur.pausedLabel" => ("Continue where you stopped", "Продолжить с места остановки"),
        "cur.lessonN" => ("Lesson {n}", "Урок {n}"),
        "cur.done" => ("Done", "Пройдено"),
        "cur.next" => ("Up next", "Следующий"),
        "cur.paused" => ("Paused here", "На паузе"),
        "cur.cta" => ("Continue: {lesson}", "Продолжить: {lesson}"),
        "cur.hint" => ("Click any adventure to start it.", "Любое приключение можно начать — просто нажмите на него."),
        "cur.back" => ("Children", "Дети"),
        "lesson.speaking" => ("Speaking", "Говорит"),
        "lesson.listening" => ("Listening", "Слушает"),
        "lesson.watching" => ("Watching the table", "Смотрит на стол"),
        "lesson.celebrating" => ("Correct!", "Верно!"),
        "lesson.muted" => ("Type a number", "Нажми на число"),
        "lesson.heard" => ("Heard", "Услышано"),
        "lesson.numpadHint" => ("The microphone can't hear right now.", "Микрофон сейчас не слышит."),
        "lesson.pause" => ("Pause", "Пауза"),
        "lesson.repeat" => ("Repeat", "Повторить"),
        "lesson.skip" => ("Skip", "Пропустить"),
        "lesson.pauseContinue" => ("Pause / continue", "Пауза / продолжить"),
        "lesson.settings" => ("Settings", "Настройки"),
        "lesson.pausedTitle" => ("Paused", "Пауза"),
        "lesson.pausedBody" => ("Your place is saved.", "Мы запомнили, где остановились."),
        "lesson.resume" => ("Continue", "Продолжить"),
        "lesson.back" => ("Back to adventures", "К приключениям"),
        "lesson.sim" => ("Simulated table", "Симулированный стол"),
        "prep.title" => ("Getting the camera ready", "Готовим камеру"),
        "prep.body" => (
            "Only on first launch — up to a minute. Meanwhile, put a few toys on the table.",
            "Только при первом запуске — до минуты. А пока поставьте на стол несколько игрушек.",
        ),
        "prep.step" => ("Loading toy recognition", "Загружаем распознавание игрушек"),
        "error.title" => ("Toy recognition didn't load", "Не удалось загрузить распознавание игрушек"),
        "error.body" => (
            "Progress is saved. Try again — if it keeps failing, check the camera in Settings.",
            "Прогресс сохранён. Попробуйте ещё раз, а если не получится — проверьте камеру в настройках.",
        ),
        "error.retry" => ("Try again", "Попробовать снова"),
        "end.hooray" => ("Hooray, {name}!", "Ура, {name}!"),
        "end.done" => ("{lesson} — done!", "«{lesson}» — пройдено!"),
        "end.ask" => ("Shall we go on?", "Продолжим?"),
        "end.yes" => ("Yes!", "Да!"),
        "end.no" => ("No", "Нет"),
        "end.bye" => ("See you next time, {name}!", "До встречи, {name}!"),
        "end.nextTime" => ("Next time: {lesson}", "В следующий раз: {lesson}"),
        "end.home" => ("Back to children", "К списку детей"),
        "settings.title" => ("Settings", "Настройки"),
        "settings.paused" => ("The lesson is paused while Settings is open.", "Пока открыты настройки, урок на паузе."),
        "settings.devices" => ("Devices", "Устройства"),
        "settings.camera" => ("Camera", "Камера"),
        "settings.mirror" => ("Mirror the picture", "Зеркальное изображение"),
        "settings.mic" => ("Microphone", "Микрофон"),
        "settings.micDefault" => ("System default", "Системный по умолчанию"),
        "settings.voice" => ("Tutor voice", "Голос наставника"),
        "settings.voiceAuto" => ("Built-in voice", "Встроенный голос"),
        "settings.voiceText" => ("Text only", "Только текст"),
        "settings.voiceOff" => ("Off", "Выключен"),
        "settings.area" => ("Table area", "Область стола"),
        "settings.areaHint" => (
            "Drag the frame to where the toys go. Toys outside it aren't counted.",
            "Перетащите рамку туда, где лежат игрушки. Игрушки за рамкой не считаются.",
        ),
        "settings.reset" => ("Reset", "Сбросить"),
        "settings.motion" => ("Reduce motion", "Меньше анимации"),
        "settings.motionHint" => (
            "Fewer animations. Numbers still light up as they are counted.",
            "Меньше анимации. Числа по-прежнему подсвечиваются при счёте.",
        ),
        "settings.sim" => ("Simulated table", "Симулированный стол"),
        "settings.simHint" => (
            "No camera: a drawn table with toys you add and take away.",
            "Без камеры: нарисованный стол, игрушки добавляются и убираются.",
        ),
        "settings.toys" => ("Toys on the table", "Игрушек на столе"),
        "settings.shortcuts" => ("Keyboard", "Клавиши"),
        "settings.close" => ("Close", "Закрыть"),
        "key.space" => ("Space", "Пробел"),
        _ => (key_leak(key), key_leak(key)),
    };
    if ru {
        rus
    } else {
        en
    }
}

/// An unknown key shows as itself (a missing string is visible, not a crash).
fn key_leak(key: &str) -> &'static str {
    Box::leak(key.to_string().into_boxed_str())
}

/// [`t`] with `{name}` placeholders filled in.
pub fn tf(ru: bool, key: &str, args: &[(&str, &str)]) -> String {
    let mut text = t(ru, key).to_string();
    for (k, v) in args {
        text = text.replace(&format!("{{{k}}}"), v);
    }
    text
}

pub fn is_ru(language: Language) -> bool {
    language == Language::Russian
}

/// The ten adventures as the app shows them: name and skill in both languages, by place in the
/// curriculum (the library's titles are English-only "Lesson N: ..." strings).
pub struct LessonMeta {
    pub number: usize,
    pub title: String,
    pub skill: String,
}

const LESSONS: [(&str, &str, &str, &str); 10] = [
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
];

impl LessonMeta {
    pub fn of(id: &str, lessons: &[(String, String)], ru: bool) -> Self {
        let index = lessons.iter().position(|(i, _)| i == id).unwrap_or(0);
        if let Some(row) = LESSONS.get(index) {
            return Self {
                number: index + 1,
                title: (if ru { row.1 } else { row.0 }).to_string(),
                skill: (if ru { row.3 } else { row.2 }).to_string(),
            };
        }
        let title = lessons.iter().find(|(i, _)| i == id).map(|(_, t)| t.clone()).unwrap_or_else(|| id.to_string());
        let short = title.split_once(':').map(|(_, s)| s.trim().to_string()).unwrap_or(title);
        Self { number: index + 1, title: short, skill: String::new() }
    }
}

// ------------------------------------------------------------------ icons

/// The icons, drawn rather than taken from a font (so they never miss a glyph).
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    Check,
    Pause,
    Repeat,
    Skip,
    Gear,
    Plus,
    Ellipsis,
    ChevronLeft,
    Close,
    Star,
    Mic,
    MicSlash,
    Speaker,
    Eye,
    Moon,
    Play,
}

pub fn icon(painter: &egui::Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Pos2::new(center.x + (x - 12.0) * s, center.y + (y - 12.0) * s);
    let stroke = Stroke::new(2.2 * s, color);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|&(x, y)| p(x, y)).collect(), stroke);
    match icon {
        Icon::Check => {
            painter.add(line(&[(4.5, 12.5), (9.5, 17.5), (19.5, 7.0)]));
        }
        Icon::Pause => {
            painter.rect_filled(Rect::from_min_max(p(6.5, 5.0), p(10.0, 19.0)), 1.0 * s, color);
            painter.rect_filled(Rect::from_min_max(p(14.0, 5.0), p(17.5, 19.0)), 1.0 * s, color);
        }
        Icon::Repeat => {
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = -PI * 0.2 + i as f32 / 20.0 * PI * 1.55;
                    p(12.0 + 7.0 * a.cos(), 12.0 + 7.0 * a.sin())
                })
                .collect();
            let end = pts[0];
            painter.add(Shape::line(pts, stroke));
            painter.add(Shape::line(vec![end + Vec2::new(-4.0, -3.5) * s, end, end + Vec2::new(-4.5, 2.0) * s], stroke));
        }
        Icon::Skip => {
            painter.add(Shape::convex_polygon(vec![p(5.0, 5.0), p(16.0, 12.0), p(5.0, 19.0)], color, Stroke::NONE));
            painter.rect_filled(Rect::from_min_max(p(17.0, 5.0), p(19.5, 19.0)), 0.5 * s, color);
        }
        Icon::Gear => {
            painter.circle_stroke(center, 3.4 * s, stroke);
            for i in 0..8 {
                let a = i as f32 / 8.0 * PI * 2.0;
                let (sn, cs) = a.sin_cos();
                painter.line_segment([p(12.0 + 6.0 * cs, 12.0 + 6.0 * sn), p(12.0 + 9.0 * cs, 12.0 + 9.0 * sn)], stroke);
            }
            painter.circle_stroke(center, 6.2 * s, stroke);
        }
        Icon::Plus => {
            painter.line_segment([p(12.0, 4.0), p(12.0, 20.0)], stroke);
            painter.line_segment([p(4.0, 12.0), p(20.0, 12.0)], stroke);
        }
        Icon::Ellipsis => {
            for x in [5.0, 12.0, 19.0] {
                painter.circle_filled(p(x, 12.0), 1.6 * s, color);
            }
        }
        Icon::ChevronLeft => {
            painter.add(line(&[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)]));
        }
        Icon::Close => {
            painter.line_segment([p(6.0, 6.0), p(18.0, 18.0)], stroke);
            painter.line_segment([p(18.0, 6.0), p(6.0, 18.0)], stroke);
        }
        Icon::Star => {
            painter.add(star(center, size * 0.5, color));
        }
        Icon::Mic | Icon::MicSlash => {
            painter.rect_stroke(Rect::from_min_max(p(9.0, 3.0), p(15.0, 14.0)), 3.0 * s, stroke, StrokeKind::Middle);
            let arc: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = PI * i as f32 / 16.0;
                    p(12.0 + 6.5 * a.cos(), 11.5 + 6.5 * a.sin())
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            painter.line_segment([p(12.0, 18.0), p(12.0, 21.5)], stroke);
            if icon == Icon::MicSlash {
                painter.line_segment([p(4.0, 3.5), p(20.0, 20.5)], stroke);
            }
        }
        Icon::Speaker => {
            painter.add(Shape::convex_polygon(
                vec![p(3.5, 9.5), p(7.0, 9.5), p(11.5, 5.5), p(11.5, 18.5), p(7.0, 14.5), p(3.5, 14.5)],
                color,
                Stroke::NONE,
            ));
            for r in [4.0, 7.5] {
                let arc: Vec<Pos2> = (0..=10)
                    .map(|i| {
                        let a = -PI / 3.0 + i as f32 / 10.0 * PI * 2.0 / 3.0;
                        p(12.0 + r * a.cos(), 12.0 + r * a.sin())
                    })
                    .collect();
                painter.add(Shape::line(arc, stroke));
            }
        }
        Icon::Eye => {
            let top: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let x = 2.5 + 19.0 * i as f32 / 16.0;
                    p(x, 12.0 - 6.5 * (PI * i as f32 / 16.0).sin())
                })
                .collect();
            let bottom: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let x = 2.5 + 19.0 * i as f32 / 16.0;
                    p(x, 12.0 + 6.5 * (PI * i as f32 / 16.0).sin())
                })
                .collect();
            painter.add(Shape::line(top, stroke));
            painter.add(Shape::line(bottom, stroke));
            painter.circle_stroke(center, 3.0 * s, stroke);
        }
        Icon::Moon => {
            // A crescent: a thick arc, its ends thinned by rounding.
            let arc: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = PI * 0.35 + i as f32 / 24.0 * PI * 1.3;
                    p(12.0 + 7.0 * a.cos(), 12.0 + 7.0 * a.sin())
                })
                .collect();
            painter.add(Shape::line(arc, Stroke::new(4.5 * s, color)));
            painter.add(star(p(17.5, 6.5), 2.6 * s, color));
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(vec![p(8.0, 5.0), p(19.0, 12.0), p(8.0, 19.0)], color, Stroke::NONE));
        }
    };
}

/// A five-pointed star.
pub fn star(center: Pos2, radius: f32, color: Color32) -> Shape {
    let points: Vec<Pos2> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { radius } else { radius * 0.45 };
            let a = -PI / 2.0 + i as f32 * PI / 5.0;
            center + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect();
    // Not convex: fan from the centre.
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for p in &points {
        mesh.colored_vertex(*p, color);
    }
    for i in 0..10u32 {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
    }
    Shape::mesh(mesh)
}

// ------------------------------------------------------------------ widgets

/// The keyboard focus ring: 3px blue, 2px off the shape.
pub fn focus_ring(ui: &Ui, response: &Response, rect: Rect, radius: f32) {
    if response.has_focus() {
        let theme = Theme::of(ui);
        ui.painter().rect_stroke(rect.expand(2.0), radius + 2.0, Stroke::new(3.0, theme.blue), StrokeKind::Outside);
    }
}

/// A soft drop shadow under `rect`.
pub fn shadow(painter: &egui::Painter, rect: Rect, radius: f32, dark: bool) {
    let shadow = egui::Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    };
    painter.add(shadow.as_shape(rect, CornerRadius::same(radius as u8)));
}

/// Onym's primary button: blue, 50px, 14px corners; hover darkens it.
pub fn primary_button(ui: &mut Ui, text: &str, width: f32, tint: Option<Color32>) -> Response {
    let theme = Theme::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 50.0), Sense::click());
    let base = tint.unwrap_or(theme.blue);
    let fill = if response.hovered() { base.gamma_multiply(0.88) } else { base };
    ui.painter().rect_filled(rect, 14.0, fill);
    let galley = ui.painter().layout(text.to_string(), semibold(16.0), theme.on_accent, width - 24.0);
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, theme.on_accent);
    focus_ring(ui, &response, rect, 14.0);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A capsule button with an optional icon (back "‹ Children", Reset, ...).
pub fn pill_button(ui: &mut Ui, icon_left: Option<Icon>, text: &str, fill: Color32, color: Color32) -> Response {
    let font = semibold(15.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let pad = 16.0;
    let icon_w = if icon_left.is_some() { 22.0 } else { 0.0 };
    let size = Vec2::new(galley.size().x + pad * 2.0 + icon_w, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let theme = Theme::of(ui);
    let fill = if response.hovered() { theme.surface3 } else { fill };
    ui.painter().rect_filled(rect, 20.0, fill);
    ui.painter().rect_stroke(rect, 20.0, Stroke::new(1.0, theme.hairline_strong), StrokeKind::Inside);
    let mut x = rect.left() + pad;
    if let Some(i) = icon_left {
        icon(ui.painter(), i, Pos2::new(x + 8.0, rect.center().y), 18.0, color);
        x += icon_w;
    }
    ui.painter().galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley, color);
    focus_ring(ui, &response, rect, 20.0);
    response
}

/// A round icon button (gear, "…", close); hover fills it with surface3.
pub fn circle_button(ui: &mut Ui, icon_kind: Icon, diameter: f32, fill: Color32, tooltip: &str) -> Response {
    let theme = Theme::of(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let fill = if response.hovered() { theme.surface3 } else { fill };
    ui.painter().circle_filled(rect.center(), diameter / 2.0, fill);
    ui.painter().circle_stroke(rect.center(), diameter / 2.0, Stroke::new(1.0, theme.hairline));
    icon(ui.painter(), icon_kind, rect.center(), diameter * 0.45, theme.text2);
    focus_ring(ui, &response, rect, diameter / 2.0);
    response.on_hover_text(tooltip)
}

/// The EN/RU capsule switch.
pub fn language_switch(ui: &mut Ui, language: &mut Language) -> bool {
    let theme = Theme::of(ui);
    let mut changed = false;
    let (outer, _) = ui.allocate_exact_size(Vec2::new(90.0, 38.0), Sense::hover());
    ui.painter().rect_filled(outer, 19.0, theme.surface3);
    for (i, (lang, label)) in [(Language::English, "EN"), (Language::Russian, "RU")].into_iter().enumerate() {
        let rect = Rect::from_min_size(outer.min + Vec2::new(3.0 + i as f32 * 42.0, 3.0), Vec2::new(42.0, 32.0));
        let response = ui.interact(rect, ui.id().with(("lang", i)), Sense::click());
        let on = *language == lang;
        if on {
            ui.painter().rect_filled(rect, 16.0, theme.surface2);
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            semibold(13.0),
            if on { theme.text } else { theme.text2 },
        );
        focus_ring(ui, &response, rect, 16.0);
        if response.clicked() && !on {
            *language = lang;
            changed = true;
        }
    }
    changed
}

/// A card's surface: surface2 with 14px corners (and a hairline over the camera).
pub fn card(painter: &egui::Painter, rect: Rect, theme: &Theme, radius: f32, over_camera: bool) {
    if over_camera {
        shadow(painter, rect, radius, theme.dark);
        painter.rect_filled(rect, radius, theme.panel());
        painter.rect_stroke(rect, radius, Stroke::new(1.0, theme.hairline_strong), StrokeKind::Inside);
    } else {
        painter.rect_filled(rect, radius, theme.surface2);
    }
}

/// Text laid out to `width` at `pos` (top-left); returns its height.
pub fn text_block(painter: &egui::Painter, pos: Pos2, text: &str, font: FontId, color: Color32, width: f32) -> f32 {
    let galley = painter.layout(text.to_string(), font, color, width);
    let h = galley.size().y;
    painter.galley(pos, galley, color);
    h
}

/// The height `text` takes at `width`.
pub fn text_height(ui: &Ui, text: &str, font: FontId, width: f32) -> f32 {
    ui.painter().layout(text.to_string(), font, Color32::WHITE, width).size().y
}

/// An indeterminate progress bar: a blue sheen sweeping along the track.
pub fn shimmer(painter: &egui::Painter, rect: Rect, theme: &Theme, time: f64, still: bool) {
    painter.rect_filled(rect, rect.height() / 2.0, theme.surface3);
    let w = rect.width() * 0.35;
    let phase = if still { 0.0 } else { (time % 1.6 / 1.6) as f32 };
    let x = rect.left() - w + (rect.width() + w) * phase;
    let bar = Rect::from_min_max(Pos2::new(x.max(rect.left()), rect.top()), Pos2::new((x + w).min(rect.right()), rect.bottom()));
    if bar.width() > 0.0 {
        painter.rect_filled(bar, rect.height() / 2.0, theme.blue);
    }
}

/// A key cap for the shortcut hints ("Space", "N").
pub fn key_cap(painter: &egui::Painter, left: Pos2, key: &str, fill: Color32, stroke: Color32, color: Color32) -> f32 {
    let galley = painter.layout_no_wrap(key.to_string(), semibold(11.5), color);
    let rect = Rect::from_min_size(left - Vec2::new(0.0, 11.0), Vec2::new(galley.size().x + 12.0, 22.0));
    painter.rect_filled(rect, 5.0, fill);
    painter.rect_stroke(rect, 5.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    rect.width()
}
