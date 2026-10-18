package app.onym.un

import android.provider.Settings
import androidx.compose.animation.core.withInfiniteAnimationFrameMillis
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.PhotoCamera
import androidx.compose.material.icons.rounded.Replay
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.un_ffi.*
import kotlin.math.roundToInt

/**
 * The Un design, as on iOS (`Un.swift`): Onym tokens for the grown-up layer, toy accents and the
 * camera-overlay tokens for the child's layer (the "Un Design Board" in Claude Design). Colours
 * follow light and dark mode, except the overlay ones: the camera picture looks the same in both.
 */
class Palette(dark: Boolean) {
    private fun pick(light: Long, dark_: Long, alpha: Float = 1f) = Color(if (dark) dark_ else light).copy(alpha = alpha)

    // Grown-up layer (Onym).
    val surface = pick(0xFFF5F5F7, 0xFF0E0E10)
    val surface2 = pick(0xFFFFFFFF, 0xFF17171A)
    val surface3 = pick(0xFFEBEBEF, 0xFF1F1F23)
    val text = pick(0xFF0A0A0C, 0xFFF2F2F4)
    val text2 = pick(0xFF0A0A0C, 0xFFF2F2F4, 0.62f)
    val text3 = pick(0xFF0A0A0C, 0xFFF2F2F4, 0.42f)
    val hairline = pick(0xFF000000, 0xFFFFFFFF, 0.08f)
    val blue = pick(0xFF1F86E0, 0xFF3FA8FF)
    val green = pick(0xFF1FA84A, 0xFF34C759)
    val red = pick(0xFFE5392E, 0xFFFF453A)
    val onAccent = pick(0xFFFFFFFF, 0xFF000000)
    // Child layer: toy accents.
    val orange = pick(0xFFE85F2A, 0xFFFF7A45)
    val purple = pick(0xFF8B4DEB, 0xFFB278FF)
    val yellow = pick(0xFFD9A400, 0xFFFFC93C)
    val pink = pick(0xFFE03253, 0xFFFF4D6D)
    val accentGreen = pick(0xFF1FA84A, 0xFF3DD66E)
    val accentBlue = pick(0xFF1F86E0, 0xFF3FA8FF)
    /** Each child's colour, by their place in the list. */
    val accents = listOf(orange, accentBlue, purple, accentGreen, pink, yellow)
    /** The glass of the lesson's controls over the camera (Android has no Liquid Glass). */
    val glass = pick(0xFFFFFFFF, 0xFF1F1F23, 0.78f)
    val glassStrong = pick(0xFFF5F5F7, 0xFF17171A, 0.94f)
    val dark = dark
}

val LocalPalette = staticCompositionLocalOf { Palette(false) }

object Un {
    val c: Palette @Composable @ReadOnlyComposable get() = LocalPalette.current
    val gray = Color(0xFF8E8E93)
    // Camera overlay: fixed in both themes.
    val badgeIdle = Color.White
    val badgeLit = Color(0xFFFFC93C)
    val badgeCounted = Color(0xFF1F86E0)
    val badgeCorrect = Color(0xFF1FA84A)
    val correctStroke = Color(0xFF34C759)
    val halo = Color.Black.copy(alpha = 0.55f)
    val ink = Color(0xFF0A0A0C)

    /** SF Rounded's stand-in: Nunito (OFL), a variable font, heavy by default. */
    private val nunito = mutableMapOf<Int, FontFamily>()
    @OptIn(androidx.compose.ui.text.ExperimentalTextApi::class)
    fun roundedFamily(weight: Int = 900): FontFamily = nunito.getOrPut(weight) {
        FontFamily(Font(R.font.nunito, FontWeight(weight), variationSettings = FontVariation.Settings(FontVariation.weight(weight))))
    }

    fun rounded(size: TextUnit, weight: Int = 900) =
        TextStyle(fontFamily = roundedFamily(weight), fontWeight = FontWeight(weight), fontSize = size)
}

/** The app's theme: the palette for the screens, Material colours for the dialogs and sheets. */
@Composable
fun UnTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    val palette = remember(dark) { Palette(dark) }
    val scheme = if (dark) darkColorScheme(
        primary = palette.blue, onPrimary = palette.onAccent, background = palette.surface, surface = palette.surface2,
        onSurface = palette.text, onBackground = palette.text, error = palette.red, surfaceContainerHigh = palette.surface2,
        surfaceContainerLow = palette.surface, surfaceContainer = palette.surface2,
    ) else lightColorScheme(
        primary = palette.blue, onPrimary = palette.onAccent, background = palette.surface, surface = palette.surface2,
        onSurface = palette.text, onBackground = palette.text, error = palette.red, surfaceContainerHigh = palette.surface2,
        surfaceContainerLow = palette.surface, surfaceContainer = palette.surface2,
    )
    CompositionLocalProvider(LocalPalette provides palette) {
        MaterialTheme(colorScheme = scheme, content = content)
    }
}

