package app.onym.un

import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.sp
import uniffi.un_ffi.*

/**
 * What the tutor sees, over the camera picture (as on iOS): a box around every toy and its stable
 * number badge. Overlays use a white core with a dark halo, so they stay readable on wood, a
 * white table or a patterned cloth. A badge is white until counted, yellow (and larger) while the
 * tutor counts it, blue once counted, green when the count was right.
 */
@Composable
fun TableOverlay(table: UnTableState?, frameSize: Pair<Int, Int>, highlight: UInt?, correct: Boolean, copy: Copy, modifier: Modifier = Modifier) {
    val measurer = rememberTextMeasurer()
    val numbered = table?.detections?.filter { it.number > 0u }.orEmpty()
    val description = numbered.joinToString(". ") {
        copy("lesson.toy", "n" to "${it.number}", "state" to copy(stateKey(badgeState(it.number, highlight, correct))))
    }
    Canvas(modifier.semantics { if (description.isNotEmpty()) contentDescription = description }) {
        val (fw, fh) = frameSize
        if (table == null || fw <= 0 || fh <= 0) return@Canvas
        // The preview fills the view keeping the frame's aspect (FILL_CENTER), so the frame is
        // scaled by the larger ratio and centred, overflowing on one axis.
        val scale = maxOf(size.width / fw, size.height / fh)
        val drawn = Size(fw * scale, fh * scale)
        val origin = Offset((size.width - drawn.width) / 2, (size.height - drawn.height) / 2)
        val badge = (size.width * 0.095f).coerceIn(44 * density, 72 * density)
        for (d in table.detections) {
            val rect = Rect(origin.x + d.bbox.x1 * drawn.width, origin.y + d.bbox.y1 * drawn.height,
                origin.x + d.bbox.x2 * drawn.width, origin.y + d.bbox.y2 * drawn.height)
            val state = badgeState(d.number, highlight, correct)
            // The box: a white (or lit / correct) stroke with a dark halo inside and out.
            val stroke = when (state) { Badge.LIT -> Un.badgeLit; Badge.CORRECT -> Un.correctStroke; else -> Color.White }
            val width = (if (state == Badge.LIT || state == Badge.CORRECT) 5f else 3f) * density
            val radius = CornerRadius(minOf(22 * density, rect.width / 5))
            drawRoundRect(Un.halo, rect.topLeft, rect.size, radius, style = Stroke(width + 3 * density))
            drawRoundRect(stroke, rect.topLeft, rect.size, radius, style = Stroke(width))
            if (d.number == 0u) continue
            // The badge, centred on the box's corner.
            val center = rect.topLeft + Offset(6 * density, 6 * density)
            val grow = when (state) { Badge.LIT -> 1.25f; Badge.CORRECT -> 1.1f; else -> 1f }
            val r = badge * grow / 2
            val (bg, fg) = when (state) {
                Badge.IDLE -> Un.badgeIdle to Un.ink
                Badge.LIT -> Un.badgeLit to Color.Black
                Badge.COUNTED -> Un.badgeCounted to Color.White
                Badge.CORRECT -> Un.badgeCorrect to Color.White
            }
            when (state) {
                Badge.LIT -> drawCircle(Un.badgeLit.copy(alpha = 0.45f), r + 12 * density * grow, center)
                Badge.CORRECT -> drawCircle(Un.correctStroke.copy(alpha = 0.35f), r + 10 * density * grow, center)
                else -> {}
            }
            drawCircle(Color.Black.copy(alpha = 0.3f), r + 2 * density, center + Offset(0f, 4 * density))
            drawCircle(Un.halo, r + 2 * density, center)
            drawCircle(if (state == Badge.IDLE || state == Badge.COUNTED) Color.White else bg, r, center)
            drawCircle(bg, r - 4 * density * grow, center)
            val text = measurer.measure(d.number.toString(), Un.rounded((badge * grow * 0.56f / density / fontScale).sp).copy(color = fg))
            drawText(text, topLeft = center - Offset(text.size.width / 2f, text.size.height / 2f))
        }
    }
}

private enum class Badge { IDLE, LIT, COUNTED, CORRECT }

private fun badgeState(number: UInt, highlight: UInt?, correct: Boolean): Badge = when {
    correct -> Badge.CORRECT
    highlight == null || number == 0u -> Badge.IDLE
    number < highlight -> Badge.COUNTED
    number == highlight -> Badge.LIT
    else -> Badge.IDLE
}

private fun stateKey(state: Badge) = when (state) {
    Badge.IDLE -> "lesson.idle"
    Badge.LIT -> "lesson.lit"
    Badge.COUNTED -> "lesson.counted"
    Badge.CORRECT -> "lesson.done"
}
