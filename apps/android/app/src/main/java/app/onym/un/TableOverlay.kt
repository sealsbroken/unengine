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
 * every recognised object (green once the table is settled, white while it is still changing)
 * and the objects' stable numbers, lit red up to the number the tutor is counting out.
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

        val color = if (table.isStable) GREEN else Color.White
        for (d in table.detections) {
            val r = rect(d.bbox)
            drawRoundRect(color, r.topLeft, r.size, androidx.compose.ui.geometry.CornerRadius(3f), style = Stroke(2f * density))
            d.label?.let { text(it, r.left + 4 * density, r.top - 4 * density, 13f, color) }
            if (d.number > 0u) {
                val lit = highlight?.let { d.number <= it } ?: false
                val radius = (if (lit) 22f else 16f) * density
                drawCircle(if (lit) RED else Color.Black.copy(alpha = 0.6f), radius, r.center)
                text(d.number.toString(), r.center.x, r.center.y, if (lit) 26f else 18f, Color.White, centered = true, bold = true)
            }
        }
    }
}

private val GREEN = Color(90, 200, 90)
private val RED = Color(230, 80, 80)

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
