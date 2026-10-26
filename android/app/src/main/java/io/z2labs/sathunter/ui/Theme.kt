package io.z2labs.sathunter.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

val Bg = Color(0xFF0A0F16)
val Panel = Color(0xFF121A24)
val Accent = Color(0xFF4FB3FF)
val SatYellow = Color(0xFFFFC83C)
val Up = Color(0xFF5AE08C)
val Down = Color(0xFFF0645A)
val Dim = Color(0xFF8A96A3)

private val scheme = darkColorScheme(
    primary = Accent,
    onPrimary = Color(0xFF02121F),
    secondary = SatYellow,
    onSecondary = Color(0xFF1E1500),
    tertiary = Up,
    background = Bg,
    surface = Panel,
    surfaceVariant = Color(0xFF1B2532),
    onSurface = Color(0xFFE3EAF2),
    onSurfaceVariant = Color(0xFFB4C0CC),
    error = Down,
)

@Composable
fun SatHunterTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = scheme, content = content)
}
