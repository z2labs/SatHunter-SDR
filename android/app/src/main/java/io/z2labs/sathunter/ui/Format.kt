package io.z2labs.sathunter.ui

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.abs

/** 435.780 000 MHz style grouping. */
fun fmtFreq(hz: Double): String {
    val mhz = hz / 1e6
    val s = "%.6f".format(Locale.US, mhz)
    val dot = s.indexOf('.')
    return s.substring(0, dot + 4) + " " + s.substring(dot + 4)
}

fun fmtMhz(hz: Double, decimals: Int = 4) = "%.${decimals}f".format(Locale.US, hz / 1e6)

fun fmtDuration(s: Double): String {
    val t = maxOf(0.0, s).toLong()
    val h = t / 3600
    val m = (t % 3600) / 60
    val sec = t % 60
    return if (h > 0) "%dh%02d".format(h, m) else "%d:%02d".format(m, sec)
}

fun fmtClock(unix: Double): String = SimpleDateFormat("HH:mm", Locale.getDefault()).format(Date((unix * 1000).toLong()))

fun fmtHz(hz: Double): String = if (abs(hz) >= 1000) "%+.2f kHz".format(Locale.US, hz / 1000) else "%+.0f Hz".format(Locale.US, hz)