/** The system's "Remove animations" (iOS: Reduce Motion). */
@Composable
fun reduceMotion(): Boolean {
    val context = LocalContext.current
    return remember {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
}

/** Seconds since the composable appeared, ticking every frame (iOS: `TimelineView(.animation)`). */
@Composable
fun rememberTime(paused: Boolean = false): State<Double> {
    val time = remember { mutableDoubleStateOf(0.0) }
    LaunchedEffect(paused) {
        if (paused) return@LaunchedEffect
        val start = withInfiniteAnimationFrameMillis { it }
        while (true) withInfiniteAnimationFrameMillis { time.doubleValue = (it - start) / 1000.0 }
    }
    return time
}

/** Places the content centred on (x, y) of its parent, whatever its size (SwiftUI's `.position`). */
fun Modifier.centerAt(x: Dp, y: Dp) = layout { measurable, constraints ->
    val p = measurable.measure(constraints.copy(minWidth = 0, minHeight = 0))
    layout(p.width, p.height) {
        p.place((x.toPx() - p.width / 2f).roundToInt(), (y.toPx() - p.height / 2f).roundToInt())
    }
}

/** Glass in `shape`; `strong` (text over the camera) is the opaque "glass-strong" surface. */
@Composable
fun Modifier.unGlass(shape: Shape, strong: Boolean = false): Modifier {
    val c = Un.c
    return this.background(if (strong) c.glassStrong else c.glass, shape)
        .border(BorderStroke(0.5.dp, Color.White.copy(alpha = 0.35f)), shape)
}

/** The ten adventures as the app shows them: name and skill in both languages, by place in the curriculum. */
data class LessonMeta(val number: Int, val title: String, val skill: String) {
    companion object {
        private val table = listOf(
            listOf("Three passengers", "Три пассажира", "Counting to 3", "Счёт до 3"),
            listOf("Picnic baskets", "Корзинки для пикника", "Adding to 5", "Сложение до 5"),
            listOf("Cave train", "Поезд в пещере", "Taking away", "Вычитание"),
            listOf("Bridge builders", "Строители мостов", "Comparing", "Сравнение"),
            listOf("Parcel detectives", "Детективы посылок", "Numbers and amounts", "Число и количество"),
            listOf("Firefly gates", "Ворота светлячков", "Counting to 10", "Счёт до 10"),
            listOf("Supply launch", "Запуск припасов", "Adding to 10", "Сложение до 10"),
            listOf("Homeward trail", "Тропа домой", "Taking away to 10", "Вычитание до 10"),
            listOf("Ten-seat ferry", "Паром на десять мест", "Making ten", "Состав числа 10"),
            listOf("Fair-share cafe", "Кафе «Всем поровну»", "Sharing equally", "Делим поровну"),
        )

        fun of(id: String, lessons: List<UnStrategyInfo>, ru: Boolean): LessonMeta {
            val index = lessons.indexOfFirst { it.id == id }.coerceAtLeast(0)
            if (index < table.size) {
                val row = table[index]
                return LessonMeta(index + 1, if (ru) row[1] else row[0], if (ru) row[3] else row[2])
            }
            val title = lessons.firstOrNull { it.id == id }?.title ?: id
            return LessonMeta(index + 1, title.substringAfter(':').trim(), "")
        }
    }
}

/**
 * Every string the screens use, English and Russian, from the design's one copy source (the same
 * table as iOS). The app addresses the grown-up formally (вы); names stay in the nominative.
 */
class Copy(val ru: Boolean) {
    operator fun invoke(key: String, vararg args: Pair<String, String>): String {
        var text = (if (ru) RU else EN)[key] ?: key
        for ((k, v) in args) text = text.replace("{$k}", v)
        return text
    }

    companion object {
        private val EN = mapOf(
            "children.title" to "Children", "children.progress" to "{n} of {total} lessons done", "children.next" to "Next: {lesson}",
            "children.resume" to "Paused in: {lesson}", "children.cont" to "Continue", "children.add" to "Add a child",
            "children.more" to "Options for {name}", "children.emptyTitle" to "Who's counting today?",
            "children.emptyBody" to "Add a child to begin the first adventure. A name is all Un needs.",
            "add.title" to "New child", "add.placeholder" to "Name", "add.helper" to "The tutor will call them by this name.",
            "add.confirm" to "Add", "add.cancel" to "Cancel",
            "remove.title" to "Remove {name}?", "remove.body" to "Their progress on this device will be deleted. This can't be undone.",
            "remove.confirm" to "Remove", "remove.cancel" to "Cancel",
            "cur.sub" to "{n} of {total} done", "cur.nextLabel" to "Next adventure", "cur.pausedLabel" to "Continue where you stopped",
            "cur.lessonN" to "Lesson {n}", "cur.done" to "Done", "cur.next" to "Up next", "cur.paused" to "Paused here",
            "cur.cta" to "Continue: {lesson}", "cur.hint" to "Tap any adventure to start it.", "cur.back" to "Children",
            "lesson.speaking" to "Speaking", "lesson.listening" to "Listening", "lesson.watching" to "Watching the table",
            "lesson.celebrating" to "Correct!", "lesson.muted" to "Tap a number", "lesson.heard" to "Heard",
            "lesson.numpadHint" to "The microphone can't hear right now.", "lesson.back" to "Back to adventures", "lesson.pause" to "Pause",
            "lesson.repeat" to "Repeat", "lesson.skip" to "Skip", "lesson.language" to "Language", "lesson.toy" to "Toy {n}, {state}",
            "lesson.idle" to "not counted yet", "lesson.lit" to "counting now", "lesson.counted" to "counted", "lesson.done" to "correct",
            "prep.title" to "Getting the camera ready",
            "prep.body" to "Only on first launch — up to a minute. Meanwhile, put a few toys on the table.",
            "prep.step" to "Loading toy recognition",
            "error.title" to "Toy recognition didn't load",
            "error.body" to "Progress is saved. Try again — if it keeps failing, restart the app.",
            "error.retry" to "Try again", "error.back" to "Back to adventures",
            "end.hooray" to "Hooray, {name}!", "end.done" to "{lesson} — done!", "end.ask" to "Shall we go on?", "end.yes" to "Yes!",
            "end.no" to "No", "end.rest" to "Rest for today", "end.bye" to "See you next time, {name}!",
            "end.nextTime" to "Next time: {lesson}", "end.home" to "Back to children",
        )

        private val RU = mapOf(
            "children.title" to "Дети", "children.progress" to "Пройдено {n} из {total} уроков", "children.next" to "Дальше: {lesson}",
            "children.resume" to "Остановились на: {lesson}", "children.cont" to "Продолжить", "children.add" to "Добавить ребёнка",
            "children.more" to "Действия: {name}", "children.emptyTitle" to "Кто сегодня считает?",
            "children.emptyBody" to "Добавьте ребёнка, чтобы начать первое приключение. Нужно только имя.",
            "add.title" to "Новый ребёнок", "add.placeholder" to "Имя", "add.helper" to "Так наставник будет обращаться к ребёнку.",
            "add.confirm" to "Добавить", "add.cancel" to "Отмена",
            "remove.title" to "Удалить профиль «{name}»?", "remove.body" to "Прогресс на этом устройстве будет удалён. Это действие нельзя отменить.",
            "remove.confirm" to "Удалить", "remove.cancel" to "Отмена",
            "cur.sub" to "{n} из {total} пройдено", "cur.nextLabel" to "Следующее приключение", "cur.pausedLabel" to "Продолжить с места остановки",
            "cur.lessonN" to "Урок {n}", "cur.done" to "Пройдено", "cur.next" to "Следующий", "cur.paused" to "На паузе",
            "cur.cta" to "Продолжить: {lesson}", "cur.hint" to "Любое приключение можно начать — просто нажмите на него.", "cur.back" to "Дети",
            "lesson.speaking" to "Говорит", "lesson.listening" to "Слушает", "lesson.watching" to "Смотрит на стол",
            "lesson.celebrating" to "Верно!", "lesson.muted" to "Нажми на число", "lesson.heard" to "Услышано",
            "lesson.numpadHint" to "Микрофон сейчас не слышит.", "lesson.back" to "К приключениям", "lesson.pause" to "Пауза",
            "lesson.repeat" to "Повторить", "lesson.skip" to "Пропустить", "lesson.language" to "Язык", "lesson.toy" to "Игрушка {n}, {state}",
            "lesson.idle" to "ещё не посчитана", "lesson.lit" to "считаем сейчас", "lesson.counted" to "посчитана", "lesson.done" to "верно",
            "prep.title" to "Готовим камеру",
            "prep.body" to "Только при первом запуске — до минуты. А пока поставьте на стол несколько игрушек.",
            "prep.step" to "Загружаем распознавание игрушек",
            "error.title" to "Не удалось загрузить распознавание игрушек",
            "error.body" to "Прогресс сохранён. Попробуйте ещё раз, а если не получится — перезапустите приложение.",
            "error.retry" to "Попробовать снова", "error.back" to "К приключениям",
            "end.hooray" to "Ура, {name}!", "end.done" to "«{lesson}» — пройдено!", "end.ask" to "Продолжим?", "end.yes" to "Да!",
            "end.no" to "Нет", "end.rest" to "Отдохнём", "end.bye" to "До встречи, {name}!",
            "end.nextTime" to "В следующий раз: {lesson}", "end.home" to "К списку детей",
        )
    }
}

/** The capsule EN/RU switch next to the wordmark and on the trail. */
@Composable
fun LanguageSwitch(language: UnLanguage, onChange: (UnLanguage) -> Unit) {
    val c = Un.c
    Row(
        Modifier.clip(CircleShape).background(c.surface3).padding(3.dp).semantics { contentDescription = "Language" },
        horizontalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        for ((lang, label) in listOf(UnLanguage.ENGLISH to "EN", UnLanguage.RUSSIAN to "RU")) {
            val on = language == lang
            Box(
                Modifier.clip(CircleShape).background(if (on) c.surface2 else Color.Transparent)
                    .clickable { onChange(lang) }.defaultMinSize(40.dp, 32.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, color = if (on) c.text else c.text2)
            }
        }
    }
}

/** Onym's primary button: full-width blue, 50dp, 14dp corners. */
@Composable
fun PrimaryButton(title: String, modifier: Modifier = Modifier, icon: ImageVector? = null, tint: Color = Un.c.blue, onClick: () -> Unit) {
    val c = Un.c
    Row(
        modifier.fillMaxWidth().defaultMinSize(minHeight = 50.dp).clip(RoundedCornerShape(14.dp)).background(tint)
            .clickable(onClick = onClick).padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, null, tint = c.onAccent, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(7.dp))
        }
        Text(title, fontSize = 16.sp, fontWeight = FontWeight.SemiBold, color = c.onAccent, textAlign = TextAlign.Center)
    }
}

