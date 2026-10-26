package io.z2labs.sathunter.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.VolumeOff
import androidx.compose.material.icons.automirrored.filled.VolumeUp
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material.icons.filled.FullscreenExit
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import io.z2labs.sathunter.Pose
import io.z2labs.sathunter.SatDetail
import io.z2labs.sathunter.Status
import io.z2labs.sathunter.WaterfallView
import kotlinx.coroutines.delay
import kotlin.math.floor
import kotlin.math.log10
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow

private val MODES = listOf("NFM", "WFM", "AM", "USB", "LSB", "CW")
private val Glass = Color(0xB30A0F16)

@Composable
fun ReceiverScreen(
    st: Status,
    detail: SatDetail?,
    pose: Pose,
    compassOn: Boolean,
    radarOn: Boolean,
    muted: Boolean,
    fullscreen: Boolean,
    onFullscreen: (Boolean) -> Unit,
    onPlay: () -> Unit,
    onMute: () -> Unit,
    onTune: (Double) -> Unit,
    onTuneBy: (Double) -> Unit,
    onMode: (String) -> Unit,
    onCompass: () -> Unit,
) {
    var viewLo by remember { mutableFloatStateOf(0f) }
    var viewHi by remember { mutableFloatStateOf(1f) }
    var radarLarge by remember { mutableStateOf(false) }
    var waterfall by remember { mutableStateOf<WaterfallView?>(null) }
    val stNow by rememberUpdatedState(st)
    LaunchedEffect(viewLo, viewHi, waterfall) {
        waterfall?.renderer?.let { it.viewLo = viewLo; it.viewHi = viewHi }
    }
    // Full screen: the floating controls fade out after a few seconds and come back on a touch
    var lastTouch by remember { mutableLongStateOf(System.currentTimeMillis()) }
    var controlsVisible by remember { mutableStateOf(true) }
    LaunchedEffect(lastTouch, fullscreen) {
        controlsVisible = true
        if (fullscreen) {
            delay(4000)
            controlsVisible = false
        }
    }

    Column(Modifier.fillMaxSize()) {
        if (!fullscreen) TopBar(st, muted, onMute, onPlay) { onFullscreen(true) }

        Box(
            Modifier.weight(1f).fillMaxWidth().pointerInput(Unit) {
                awaitPointerEventScope {
                    while (true) {
                        awaitPointerEvent(PointerEventPass.Initial)
                        lastTouch = System.currentTimeMillis()
                    }
                }
            }
        ) {
            AndroidView(factory = { ctx -> WaterfallView(ctx).also { waterfall = it } }, modifier = Modifier.fillMaxSize())
            val measurer = rememberTextMeasurer()
            Canvas(
                Modifier.fillMaxSize()
                    .pointerInput(Unit) {
                        detectTapGestures { p ->
                            val s = stNow
                            val u = viewLo + p.x / size.width * (viewHi - viewLo)
                            onTune(s.specCenterHz + (u - 0.5) * s.specRate)
                        }
                    }
                    .pointerInput(Unit) {
                        detectTransformGestures { centroid, pan, zoom, _ ->
                            val s = stNow
                            val span = viewHi - viewLo
                            if (zoom != 1f) {
                                // Zoom around the fingers, between the full span and 1/64 of it
                                val cu = viewLo + centroid.x / size.width * span
                                val ns = (span / zoom).coerceIn(1f / 64f, 1f)
                                val lo = (cu - (cu - viewLo) * ns / span).coerceIn(0f, 1f - ns)
                                viewLo = lo
                                viewHi = lo + ns
                            } else if (pan.x != 0f) {
                                // One finger: the frequency follows the finger like a dial
                                onTuneBy(pan.x / size.width * span * s.specRate)
                            }
                        }
                    }
            ) {
                val s = st
                val w = size.width
                val specH = size.height * 0.32f
                fun x(hz: Double): Float = ((((hz - s.specCenterHz) / s.specRate + 0.5) - viewLo) / (viewHi - viewLo) * w).toFloat()
                val half = s.bw / 2
                val (a, b) = when (s.mode) {
                    "USB" -> Pair(s.vfoHz, s.vfoHz + s.bw)
                    "LSB" -> Pair(s.vfoHz - s.bw, s.vfoHz)
                    else -> Pair(s.vfoHz - half, s.vfoHz + half)
                }
                val xa = x(a)
                val xb = max(x(b), xa + 2f)
                drawRect(Color(0x33FFFFFF), Offset(xa, 0f), Size(xb - xa, specH))
                val xv = x(s.vfoHz)
                drawLine(Color(0xFFFF4A3A), Offset(xv, 0f), Offset(xv, size.height), 2.dp.toPx())
                // Frequency scale along the bottom of the spectrum
                val f0 = s.specCenterHz + (viewLo - 0.5) * s.specRate
                val f1 = s.specCenterHz + (viewHi - 0.5) * s.specRate
                val spanHz = max(1.0, f1 - f0)
                val raw = spanHz / 5
                val mag = 10.0.pow(floor(log10(raw)))
                val step = listOf(1.0, 2.0, 5.0, 10.0).map { it * mag }.first { it >= raw }
                var f = (floor(f0 / step) + 1) * step
                val style = TextStyle(fontSize = 11.sp, color = Color(0xFFB4C0CC))
                while (f < f1) {
                    val px = x(f)
                    drawLine(Color(0x55FFFFFF), Offset(px, specH - 6.dp.toPx()), Offset(px, specH), 1f)
                    val m = measurer.measure(fmtMhz(f, if (step < 1e4) 4 else 3), style)
                    val tx = min(max(0f, px - m.size.width / 2f), w - m.size.width)
                    drawText(m, topLeft = Offset(tx, specH - 6.dp.toPx() - m.size.height))
                    f += step
                }
                drawLine(Color(0x40FFFFFF), Offset(0f, specH), Offset(w, specH), 1f)
            }

            if (fullscreen) {
                // Floating frequency, transport and modes over the waterfall
                AnimatedVisibility(controlsVisible, Modifier.align(Alignment.TopCenter), enter = fadeIn(), exit = fadeOut()) {
                    Row(
                        Modifier.padding(top = 10.dp).background(Glass, RoundedCornerShape(18.dp)).padding(horizontal = 14.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column {
                            Text(fmtFreq(st.vfoHz), fontSize = 24.sp, fontWeight = FontWeight.SemiBold, fontFamily = FontFamily.Monospace)
                            LevelLine(st)
                        }
                        Spacer(Modifier.width(6.dp))
                        IconButton(onClick = onMute) {
                            Icon(if (muted) Icons.AutoMirrored.Filled.VolumeOff else Icons.AutoMirrored.Filled.VolumeUp, "Mute")
                        }
                        PlayButton(st, onPlay)
                        IconButton(onClick = { onFullscreen(false) }) { Icon(Icons.Filled.FullscreenExit, "Exit full screen") }
                    }
                }
                AnimatedVisibility(controlsVisible, Modifier.align(Alignment.BottomCenter), enter = fadeIn(), exit = fadeOut()) {
                    ModeRow(st, onMode, Modifier.padding(bottom = 12.dp).background(Glass, RoundedCornerShape(18.dp)))
                }
            }

            if (radarOn && detail != null) {
                Radar(
                    d = detail, pose = pose, compassOn = compassOn, onCompass = onCompass,
                    sizeDp = if (radarLarge) 320.dp else 190.dp, onToggleSize = { radarLarge = !radarLarge },
                )
            }
        }

        if (!fullscreen) ModeRow(st, onMode, Modifier.fillMaxWidth().background(Panel))
    }
}

@Composable
private fun LevelLine(st: Status) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("${st.mode}  ${"%.1f".format(st.bw / 1000)} kHz", color = Dim, fontSize = 13.sp)
        Spacer(Modifier.width(10.dp))
        val lvl = ((st.levelDb + 100) / 80).toFloat().coerceIn(0f, 1f)
        LinearProgressIndicator(progress = { lvl }, modifier = Modifier.width(90.dp).height(6.dp), color = Up, trackColor = Color(0xFF243040))
        if (st.timeShift != 0.0) {
            Spacer(Modifier.width(10.dp))
            Text("SIMULATED", color = SatYellow, fontSize = 12.sp)
        }
    }
}

