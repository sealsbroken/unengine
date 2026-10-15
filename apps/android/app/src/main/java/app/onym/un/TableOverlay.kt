package app.onym.un

import android.graphics.Paint
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import uniffi.un_ffi.*

/**
 * What the tutor sees, drawn over the camera picture as on iOS and the desktop: a box around
 * every recognised object (amber while a hand is over the table, green once the table is
 * settled, white while it is still changing), the objects' stable numbers, lit red up to the
 * number the tutor is counting out, a blue ring around the object a finger is on, the fingers
 * the detector saw in blue (never counted), and the hand skeletons with the raised-finger count.
 */
@Composable
fun TableOverlay(table: UnTableState?, frameSize: Pair<Int, Int>, highlight: UInt?, modifier: Modifier = Modifier) {
    Canvas(modifier) {
        val (fw, fh) = frameSize
        if (table == null || fw <= 0 || fh <= 0) return@Canvas
        // The preview fills the view keeping the frame's aspect (FILL_CENTER), so the frame is
        // scaled by the larger ratio and centred, overflowing on one axis.
        val scale = maxOf(size.width / fw, size.height / fh)
        val drawn = Size(fw * scale, fh * scale)
        val origin = Offset((size.width - drawn.width) / 2, (size.height - drawn.height) / 2)
        fun point(x: Float, y: Float) = Offset(origin.x + x * drawn.width, origin.y + y * drawn.height)
        fun rect(b: UnBox): Rect = Rect(point(b.x1, b.y1), point(b.x2, b.y2))

        val color = if (table.handsPresent) AMBER else if (table.isStable) GREEN else Color.White
        for (d in table.detections) {
            val r = rect(d.bbox)
            drawRoundRect(color, r.topLeft, r.size, androidx.compose.ui.geometry.CornerRadius(3f), style = Stroke(2f * density))
            d.label?.let { text(it, r.left + 4 * density, r.top - 4 * density, 13f, color) }
            if (d.number > 0u) {
                if (table.pointedAt == d.number) {
                    // The object a finger is on.
                    drawCircle(BLUE, maxOf(r.width, r.height) * 0.7f, r.center, style = Stroke(3f * density))
                }
                val lit = highlight?.let { d.number <= it } ?: false
                val radius = (if (lit) 22f else 16f) * density
                drawCircle(if (lit) RED else Color.Black.copy(alpha = 0.6f), radius, r.center)
                text(d.number.toString(), r.center.x, r.center.y, if (lit) 26f else 18f, Color.White, centered = true, bold = true)
            }
        }

        for (d in table.fingers) {
            val r = rect(d.bbox)
            drawRoundRect(BLUE, r.topLeft, r.size, androidx.compose.ui.geometry.CornerRadius(3f), style = Stroke(2f * density))
            val label = "${d.label ?: "finger"} ${Math.round(d.confidence * 100)}%"
            text(label, r.left + 4 * density, r.top - 4 * density, 13f, BLUE)
        }

        for (hand in table.hands) {
            val joints = (0 until hand.joints.size - 1 step 2).map { i ->
                val x = hand.joints[i]; val y = hand.joints[i + 1]
                if (x.isNaN() || y.isNaN()) null else point(x, y)
            }
            for ((a, b) in BONES) {
                if (a < joints.size && b < joints.size) {
                    val pa = joints[a]; val pb = joints[b]
                    if (pa != null && pb != null) drawLine(BLUE, pa, pb, 2f * density)
                }
            }
            for (p in joints) if (p != null) drawCircle(Color.White, 3f * density, p)
            joints.firstOrNull()?.let { wrist ->
                val side = when (hand.chirality) { UnChirality.LEFT -> "Left"; UnChirality.RIGHT -> "Right"; else -> "Hand" }
                text("$side ${hand.raisedFingers}", wrist.x, wrist.y + 20 * density, 15f, BLUE, centered = true)
            }
        }
    }
}

private val AMBER = Color(240, 190, 60)
private val GREEN = Color(90, 200, 90)
private val RED = Color(230, 80, 80)
private val BLUE = Color(90, 160, 255)

/** MediaPipe's hand skeleton over the 21 joints in `UnHand.joints` order. */
private val BONES = listOf(
    0 to 1, 1 to 2, 2 to 3, 3 to 4,
    0 to 5, 5 to 6, 6 to 7, 7 to 8,
    5 to 9, 9 to 10, 10 to 11, 11 to 12,
    9 to 13, 13 to 14, 14 to 15, 15 to 16,
    13 to 17, 17 to 18, 18 to 19, 19 to 20,
    0 to 17,
)

/** Text in scale-independent points at (x, y), the baseline at y unless `centered`. */
private fun DrawScope.text(s: String, x: Float, y: Float, sp: Float, color: Color, centered: Boolean = false, bold: Boolean = false) {
    val paint = Paint().apply {
        this.color = color.toArgb()
        textSize = sp * density
        isAntiAlias = true
        isFakeBoldText = bold
        textAlign = if (centered) Paint.Align.CENTER else Paint.Align.LEFT
    }
    val baseline = if (centered) y - (paint.descent() + paint.ascent()) / 2 else y
    drawContext.canvas.nativeCanvas.drawText(s, x, baseline, paint)
}
