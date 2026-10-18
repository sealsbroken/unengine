package app.onym.un

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.*
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBackIos
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.*
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import uniffi.un_ffi.*

/**
 * A child's curriculum as a board-game trail (as on iOS): rows of two adventures that snake down
 * the page. Nodes: done (green tick), up next (big orange numeral, pulsing), paused here (orange
 * ring and a pause badge), not yet (quiet, still tappable). The next adventure is also the hero
 * card and the pinned "Continue: ..." button.
 */
@Composable
fun TrailScreen(model: LessonModel) {
    val c = Un.c
    val children by model.children.collectAsState()
    val childId by model.childId.collectAsState()
    val lessons by model.lessons.collectAsState()
    val language by model.language.collectAsState()
    val child = children.firstOrNull { it.id == childId } ?: return
    val ru = language == UnLanguage.RUSSIAN
    val t = Copy(ru)
    val next = model.nextLesson(child)
    val paused = next != null && child.paused == next.id
    val done = lessons.count { child.completed.contains(it.id) }
    BackHandler { model.chooseChild(null) }
    Column(Modifier.fillMaxSize().background(c.surface).windowInsetsPadding(WindowInsets.safeDrawing)) {
        // The header: back to the children, the child and their progress, the language.
        Box(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), contentAlignment = Alignment.Center) {
            Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(child.name, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = c.text)
                Text(t("cur.sub", "n" to "$done", "total" to "${lessons.size}"), fontSize = 12.5.sp, color = c.text2)
            }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                GlassCircleButton(Icons.AutoMirrored.Rounded.ArrowBackIos, t("cur.back"), { model.chooseChild(null) })
                Spacer(Modifier.weight(1f))
                LanguageSwitch(language) { model.setLanguage(it) }
            }
        }
        Box(Modifier.weight(1f)) {
            Column(
                Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = 8.dp, bottom = 96.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                CameraStatusCard(model, Modifier.padding(horizontal = 16.dp))
                if (next != null) Hero(model, next, paused, lessons, ru, t, Modifier.padding(horizontal = 16.dp))
                Trail(model, child, lessons, next?.id, ru, Modifier.padding(horizontal = 24.dp))
                Text(t("cur.hint"), fontSize = 12.5.sp, color = c.text2, textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 24.dp))
            }
            if (next != null) {
                PrimaryButton(
                    t("cur.cta", "lesson" to LessonMeta.of(next.id, lessons, ru).title),
                    Modifier.align(Alignment.BottomCenter).widthIn(max = 520.dp).padding(horizontal = 16.dp, vertical = 12.dp)
                        .shadow(12.dp, RoundedCornerShape(14.dp), ambientColor = c.blue, spotColor = c.blue),
                ) { model.start(next.id) }
            }
        }
    }
}

@Composable
private fun Hero(model: LessonModel, lesson: UnStrategyInfo, paused: Boolean, lessons: List<UnStrategyInfo>, ru: Boolean, t: Copy, modifier: Modifier) {
    val c = Un.c
    val meta = LessonMeta.of(lesson.id, lessons, ru)
    Row(
        modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(c.surface2).clickable { model.start(lesson.id) }.padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        LessonArt(meta.number, Modifier.size(64.dp))
        Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text((if (paused) t("cur.pausedLabel") else t("cur.nextLabel")).uppercase(), fontSize = 12.sp, fontWeight = FontWeight.SemiBold,
                letterSpacing = 0.4.sp, color = c.text2)
            Text(meta.title, fontSize = 22.sp, fontWeight = FontWeight.Bold, color = c.text)
            Text("${t("cur.lessonN", "n" to "${meta.number}")} · ${meta.skill}", fontSize = 15.sp, color = c.text2)
        }
    }
}

/** Lesson art is a placeholder until an illustrator draws it: the adventure's symbol on stripes. */
@Composable
fun LessonArt(number: Int, modifier: Modifier = Modifier) {
    val c = Un.c
    val symbols = listOf(
        Icons.Rounded.DirectionsBus, Icons.Rounded.ShoppingBasket, Icons.Rounded.Tram, Icons.Rounded.AddRoad, Icons.Rounded.Inventory2,
        Icons.Rounded.AutoAwesome, Icons.AutoMirrored.Rounded.Send, Icons.Rounded.Home, Icons.Rounded.DirectionsBoat, Icons.Rounded.LocalCafe,
    )
    Box(modifier.clip(RoundedCornerShape(12.dp)).background(c.surface3), contentAlignment = Alignment.Center) {
        Canvas(Modifier.matchParentSize()) {
            var x = -size.height
            val stroke = c.text3.copy(alpha = c.text3.alpha * 0.35f)
            while (x < size.width) {
                drawLine(stroke, Offset(x, size.height), Offset(x + size.height, 0f), strokeWidth = 5.dp.toPx())
                x += 12.dp.toPx()
            }
        }
        Icon(symbols[(number - 1).mod(symbols.size)], null, tint = c.orange, modifier = Modifier.size(30.dp))
    }
}

private enum class NodeState { DONE, NEXT, PAUSED, NOT_YET }

