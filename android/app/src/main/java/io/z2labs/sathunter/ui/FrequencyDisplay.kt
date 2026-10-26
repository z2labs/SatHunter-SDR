package io.z2labs.sathunter.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlin.math.roundToLong

private val POW10 = LongArray(10) { var v = 1L; repeat(it) { v *= 10 }; v }

private val Glow = Color(0xFFFF9A3C)
private val GlowHalo = Color(0xFFFF5A00)
private val Unlit = Color(0x24FFA060)
private val TubeTop = Color(0xFF22160E)
private val TubeBottom = Color(0xFF0C0806)
private val TubeRim = Color(0xFF4A3424)

/**
 * The frequency as a row of nixie tubes. Every tube is a digit: tap its upper half to go up,
 * the lower half to go down, or drag it up / down to spin it. Long-press any tube to type a
 * frequency in MHz (the keyboard comes up).
 *
 * The tubes share the given width; [tubeHeight] sets the size of the numerals.
 */
@Composable
fun FrequencyDisplay(hz: Double, onSet: (Double) -> Unit, modifier: Modifier = Modifier, tubeHeight: Dp = 60.dp) {
    var typing by remember { mutableStateOf(false) }
    var active by remember { mutableIntStateOf(-1) }
    val current by rememberUpdatedState(hz)
    val density = LocalDensity.current
    val stepPx = with(density) { 22.dp.toPx() }
    val value = hz.roundToLong().coerceIn(0, 9_999_999_999L)
    val text = "%010d".format(Locale.US, value)
    // GHz tube only above 1 GHz; leading zeros of the MHz part stay unlit
    val from = if (value >= 1_000_000_000L) 0 else 1
    val firstLit = text.indexOfFirst { it != '0' }.let { if (it < 0 || it > 3) 3 else it }
    val digitStyle = TextStyle(
        fontSize = with(density) { (tubeHeight * 0.74f).toSp() },
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Light,
    )

    Row(modifier.height(tubeHeight), horizontalArrangement = Arrangement.spacedBy(3.dp), verticalAlignment = Alignment.CenterVertically) {
        for (k in from until 10) {
            if (k == 4) DecimalPoint(tubeHeight)
            if (k == 7) Box(Modifier.width(4.dp))
            val weight = POW10[9 - k]
            Box(
                Modifier
                    .weight(1f)
                    .height(tubeHeight)
                    .background(Brush.verticalGradient(listOf(TubeTop, TubeBottom)), RoundedCornerShape(percent = 30))
                    .border(1.dp, if (active == k) Glow.copy(alpha = 0.7f) else TubeRim, RoundedCornerShape(percent = 30))
                    .pointerInput(k) {
                        detectTapGestures(
                            onPress = { active = k; tryAwaitRelease(); active = -1 },
                            onTap = { p -> onSet(current + if (p.y < size.height / 2f) weight else -weight) },
                            onLongPress = { typing = true },
                        )
                    }
                    .pointerInput(k) {
                        var acc = 0f
                        var base = 0.0
                        detectVerticalDragGestures(
                            onDragStart = { acc = 0f; base = current; active = k },
                            onDragEnd = { active = -1 },
                            onDragCancel = { active = -1 },
                        ) { change, dy ->
                            change.consume()
                            acc -= dy // up = higher
                            val steps = (acc / stepPx).toInt()
                            if (steps != 0) {
                                acc -= steps * stepPx
                                base += steps * weight
                                onSet(base)
                            }
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                TubeMesh()
                // The unlit cathodes behind, then the lit digit with its orange glow
                Text("8", style = digitStyle.copy(color = Unlit))
                if (k >= firstLit) {
                    Text(
                        text[k].toString(),
                        style = digitStyle.copy(color = Glow, shadow = Shadow(GlowHalo, Offset.Zero, blurRadius = 22f)),
                    )
                }
            }
        }
    }

    if (typing) {
        val start = "%.6f".format(Locale.US, hz / 1e6).trimEnd('0').trimEnd('.')
        var input by remember { mutableStateOf(TextFieldValue(start, TextRange(0, start.length))) }
        val focus = remember { FocusRequester() }
        fun accept() {
            val mhz = input.text.replace(',', '.').trim().toDoubleOrNull()
            if (mhz != null && mhz > 0) onSet(mhz * 1e6)
            typing = false
        }
        AlertDialog(
            onDismissRequest = { typing = false },
            title = { Text("Frequency (MHz)") },
            text = {
                OutlinedTextField(
                    value = input, onValueChange = { input = it }, singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { accept() }),
                    modifier = Modifier.focusRequester(focus).padding(top = 4.dp),
                )
                LaunchedEffect(Unit) { focus.requestFocus() } // the keyboard comes up at once
            },
            confirmButton = { TextButton(onClick = { accept() }) { Text("Tune") } },
            dismissButton = { TextButton(onClick = { typing = false }) { Text("Cancel") } },
        )
    }
}

/** The fine anode mesh in front of the digits. */
@Composable
private fun TubeMesh() {
    Canvas(Modifier.fillMaxSize().padding(3.dp)) {
        val step = 5.dp.toPx()
        val c = Color(0x10FFC090)
        var x = step / 2
        while (x < size.width) {
            drawLine(c, Offset(x, 0f), Offset(x, size.height), 1f)
            x += step
        }
        var y = step / 2
        while (y < size.height) {
            drawLine(c, Offset(0f, y), Offset(size.width, y), 1f)
            y += step
        }
    }
}

@Composable
private fun DecimalPoint(tubeHeight: Dp) {
    Canvas(Modifier.width(8.dp).height(tubeHeight)) {
        val r = 2.6.dp.toPx()
        val c = Offset(size.width / 2, size.height * 0.80f)
        drawCircle(GlowHalo.copy(alpha = 0.35f), r * 2.4f, c)
        drawCircle(Glow, r, c)
    }
}
