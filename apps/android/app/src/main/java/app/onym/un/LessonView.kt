package app.onym.un

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBackIos
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import uniffi.un_ffi.*
import kotlin.math.cos
import kotlin.math.sin

/**
 * The child's screen (as on iOS): the live camera is the hero. Only three things sit over it: the
 * toy boxes with their number badges, one tutor panel at the bottom, and at the top a back button
 * (it pauses) and a small parent toolbar (Pause, Repeat, Skip, language). The number pad appears
 * only when speech fails; the end screens follow a completed lesson.
 */
@Composable
fun LessonView(model: LessonModel) {
    val snapshot by model.snapshot.collectAsState()
    val language by model.language.collectAsState()
    val lessons by model.lessons.collectAsState()
    val lessonId by model.lessonId.collectAsState()
    val table by model.table.collectAsState()
    val frameSize by model.frameSize.collectAsState()
    val state by model.tutorState.collectAsState()
    val endPhase by model.endPhase.collectAsState()
    val preparing by model.preparingCamera.collectAsState()
    val failed by model.detectorFailed.collectAsState()
    val ru = language == UnLanguage.RUSSIAN
    val t = Copy(ru)
    val meta = LessonMeta.of(lessonId, lessons, ru)
    val celebrating = state == LessonModel.TutorState.CELEBRATING
    // Back (the system gesture, as the button top left): pauses the lesson and returns to the trail.
    BackHandler { model.pause() }
    BoxWithConstraints(Modifier.fillMaxSize().background(Color.Black)) {
        val compact = maxWidth < 600.dp
        AndroidView(factory = { model.previewView }, modifier = Modifier.fillMaxSize())
        TableOverlay(table, frameSize, snapshot?.highlightCount, celebrating, t, Modifier.fillMaxSize())
        val dim by animateFloatAsState(if (celebrating) 0.66f else 0f, tween(600), label = "dim")
        Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = dim)))
        Column(
            Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(horizontal = 16.dp).padding(bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            TopBar(model, meta, language, t)
            Spacer(Modifier.weight(1f))
            if (snapshot?.showNumberPad == true) NumberPad(model, t)
            if (preparing || failed) {
                CameraStatusCard(model, overCamera = true, onBack = { model.end() })
            } else {
                TutorPanel(snapshot, state, t, compact)
            }
        }
        if (celebrating && endPhase == null) {
            CountTicket(table?.count ?: snapshot?.targetCount ?: 0u)
        }
        AnimatedContent(endPhase, transitionSpec = { fadeIn(tween(400)) togetherWith fadeOut(tween(400)) }, label = "end") { phase ->
            if (phase != null) EndView(model, phase, meta, t)
        }
    }
}