/** The first-launch card: the detector is loading (up to a minute), or failed to load. */
@Composable
fun CameraStatusCard(model: LessonModel, modifier: Modifier = Modifier, overCamera: Boolean = false, onBack: (() -> Unit)? = null) {
    val c = Un.c
    val preparing by model.preparingCamera.collectAsState()
    val failed by model.detectorFailed.collectAsState()
    val t = Copy(model.language.collectAsState().value == UnLanguage.RUSSIAN)
    if (!preparing && !failed) return
    val surface = if (overCamera) Modifier.unGlass(RoundedCornerShape(22.dp), strong = true)
                  else Modifier.background(c.surface2, RoundedCornerShape(14.dp))
    Column(modifier.widthIn(max = 520.dp).fillMaxWidth().then(surface).padding(20.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (preparing) {
            Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                for (dot in listOf(c.orange, c.blue, c.green)) Box(Modifier.size(9.dp).background(dot, CircleShape))
            }
            Text(t("prep.title"), fontSize = 20.sp, fontWeight = FontWeight.Bold, color = c.text)
            Text(t("prep.body"), fontSize = 15.sp, color = c.text2)
            ShimmerBar()
            Text(t("prep.step"), fontSize = 12.5.sp, color = c.text2)
        } else {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.PhotoCamera, null, tint = c.text, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text(t("error.title"), fontSize = 18.sp, fontWeight = FontWeight.Bold, color = c.text)
            }
            Text(t("error.body"), fontSize = 15.sp, color = c.text2)
            PrimaryButton(t("error.retry"), icon = Icons.Rounded.Replay) { model.preparePerception() }
            if (onBack != null) {
                TextButton(onClick = onBack, Modifier.fillMaxWidth().defaultMinSize(minHeight = 44.dp)) {
                    Text(t("error.back"), fontSize = 15.sp, fontWeight = FontWeight.Medium, color = c.blue)
                }
            }
        }
    }
}

/** An indeterminate progress bar: a blue sheen sweeping along the track. A plain bar without animations. */
@Composable
fun ShimmerBar() {
    val c = Un.c
    val still = reduceMotion()
    val time by rememberTime(paused = still)
    BoxWithConstraints(Modifier.fillMaxWidth().height(6.dp).clip(CircleShape).background(c.surface3)) {
        val phase = ((time % 1.6) / 1.6).toFloat()
        val bar = maxWidth * 0.35f
        val x = if (still) 0.dp else maxWidth * 1.35f * phase - bar
        Box(Modifier.offset(x = x).width(bar).fillMaxHeight().clip(CircleShape).background(c.blue))
    }
}

/** A circle button on glass (back, the lesson's tools). */
@Composable
fun GlassCircleButton(icon: ImageVector, label: String, onClick: () -> Unit, glass: Boolean = true, size: Dp = 44.dp) {
    val c = Un.c
    Box(
        Modifier.size(size).clip(CircleShape).then(if (glass) Modifier.unGlass(CircleShape) else Modifier)
            .clickable(onClick = onClick).semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = c.text, modifier = Modifier.size(20.dp))
    }
}
