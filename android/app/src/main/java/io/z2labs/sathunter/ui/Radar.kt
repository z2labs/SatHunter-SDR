package io.z2labs.sathunter.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.z2labs.sathunter.Pose
import io.z2labs.sathunter.SatDetail
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.roundToInt
import kotlin.math.sin

private fun wrap180(a: Double): Double {
    var x = (a + 180.0) % 360.0
    if (x < 0) x += 360.0
    return x - 180.0
}

/**
 * Look4Sat style radar: the selected satellite's pass, the satellite, other satellites overhead.
 * With the compass on it turns with the phone (flat) or guides the aim (raised).
 * Drag to move, tap to switch size, tap the compass icon to switch the compass.
 */
@Composable
fun Radar(
    d: SatDetail,
    pose: Pose,
    compassOn: Boolean,
    onCompass: () -> Unit,
    sizeDp: Dp,
    onToggleSize: () -> Unit,
) {
    var off by remember { mutableStateOf(Offset.Zero) }
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val small = TextStyle(fontSize = 11.sp, color = Color(0xFFC8E6D2))
    val label = TextStyle(fontSize = 13.sp, color = Color(0xFFE3EAF2))
    Box(
        Modifier
            .offset { IntOffset(off.x.roundToInt(), off.y.roundToInt()) }
            .size(sizeDp, sizeDp + 40.dp)
            .background(Color(0xD90A0E14), RoundedCornerShape(12.dp))
            .pointerInput(Unit) { detectDragGestures { c, drag -> c.consume(); off += drag } }
            .pointerInput(compassOn) {
                detectTapGestures { p ->
                    val cb = with(density) { 36.dp.toPx() }
                    if (p.x > size.width - cb - 8 && p.y < cb + 8) onCompass() else onToggleSize()
                }
            }
    ) {
        Canvas(Modifier.size(sizeDp, sizeDp + 40.dp)) {
            val side = size.width
            val c = Offset(side / 2f, side / 2f)
            val r = side / 2f - 22.dp.toPx()
            val heading = if (compassOn && pose.valid) pose.heading else 0.0
            fun pt(az: Double, el: Double): Offset {
                val rr = r * (90.0 - max(el, 0.0)) / 90.0
                val a = Math.toRadians(az - heading)
                return Offset(c.x + (rr * sin(a)).toFloat(), c.y - (rr * cos(a)).toFloat())
            }
            val grid = Color(0xA05A8C78)
            for (e in listOf(0, 30, 60)) drawCircle(grid, r * (90 - e) / 90f, c, style = Stroke(1.dp.toPx()))
            for (a in 0 until 360 step 45) drawLine(Color(0x465A8C78), pt(a.toDouble(), 90.0), pt(a.toDouble(), 0.0), 1f)
            val card = listOf("N", "E", "S", "W")
            for (k in 0..3) {
                val p = pt(k * 90.0, -8.0)
                val q = Offset(c.x + (p.x - c.x) * (r + 12.dp.toPx()) / r, c.y + (p.y - c.y) * (r + 12.dp.toPx()) / r)
                val st = if (k == 0) label.copy(color = Color(0xFFFF6E5A)) else label
                val m = measurer.measure(card[k], st)
                drawText(m, topLeft = Offset(q.x - m.size.width / 2f, q.y - m.size.height / 2f))
            }
            val now = d.now
            // Pass track: flown part dimmed, a tick every minute, AOS / LOS times, max elevation
            d.track?.let { t ->
                val n = t.az.size
                for (i in 0 until n - 1) {
                    if (t.el[i] < 0 && t.el[i + 1] < 0) continue
                    val flown = t.t0 + (i + 1) * t.step <= now
                    drawLine(
                        if (flown) Color(0x8C5078AA) else Color(0xEB50AAFF),
                        pt(t.az[i].toDouble(), t.el[i].toDouble()), pt(t.az[i + 1].toDouble(), t.el[i + 1].toDouble()),
                        (if (flown) 2.dp else 3.dp).toPx(), cap = StrokeCap.Round,
                    )
                }
                val perMin = max(1, (60.0 / t.step).roundToInt())
                var i = perMin
                while (i < n - 1) {
                    if (t.el[i] >= 0) drawCircle(Color(0xC8C8E1FF), 2.2.dp.toPx(), pt(t.az[i].toDouble(), t.el[i].toDouble()))
                    i += perMin
                }
                if (n > 1) {
                    val a = pt(t.az[0].toDouble(), t.el[0].toDouble())
                    val b = pt(t.az[n - 1].toDouble(), t.el[n - 1].toDouble())
                    drawCircle(Color(0xFF50DC78), 4.dp.toPx(), a)
                    drawCircle(Color(0xFFF05A50), 4.dp.toPx(), b)
                    fun lbl(p: Offset, txt: String, col: Color) {
                        val m = measurer.measure(txt, small.copy(color = col))
                        drawText(m, topLeft = Offset((p.x - m.size.width / 2f).coerceIn(2f, side - m.size.width - 2f), (p.y + 5.dp.toPx()).coerceIn(2f, side - m.size.height - 2f)))
                    }
                    lbl(a, fmtClock(t.t0), Color(0xFF78E696))
                    if (t.t1 - t.t0 < 6 * 3600) lbl(b, fmtClock(t.t1), Color(0xFFFA8C82))
                    if (t.tmax > t.t0 && t.tmax < t.t1) {
                        val im = ((t.tmax - t.t0) / t.step).toInt().coerceIn(0, n - 1)
                        val m = pt(t.az[im].toDouble(), t.el[im].toDouble())
                        val txt = measurer.measure("%.0f°".format(d.pass?.maxEl ?: 0.0), small.copy(color = Color(0xDCAAC8E6)))
                        drawText(txt, topLeft = Offset(m.x + 6.dp.toPx(), m.y - txt.size.height / 2f))
                    }
                }
            }
            // Other satellites overhead
            for (o in d.others) {
                val p = pt(o.az, o.el)
                drawCircle(Color(0xC8AAB4BE), 3.dp.toPx(), p)
                val m = measurer.measure(o.name, small.copy(color = Color(0xB496A0AA)))
                drawText(m, topLeft = Offset(p.x + 4.dp.toPx(), p.y - m.size.height / 2f))
            }
            // Where the phone points, and the guidance
            var onTarget = false
            val el = d.el ?: -90.0
            val az = d.az ?: 0.0
            if (compassOn && pose.valid && !pose.flat) {
                val p = pt(pose.az, pose.el)
                val k = 8.dp.toPx()
                drawLine(Color.White, Offset(p.x - k, p.y), Offset(p.x + k, p.y), 1.5.dp.toPx())
                drawLine(Color.White, Offset(p.x, p.y - k), Offset(p.x, p.y + k), 1.5.dp.toPx())
                onTarget = abs(wrap180(az - pose.az)) * cos(Math.toRadians(max(0.0, el))) < 10 && abs(el - pose.el) < 10
            }
            val sp = pt(az, el)
            if (el > 0) {
                if (onTarget) drawCircle(Color(0xFF50FF78), 14.dp.toPx(), sp, style = Stroke(3.dp.toPx()))
                drawCircle(SatYellow, 7.dp.toPx(), sp)
            } else {
                drawCircle(Color(0xC8AAAAAA), 5.dp.toPx(), sp, style = Stroke(1.5.dp.toPx()))
            }
            // Compass icon (top right): needle; green = turning, orange = no data yet, grey = off
            compassIcon(Offset(side - 22.dp.toPx(), 22.dp.toPx()), 14.dp.toPx(), compassOn, pose)
            if (d.simulated) {
                drawText(measurer.measure("SIMULATED", small.copy(color = SatYellow)), topLeft = Offset(8.dp.toPx(), 6.dp.toPx()))
            }
            // Name and status line
            val line2 = when {
                compassOn && pose.valid && !pose.flat && el > 0 ->
                    if (onTarget) "ON TARGET  el %.0f°".format(el)
                    else {
                        val dAz = wrap180(az - pose.az)
                        val dEl = el - pose.el
                        "%s %.0f°  %s %.0f°".format(if (dAz >= 0) "right" else "left", abs(dAz), if (dEl >= 0) "up" else "down", abs(dEl))
                    }
                el > 0 -> "az %.0f° el %.0f°  LOS %s".format(az, el, fmtDuration((d.pass?.los ?: now) - now))
                d.pass != null -> "AOS in %s  max %.0f°".format(fmtDuration(d.pass.aos - now), d.pass.maxEl)
                else -> "no pass in 24 h"
            }
            drawText(measurer.measure(d.name, label), topLeft = Offset(10.dp.toPx(), side + 2.dp.toPx()))
            drawText(
                measurer.measure(line2, label.copy(color = if (onTarget) Color(0xFF50FF78) else Color(0xFFC8E6D2))),
                topLeft = Offset(10.dp.toPx(), side + 20.dp.toPx()),
            )
        }
    }
}

private fun DrawScope.compassIcon(cc: Offset, cr: Float, on: Boolean, pose: Pose) {
    val col = when {
        !on -> Color(0xDC8C8C8C)
        pose.valid -> Color(0xFF5AE678)
        else -> Color(0xFFFFB43C)
    }
    drawCircle(Color(0xE6141A22), cr, cc)
    drawCircle(col, cr, cc, style = Stroke(1.5.dp.toPx()))
    val na = Math.toRadians(if (on && pose.valid) -pose.heading else 0.0)
    val nd = Offset(sin(na).toFloat(), -cos(na).toFloat())
    val side = Offset(-nd.y * cr * 0.28f, nd.x * cr * 0.28f)
    val tip = cc + nd * (cr * 0.75f)
    val tail = cc - nd * (cr * 0.75f)
    drawPath(Path().apply { moveTo(tip.x, tip.y); lineTo(cc.x + side.x, cc.y + side.y); lineTo(cc.x - side.x, cc.y - side.y); close() }, Color(0xFFF05A50))
    drawPath(Path().apply { moveTo(tail.x, tail.y); lineTo(cc.x + side.x, cc.y + side.y); lineTo(cc.x - side.x, cc.y - side.y); close() }, col)
}
