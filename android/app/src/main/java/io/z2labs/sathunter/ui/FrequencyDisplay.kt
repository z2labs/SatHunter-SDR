package io.z2labs.sathunter.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Keyboard
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import java.util.Locale
import kotlin.math.roundToLong

private val POW10 = LongArray(10) { var v = 1L; repeat(it) { v *= 10 }; v }

/**
 * Frequency as tuning digits: drag a digit up / down (or tap its upper / lower half) to change
 * it, long-press or the keyboard button to type a frequency in MHz.
 */
@Composable
fun FrequencyDisplay(hz: Double, fontSize: TextUnit, onSet: (Double) -> Unit, modifier: Modifier = Modifier, compact: Boolean = false) {
    var typing by remember { mutableStateOf(false) }
    var active by remember { mutableIntStateOf(-1) }
    val current by rememberUpdatedState(hz)
    val stepPx = with(LocalDensity.current) { 22.dp.toPx() }
    val value = hz.roundToLong().coerceIn(0, 9_999_999_999L)
    val text = "%010d".format(Locale.US, value)
    val firstSignificant = text.indexOfFirst { it != '0' }.let { if (it < 0 || it > 3) 3 else it }

    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        for (k in 0 until 10) {
            if (k == 4) Text(".", fontSize = fontSize, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.SemiBold)
            if (k == 7) Text(" ", fontSize = fontSize, fontFamily = FontFamily.Monospace)
            val weight = POW10[9 - k]
            val dim = k < firstSignificant
            Box(
                Modifier
                    .background(if (active == k) Color(0x334FB3FF) else Color.Transparent)
                    .pointerInput(k) {
                        detectTapGestures(
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
                    }
            ) {
                Text(
                    text[k].toString(), fontSize = fontSize, fontFamily = FontFamily.Monospace, fontWeight = FontWeight.SemiBold,
                    color = if (dim) Color(0xFF3A4654) else Color(0xFFE3EAF2),
                )
            }
        }
        IconButton(onClick = { typing = true }, modifier = if (compact) Modifier.size(34.dp) else Modifier) { Icon(Icons.Filled.Keyboard, contentDescription = "Type a frequency", tint = Dim) }
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
