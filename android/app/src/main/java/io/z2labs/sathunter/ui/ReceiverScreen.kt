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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt
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
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Text
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
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
    volume: Float,
    onVolume: (Float) -> Unit,
    fullscreen: Boolean,
    onFullscreen: (Boolean) -> Unit,
    onPlay: () -> Unit,
    onMute: () -> Unit,
    onTune: (Double) -> Unit,
    onCenter: (Double) -> Unit,
    onMode: (String) -> Unit,
    onCompass: () -> Unit,
) {
    var viewLo by remember { mutableFloatStateOf(0f) }
    var viewHi by remember { mutableFloatStateOf(1f) }
    var radarLarge by remember { mutableStateOf(false) }
    // Spectrum / waterfall split, dragged with the handle on the divider, kept between runs
    val prefs = LocalContext.current.getSharedPreferences("ui", android.content.Context.MODE_PRIVATE)
    var split by remember { mutableFloatStateOf(prefs.getFloat("split", 0.32f).coerceIn(0.12f, 0.75f)) }
    var areaH by remember { mutableIntStateOf(1) }
    var waterfall by remember { mutableStateOf<WaterfallView?>(null) }
    val stNow by rememberUpdatedState(st)
    // Shown centre frequency: while dragging it follows the finger at once (the hardware
    // catches up a moment later; the waterfall places each row by its own centre)
    var dragCenter by remember { mutableStateOf<Double?>(null) }
    var dragAt by remember { mutableLongStateOf(0L) }
    LaunchedEffect(dragAt) {
        delay(1500)
        dragCenter = null
    }
    val shownCenter = dragCenter ?: st.centerHz
    val shownCenterNow by rememberUpdatedState(shownCenter)
    val shownVfo = if (st.centerTuning) shownCenter else st.vfoHz
    LaunchedEffect(viewLo, viewHi, waterfall, shownCenter, st.specRate) {
        waterfall?.renderer?.let {
            it.viewLo = viewLo
            it.viewHi = viewHi
            it.dispCenter = shownCenter
            it.dispRate = st.specRate
        }
    }
    LaunchedEffect(split, waterfall) { waterfall?.renderer?.spectrumFraction = split }
    fun moveCenter(hz: Double) {
        dragCenter = hz
        dragAt = System.currentTimeMillis()
        onCenter(hz)
    }
    val tuneTo: (Double) -> Unit = { if (st.centerTuning) moveCenter(it) else onTune(it) }
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
        if (!fullscreen) TopBar(st, shownVfo, muted, onMute, volume, onVolume, onPlay, tuneTo) { onFullscreen(true) }

        Box(
            Modifier.weight(1f).fillMaxWidth().onSizeChanged { areaH = it.height.coerceAtLeast(1) }.pointerInput(Unit) {
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
                            val f = shownCenterNow + (u - 0.5) * s.specRate
                            if (s.centerTuning) moveCenter(f) else onTune(f)
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
                                // Drag sweeps the frequency axis (the VFO marker stays put on its
                                // frequency): first the zoomed view slides inside the captured
                                // span, past its edges the receiver's centre follows the finger.
                                val du = -pan.x / size.width * span
                                val lo = (viewLo + du).coerceIn(0f, 1f - span)
                                val rest = du - (lo - viewLo)
                                viewLo = lo
                                viewHi = lo + span
                                if (rest != 0f) moveCenter(shownCenterNow + rest.toDouble() * s.specRate)
                            }
                        }
                    }
            ) {
                val s = st
                val w = size.width
                val vfo = shownVfo
                val specH = size.height * split
                fun x(hz: Double): Float = ((((hz - shownCenter) / s.specRate + 0.5) - viewLo) / (viewHi - viewLo) * w).toFloat()
                val half = s.bw / 2
                val (a, b) = when (s.mode) {
                    "USB" -> Pair(vfo, vfo + s.bw)
                    "LSB" -> Pair(vfo - s.bw, vfo)
                    else -> Pair(vfo - half, vfo + half)
                }
                val xa = x(a)
                val xb = max(x(b), xa + 2f)
                drawRect(Color(0x33FFFFFF), Offset(xa, 0f), Size(xb - xa, specH))
                val xv = x(vfo)
                drawLine(Color(0xFFFF4A3A), Offset(xv, 0f), Offset(xv, size.height), 2.dp.toPx())
                // Frequency scale along the bottom of the spectrum
                val f0 = shownCenter + (viewLo - 0.5) * s.specRate
                val f1 = shownCenter + (viewHi - 0.5) * s.specRate
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
                // Divider between spectrum and waterfall
                drawRect(Color(0xFF0A0E14), Offset(0f, specH - 1.5.dp.toPx()), Size(w, 3.dp.toPx()))
                drawLine(Color(0xFF6B7F95), Offset(0f, specH), Offset(w, specH), 1.5.dp.toPx())
            }

            // Handle in the middle of the divider: drag up / down to share the height
            Box(
                Modifier
                    .align(Alignment.TopCenter)
                    .offset { IntOffset(0, (areaH * split).roundToInt() - 16.dp.roundToPx()) }
                    .size(88.dp, 32.dp)
                    .pointerInput(Unit) {
                        detectVerticalDragGestures(
                            onDragEnd = { prefs.edit().putFloat("split", split).apply() },
                        ) { c, dy ->
                            c.consume()
                            split = (split + dy / areaH).coerceIn(0.12f, 0.75f)
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                Box(
                    Modifier.size(52.dp, 16.dp).background(Color(0xFF2A3644), RoundedCornerShape(8.dp)),
                    contentAlignment = Alignment.Center,
                ) {
                    Column(verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        repeat(2) { Box(Modifier.size(26.dp, 2.dp).background(Color(0xFFB4C0CC), RoundedCornerShape(1.dp))) }
                    }
                }
            }

            if (fullscreen) {
                // Floating frequency, transport and modes over the waterfall
                Fade(controlsVisible, Modifier.align(Alignment.TopCenter)) {
                    Column(
                        Modifier.padding(top = 6.dp).width(320.dp).background(Glass, RoundedCornerShape(14.dp)).padding(horizontal = 8.dp, vertical = 4.dp),
                    ) {
                        FrequencyDisplay(shownVfo, tuneTo, Modifier.fillMaxWidth(), tubeHeight = 44.dp)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(Modifier.weight(1f)) { LevelLine(st, short = true) }
                            Volume(muted, onMute, volume, onVolume, Modifier.width(120.dp))
                            PlayButton(st, onPlay, Modifier.size(36.dp))
                        }
                    }
                }
                // Leaving full screen: its own big button in the corner, away from the others
                Fade(controlsVisible, Modifier.align(Alignment.TopEnd)) {
                    FilledIconButton(
                        onClick = { onFullscreen(false) },
                        modifier = Modifier.padding(8.dp).size(60.dp),
                        colors = IconButtonDefaults.filledIconButtonColors(containerColor = Glass),
                    ) { Icon(Icons.Filled.FullscreenExit, "Exit full screen", Modifier.size(36.dp)) }
                }
                Fade(controlsVisible, Modifier.align(Alignment.BottomCenter)) {
                    ModeRow(st, onMode, Modifier.padding(bottom = 8.dp).background(Glass, RoundedCornerShape(14.dp)))
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

/** Fade in / out (outside any layout scope, so the plain AnimatedVisibility is used). */
@Composable
private fun Fade(visible: Boolean, modifier: Modifier, content: @Composable () -> Unit) {
    AnimatedVisibility(visible = visible, modifier = modifier, enter = fadeIn(), exit = fadeOut()) { content() }
}

@Composable
private fun LevelLine(st: Status, short: Boolean = false) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text("${st.mode}  ${"%.1f".format(st.bw / 1000)} kHz", color = Dim, fontSize = 13.sp)
        Spacer(Modifier.width(8.dp))
        val lvl = ((st.levelDb + 100) / 80).toFloat().coerceIn(0f, 1f)
        LinearProgressIndicator(progress = { lvl }, modifier = Modifier.width(if (short) 50.dp else 80.dp).height(6.dp), color = Up, trackColor = Color(0xFF243040))
        if (st.timeShift != 0.0) {
            Spacer(Modifier.width(10.dp))
            Text("SIMULATED", color = SatYellow, fontSize = 12.sp)
        }
    }
}

@Composable
private fun PlayButton(st: Status, onPlay: () -> Unit, modifier: Modifier = Modifier) {
    FilledIconButton(onClick = onPlay, modifier = modifier, colors = IconButtonDefaults.filledIconButtonColors(containerColor = if (st.running) Down else Accent)) {
        Icon(if (st.running) Icons.Filled.Stop else Icons.Filled.PlayArrow, contentDescription = if (st.running) "Stop" else "Start")
    }
}

@Composable
private fun TopBar(
    st: Status, vfo: Double, muted: Boolean, onMute: () -> Unit, volume: Float, onVolume: (Float) -> Unit,
    onPlay: () -> Unit, onTune: (Double) -> Unit, onFullscreen: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().background(Panel).padding(start = 8.dp, end = 4.dp, top = 8.dp, bottom = 4.dp)) {
        // Nixie tubes over the full width
        FrequencyDisplay(vfo, onTune, Modifier.fillMaxWidth().padding(end = 4.dp), tubeHeight = 58.dp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f).padding(start = 4.dp)) {
                LevelLine(st)
                val src = if (st.sourceStatus.isNotEmpty()) "${st.sourceName} · ${st.sourceStatus}" else st.sourceName
                if (src.isNotEmpty()) Text(src, color = Dim, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Volume(muted, onMute, volume, onVolume, Modifier.width(130.dp))
            PlayButton(st, onPlay)
            IconButton(onClick = onFullscreen) { Icon(Icons.Filled.Fullscreen, contentDescription = "Full screen") }
        }
    }
}

/** Speaker button (mute) and a volume slider. */
@Composable
private fun Volume(muted: Boolean, onMute: () -> Unit, volume: Float, onVolume: (Float) -> Unit, modifier: Modifier) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = onMute, modifier = Modifier.size(36.dp)) {
            Icon(
                if (muted || volume == 0f) Icons.AutoMirrored.Filled.VolumeOff else Icons.AutoMirrored.Filled.VolumeUp,
                contentDescription = "Mute", tint = if (muted) Down else Color(0xFFE3EAF2),
            )
        }
        Slider(
            value = volume, onValueChange = onVolume, valueRange = 0f..2f, modifier = Modifier.weight(1f),
            colors = SliderDefaults.colors(thumbColor = Accent, activeTrackColor = Accent),
        )
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