@Composable
private fun PlayButton(st: Status, onPlay: () -> Unit) {
    FilledIconButton(onClick = onPlay, colors = IconButtonDefaults.filledIconButtonColors(containerColor = if (st.running) Down else Accent)) {
        Icon(if (st.running) Icons.Filled.Stop else Icons.Filled.PlayArrow, contentDescription = if (st.running) "Stop" else "Start")
    }
}

@Composable
private fun TopBar(st: Status, muted: Boolean, onMute: () -> Unit, onPlay: () -> Unit, onFullscreen: () -> Unit) {
    Row(Modifier.fillMaxWidth().background(Panel).padding(start = 12.dp, end = 4.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(fmtFreq(st.vfoHz), fontSize = 30.sp, fontWeight = FontWeight.SemiBold, fontFamily = FontFamily.Monospace)
            LevelLine(st)
            val src = if (st.sourceStatus.isNotEmpty()) "${st.sourceName} · ${st.sourceStatus}" else st.sourceName
            if (src.isNotEmpty()) Text(src, color = Dim, fontSize = 11.sp, maxLines = 1)
        }
        IconButton(onClick = onMute) {
            Icon(if (muted) Icons.AutoMirrored.Filled.VolumeOff else Icons.AutoMirrored.Filled.VolumeUp, contentDescription = "Mute")
        }
        PlayButton(st, onPlay)
        IconButton(onClick = onFullscreen) { Icon(Icons.Filled.Fullscreen, contentDescription = "Full screen") }
    }
}

@Composable
private fun ModeRow(st: Status, onMode: (String) -> Unit, modifier: Modifier) {
    Row(
        modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 8.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        for (m in MODES) {
            FilterChip(
                selected = st.mode == m, onClick = { onMode(m) }, label = { Text(m) },
                colors = FilterChipDefaults.filterChipColors(selectedContainerColor = Accent.copy(alpha = 0.35f)),
            )
        }
    }
}