@Composable
private fun TopBar(model: LessonModel, meta: LessonMeta, language: UnLanguage, t: Copy) {
    val c = Un.c
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        // Back: pauses the lesson (the place is saved) and returns to the trail.
        GlassCircleButton(Icons.AutoMirrored.Rounded.ArrowBackIos, t("lesson.back"), { model.pause() })
        Row(
            Modifier.weight(1f, fill = false).heightIn(min = 44.dp).unGlass(CircleShape).padding(start = 6.dp, end = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(Modifier.size(26.dp).background(c.orange, CircleShape), contentAlignment = Alignment.Center) {
                Text("${meta.number}", style = Un.rounded(14.sp), color = Color.White)
            }
            Text(meta.title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = c.text, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Spacer(Modifier.weight(0.001f))
        Row(Modifier.unGlass(CircleShape).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            GlassCircleButton(Icons.Rounded.Pause, t("lesson.pause"), { model.pause() }, glass = false)
            GlassCircleButton(Icons.Rounded.Replay, t("lesson.repeat"), { model.repeat() }, glass = false)
            GlassCircleButton(Icons.Rounded.SkipNext, t("lesson.skip"), { model.skip() }, glass = false)
            Box(
                Modifier.size(44.dp).clip(CircleShape)
                    .clickable { model.setLanguage(if (language == UnLanguage.RUSSIAN) UnLanguage.ENGLISH else UnLanguage.RUSSIAN) }
                    .semantics { contentDescription = t("lesson.language") },
                contentAlignment = Alignment.Center,
            ) { Text(if (language == UnLanguage.RUSSIAN) "RU" else "EN", fontSize = 13.sp, fontWeight = FontWeight.Bold, color = c.text) }
        }
    }
}

/**
 * One state at a time: orange speaking (ripples), blue listening (mic level and what was heard),
 * purple watching the table (a slowly turning dashed ring), green correct.
 */
@Composable
private fun TutorPanel(s: UnSnapshot?, state: LessonModel.TutorState, t: Copy, compact: Boolean) {
    val c = Un.c
    val label = when (state) {
        LessonModel.TutorState.SPEAKING -> t("lesson.speaking")
        LessonModel.TutorState.LISTENING -> t("lesson.listening")
        LessonModel.TutorState.WATCHING -> t("lesson.watching")
        LessonModel.TutorState.CELEBRATING -> t("lesson.celebrating")
        LessonModel.TutorState.MUTED -> t("lesson.muted")
    }
    Row(
        Modifier.fillMaxWidth().unGlass(RoundedCornerShape(22.dp), strong = true).padding(24.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(20.dp),
    ) {
        TutorOrb(state, s?.micLevel?.toDouble() ?: 0.0, if (compact) 64.dp else 84.dp)
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(label.uppercase(), fontSize = 13.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.6.sp, color = orbColor(state))
                if (state == LessonModel.TutorState.LISTENING) MicBars(s?.micLevel?.toDouble() ?: 0.0)
            }
            Text(s?.lastSpoken?.ifEmpty { "…" } ?: "…", fontSize = if (compact) 24.sp else 32.sp, fontWeight = FontWeight.Bold, color = c.text,
                lineHeight = if (compact) 29.sp else 38.sp)
            val heard = s?.heardText.orEmpty()
            if (state == LessonModel.TutorState.LISTENING && heard.isNotEmpty()) {
                Text(buildAnnotatedString {
                    withStyle(SpanStyle(color = c.text2)) { append("${t("lesson.heard")}: ") }
                    withStyle(SpanStyle(color = c.text, fontWeight = FontWeight.Bold)) { append("«$heard»") }
                }, fontSize = 15.sp, maxLines = 2)
            }
        }
    }
}

@Composable
private fun orbColor(state: LessonModel.TutorState): Color {
    val c = Un.c
    return when (state) {
        LessonModel.TutorState.SPEAKING -> c.orange
        LessonModel.TutorState.LISTENING -> c.blue
        LessonModel.TutorState.WATCHING -> c.purple
        LessonModel.TutorState.CELEBRATING -> c.green
        LessonModel.TutorState.MUTED -> Un.gray
    }
}

/** The tutor's orb: colour, icon and motion say the state, never colour alone. */
@Composable
private fun TutorOrb(state: LessonModel.TutorState, level: Double = 0.0, size: Dp = 84.dp) {
    val c = Un.c
    val still = reduceMotion()
    val time by rememberTime(paused = still)
    val color = orbColor(state)
    Box(Modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.matchParentSize()) {
            val r = this.size.minDimension / 2
            when (state) {
                LessonModel.TutorState.SPEAKING -> for (i in 0 until 2) {
                    val phase = ((time / 1.8 + i * 0.5) % 1.0).toFloat()
                    drawCircle(c.orange.copy(alpha = 0.65f * (1 - phase)), r * (1 + phase * 0.75f), style = Stroke(3.dp.toPx()))
                }
                LessonModel.TutorState.LISTENING -> {
                    // The ring follows the microphone (1.0 to 1.3); a breath when it reports nothing.
                    val breath = (if (level > 0.02) level else 0.35 + 0.35 * sin(time * 4)).toFloat()
                    drawCircle(c.blue.copy(alpha = 0.18f), r * (1.1f + 0.3f * breath))
                    drawCircle(c.blue.copy(alpha = 0.28f), r * (1.05f + 0.15f * breath))
                }
                LessonModel.TutorState.WATCHING -> rotate((time / 9 * 360).toFloat()) {
                    drawCircle(c.purple, r * 1.2f, style = Stroke(3.dp.toPx(),
                        pathEffect = PathEffect.dashPathEffect(floatArrayOf(8.dp.toPx(), 6.dp.toPx()))))
                }
                LessonModel.TutorState.CELEBRATING -> {
                    val phase = ((time / 1.2) % 1.0).toFloat()
                    drawCircle(c.green.copy(alpha = 0.35f * (1 - phase)), r * (0.95f + phase * 0.65f))
                }
                LessonModel.TutorState.MUTED -> {}
            }
            drawCircle(color, r)
            drawCircle(Color.Black.copy(alpha = 0.14f), r - 2.dp.toPx(), center.copy(y = center.y + 2.dp.toPx()), style = Stroke(4.dp.toPx()))
        }
        val icon = when (state) {
            LessonModel.TutorState.SPEAKING -> Icons.AutoMirrored.Rounded.VolumeUp
            LessonModel.TutorState.LISTENING -> Icons.Rounded.Mic
            LessonModel.TutorState.WATCHING -> Icons.Rounded.Visibility
            LessonModel.TutorState.CELEBRATING -> Icons.Rounded.Star
            LessonModel.TutorState.MUTED -> Icons.Rounded.MicOff
        }
        Icon(icon, null, tint = Color.White, modifier = Modifier.size(size * 0.45f))
    }
}