/** The ten nodes on a snaking path: row 0 left to right, row 1 right to left, and so on. */
@Composable
private fun Trail(model: LessonModel, child: UnProfile, lessons: List<UnStrategyInfo>, next: String?, ru: Boolean, modifier: Modifier) {
    val c = Un.c
    val rowHeight = 150.dp
    val rows = (lessons.size + 1) / 2
    BoxWithConstraints(modifier.fillMaxWidth().height(rowHeight * rows + 30.dp)) {
        val w = maxWidth
        val xs = listOf(w * 0.25f, w * 0.75f)
        fun point(i: Int): Pair<Dp, Dp> {
            val row = i / 2
            val col = if (row % 2 == 0) i % 2 else 1 - i % 2
            return xs[col] to rowHeight * row + 46.dp
        }
        // The path: straight within a row, a half-loop at the side between rows.
        Canvas(Modifier.matchParentSize()) {
            if (lessons.isEmpty()) return@Canvas
            fun px(p: Pair<Dp, Dp>) = Offset(p.first.toPx(), p.second.toPx())
            val path = Path()
            px(point(0)).let { path.moveTo(it.x, it.y) }
            for (i in 1 until lessons.size) {
                val a = px(point(i - 1)); val b = px(point(i))
                if (a.y == b.y) {
                    path.lineTo(b.x, b.y)
                } else {
                    val r = (b.y - a.y) / 2
                    val right = a.x > size.width / 2
                    path.arcTo(Rect(Offset(a.x, a.y + r), r), -90f, if (right) 180f else -180f, false)
                }
            }
            drawPath(path, c.surface3, style = Stroke(
                width = 8.dp.toPx(), cap = StrokeCap.Round,
                pathEffect = PathEffect.dashPathEffect(floatArrayOf(2.dp.toPx(), 12.dp.toPx())),
            ))
        }
        lessons.forEachIndexed { i, lesson ->
            val meta = LessonMeta.of(lesson.id, lessons, ru)
            val state = when {
                child.completed.contains(lesson.id) -> NodeState.DONE
                child.paused == lesson.id -> NodeState.PAUSED
                lesson.id == next -> NodeState.NEXT
                else -> NodeState.NOT_YET
            }
            val (x, y) = point(i)
            Box(Modifier.width(w / 2 - 8.dp).centerAt(x, y + 34.dp)) {
                TrailNode(meta, state, Copy(ru)) { model.start(lesson.id) }
            }
        }
    }
}

/** One adventure on the trail, with its 120dp hit area. */
@Composable
private fun TrailNode(meta: LessonMeta, state: NodeState, t: Copy, onClick: () -> Unit) {
    val c = Un.c
    val subtitle = when (state) {
        NodeState.DONE -> t("cur.done")
        NodeState.NEXT -> t("cur.next")
        NodeState.PAUSED -> t("cur.paused")
        NodeState.NOT_YET -> meta.skill
    }
    val subtitleColor = when (state) {
        NodeState.DONE -> c.green
        NodeState.NEXT, NodeState.PAUSED -> c.orange
        NodeState.NOT_YET -> c.text2
    }
    val pulsing = (state == NodeState.NEXT || state == NodeState.PAUSED) && !reduceMotion()
    Column(
        Modifier.fillMaxWidth().defaultMinSize(120.dp, 120.dp).clip(RoundedCornerShape(16.dp)).clickable(onClick = onClick)
            .semantics { contentDescription = "${t("cur.lessonN", "n" to "${meta.number}")}, ${meta.title}, $subtitle" },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.size(84.dp), contentAlignment = Alignment.Center) {
            if (pulsing) {
                val pulse by rememberInfiniteTransition(label = "pulse").animateFloat(
                    0f, 1f, infiniteRepeatable(tween(1800, easing = LinearOutSlowInEasing)), label = "pulse",
                )
                Box(Modifier.size(72.dp).scale(1f + 0.45f * pulse)
                    .border((10f - 9f * pulse).dp, c.orange.copy(alpha = 0.5f * (1 - pulse)), CircleShape))
            }
            when (state) {
                NodeState.DONE -> Box(Modifier.size(72.dp).background(c.green, CircleShape), contentAlignment = Alignment.Center) {
                    Icon(Icons.Rounded.Check, null, tint = Color.White, modifier = Modifier.size(34.dp))
                }
                NodeState.NEXT -> Box(Modifier.size(88.dp).background(c.orange, CircleShape), contentAlignment = Alignment.Center) {
                    Text("${meta.number}", style = Un.rounded(40.sp), color = Color.White)
                }
                NodeState.PAUSED -> Box(
                    Modifier.size(72.dp).background(c.surface2, CircleShape).border(4.dp, c.orange, CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Text("${meta.number}", style = Un.rounded(36.sp), color = c.orange) }
                NodeState.NOT_YET -> Box(
                    Modifier.size(66.dp).background(c.surface2, CircleShape).border(1.dp, c.hairline, CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Text("${meta.number}", style = Un.rounded(30.sp), color = c.text2) }
            }
            if (state == NodeState.PAUSED) {
                Box(
                    Modifier.offset(26.dp, 24.dp).size(24.dp).background(c.orange, CircleShape).border(2.dp, c.surface, CircleShape),
                    contentAlignment = Alignment.Center,
                ) { Icon(Icons.Rounded.Pause, null, tint = Color.White, modifier = Modifier.size(13.dp)) }
            }
        }
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(meta.title, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = c.text, textAlign = TextAlign.Center)
            Text(subtitle, fontSize = 12.5.sp, fontWeight = if (state == NodeState.NOT_YET) FontWeight.Normal else FontWeight.Medium,
                color = subtitleColor, textAlign = TextAlign.Center)
        }
    }
}