/** Five bars next to "Listening", moving with the microphone. */
@Composable
private fun MicBars(level: Double) {
    val c = Un.c
    val time by rememberTime()
    Row(Modifier.height(16.dp), horizontalArrangement = Arrangement.spacedBy(2.dp), verticalAlignment = Alignment.CenterVertically) {
        for (i in 0 until 5) {
            val wave = 0.5 + 0.5 * sin(time * 9 + i * 1.3)
            val height = 4 + 12 * (if (level > 0.02) level else 0.45) * wave
            Box(Modifier.width(3.dp).height(height.dp).background(c.blue, CircleShape))
        }
    }
}

/** The fallback when the microphone can't hear: numbers to tap (keys of 56dp). */
@Composable
private fun NumberPad(model: LessonModel, t: Copy) {
    val c = Un.c
    Column(
        Modifier.widthIn(max = 560.dp).fillMaxWidth().unGlass(RoundedCornerShape(22.dp), strong = true).padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Text(t("lesson.numpadHint"), fontSize = 13.sp, fontWeight = FontWeight.Medium, color = c.text2)
        for (row in (0..10).chunked(6)) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (n in row) {
                    Box(
                        Modifier.weight(1f).heightIn(min = 56.dp).clip(RoundedCornerShape(14.dp)).background(c.surface2)
                            .clickable { model.tap(n.toUInt()) },
                        contentAlignment = Alignment.Center,
                    ) { Text("$n", style = Un.rounded(28.sp), color = c.text) }
                }
                repeat(6 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

/** A correct count: the stamped ticket with the big green numeral and a burst of confetti. */
@Composable
private fun CountTicket(count: UInt) {
    var shown by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { shown = true }
    val appear by animateFloatAsState(if (shown) 1f else 0f, spring(dampingRatio = 0.6f, stiffness = Spring.StiffnessLow), label = "ticket")
    Box(Modifier.fillMaxSize().semantics { contentDescription = "$count" }, contentAlignment = Alignment.Center) {
        Confetti()
        Box(Modifier.scale(0.6f + 0.4f * appear).alpha(appear.coerceIn(0f, 1f))) {
            Column(
                Modifier.shadow(20.dp, RoundedCornerShape(22.dp)).background(Color.White, RoundedCornerShape(22.dp))
                    .padding(horizontal = 36.dp, vertical = 22.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text("$count", style = Un.rounded(132.sp), color = Un.badgeCorrect, lineHeight = 140.sp)
                DashedLine(Modifier.width(140.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (color in listOf(Color(0xFFD8402F), Color(0xFFF2BF2A), Color(0xFF3A6CD0))) {
                        Box(Modifier.size(22.dp, 16.dp).background(color, RoundedCornerShape(5.dp)))
                    }
                }
            }
            Box(
                Modifier.align(Alignment.TopEnd).padding(12.dp).size(40.dp).rotate(-12f).border(2.5.dp, Un.c.orange, CircleShape),
                contentAlignment = Alignment.Center,
            ) { Icon(Icons.Rounded.Check, null, tint = Un.c.orange, modifier = Modifier.size(22.dp)) }
        }
    }
}

@Composable
private fun DashedLine(modifier: Modifier) {
    Canvas(modifier.height(1.5.dp)) {
        drawLine(Color.Black.copy(alpha = 0.15f), Offset(0f, size.height / 2), Offset(size.width, size.height / 2), 1.5.dp.toPx(),
            pathEffect = PathEffect.dashPathEffect(floatArrayOf(5.dp.toPx(), 5.dp.toPx())))
    }
}

/** Sixteen pieces flying out over 2.4 s (none without animations). */
@Composable
private fun Confetti() {
    if (reduceMotion()) return
    val time by rememberTime()
    val colors = listOf(0xFFFFC93C, 0xFFE85F2A, 0xFF1F86E0, 0xFF1FA84A, 0xFF8B4DEB, 0xFFE03253).map { Color(it) }
    Canvas(Modifier.fillMaxSize()) {
        val p = (time / 2.4).coerceAtMost(1.0).toFloat()
        val opacity = if (p < 0.7f) 1f else (1 - p) / 0.3f
        for (i in 0 until 16) {
            val radians = (i * 22.5 + (i % 2) * 7) * Math.PI / 180
            val distance = 210.dp.toPx() * p
            val at = center + Offset((cos(radians) * distance).toFloat(), (sin(radians) * distance).toFloat())
            val h = (if (i % 3 == 0) 24 else 14).dp.toPx()
            val w = 8.dp.toPx()
            rotate(p * 160, at) {
                drawRoundRect(colors[i % colors.size].copy(alpha = opacity), Offset(at.x - w / 2, at.y - h / 2),
                    androidx.compose.ui.geometry.Size(w, h), androidx.compose.ui.geometry.CornerRadius(2.dp.toPx()))
            }
        }
    }
}

/**
 * The end of a completed lesson, in three beats: the stamped ticket with three stars,
 * "Продолжим?" with picture buttons that mirror what the tutor hears, and a calm goodbye.
 */
@Composable
private fun EndView(model: LessonModel, phase: LessonModel.EndPhase, meta: LessonMeta, t: Copy) {
    val c = Un.c
    val children by model.children.collectAsState()
    val childId by model.childId.collectAsState()
    val lessons by model.lessons.collectAsState()
    val child = children.firstOrNull { it.id == childId }
    val name = child?.name ?: ""
    Box(Modifier.fillMaxSize().background(c.surface).windowInsetsPadding(WindowInsets.safeDrawing).padding(24.dp), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(22.dp)) {
            when (phase) {
                LessonModel.EndPhase.CELEBRATE -> {
                    Box(contentAlignment = Alignment.Center) {
                        Confetti()
                        StarTicket(meta)
                    }
                    Text(t("end.hooray", "name" to name), style = Un.rounded(52.sp), color = c.text, textAlign = TextAlign.Center, lineHeight = 58.sp)
                    Text(t("end.done", "lesson" to meta.title), fontSize = 17.sp, color = c.text2, textAlign = TextAlign.Center)
                    TrailDots(lessons, child)
                }
                LessonModel.EndPhase.ASK -> {
                    TutorOrb(LessonModel.TutorState.LISTENING, size = 84.dp)
                    Text(t("end.ask"), style = Un.rounded(52.sp), color = c.text, textAlign = TextAlign.Center, lineHeight = 58.sp)
                    Row(Modifier.widthIn(max = 520.dp).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                        AnswerCard(t("end.yes"), Icons.Rounded.Check, c.green, Modifier.weight(1f)) { model.answerNext(true) }
                        AnswerCard(t("end.no"), Icons.Rounded.Bedtime, Un.gray, Modifier.weight(1f)) { model.answerNext(false) }
                    }
                }
                LessonModel.EndPhase.BYE -> {
                    Icon(Icons.Rounded.Bedtime, null, tint = c.purple, modifier = Modifier.size(72.dp))
                    Text(t("end.bye", "name" to name), style = Un.rounded(40.sp), color = c.text, textAlign = TextAlign.Center, lineHeight = 46.sp)
                    val next = child?.let { model.nextLesson(it) }
                    if (next != null) {
                        Text(t("end.nextTime", "lesson" to LessonMeta.of(next.id, lessons, t.ru).title), fontSize = 17.sp, color = c.text2,
                            textAlign = TextAlign.Center)
                    }
                    PrimaryButton(t("end.home"), Modifier.widthIn(max = 360.dp)) { model.goHome() }
                }
            }
        }
    }
}

@Composable
private fun AnswerCard(title: String, icon: androidx.compose.ui.graphics.vector.ImageVector, tint: Color, modifier: Modifier, onClick: () -> Unit) {
    Column(
        modifier.heightIn(min = 200.dp).clip(RoundedCornerShape(22.dp)).background(tint).clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
    ) {
        Icon(icon, null, tint = Color.White, modifier = Modifier.size(60.dp))
        Text(title, style = Un.rounded(30.sp), color = Color.White)
    }
}

/** The lesson-end ticket: three stars over the lesson's chip. */
@Composable
private fun StarTicket(meta: LessonMeta) {
    var shown by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { shown = true }
    val appear by animateFloatAsState(if (shown) 1f else 0f, spring(dampingRatio = 0.6f, stiffness = Spring.StiffnessLow), label = "stars")
    Column(
        Modifier.scale(0.6f + 0.4f * appear).alpha(appear.coerceIn(0f, 1f)).widthIn(min = 280.dp)
            .shadow(16.dp, RoundedCornerShape(22.dp)).background(Color.White, RoundedCornerShape(22.dp))
            .padding(horizontal = 32.dp, vertical = 22.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            for (i in 0 until 3) Icon(Icons.Rounded.Star, null, tint = Color(0xFFFFC93C), modifier = Modifier.size(if (i == 1) 52.dp else 42.dp))
        }
        DashedLine(Modifier.fillMaxWidth())
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(Modifier.size(24.dp).background(Un.c.orange, CircleShape), contentAlignment = Alignment.Center) {
                Text("${meta.number}", style = Un.rounded(13.sp), color = Color.White)
            }
            Text(meta.title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = Un.ink.copy(alpha = 0.7f))
        }
    }
}

/** The child's progress as ten dots, the completed ones green. */
@Composable
private fun TrailDots(lessons: List<UnStrategyInfo>, child: UnProfile?) {
    val c = Un.c
    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        for (lesson in lessons) {
            val done = child?.completed?.contains(lesson.id) == true
            Box(Modifier.size(if (done) 22.dp else 10.dp, 10.dp).background(if (done) c.green else c.surface3, CircleShape))
        }
    }
}
