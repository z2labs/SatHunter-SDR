package io.z2labs.sathunter.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.z2labs.sathunter.BuildConfig
import io.z2labs.sathunter.Engine
import io.z2labs.sathunter.Status

@Composable
private fun Section(title: String) {
    Text(title, fontWeight = FontWeight.Bold, fontSize = 16.sp, color = Accent, modifier = Modifier.padding(top = 14.dp, bottom = 4.dp))
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(
    st: Status,
    engine: Engine,
    compassOn: Boolean,
    onCompass: (Boolean) -> Unit,
    gpsInfo: String,
    onFollowGps: (Boolean) -> Unit,
) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp)) {
        Section("Receiver")
        Text("Source", fontSize = 14.sp)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for ((id, name) in listOf("demo" to "Demo", "rtl" to "RTL-SDR", "esp" to "ESP-SDR (ESP32-S3)")) {
                FilterChip(
                    selected = st.source == id, onClick = { engine.source(id) }, enabled = id == "demo" || id in st.devices,
                    label = { Text(name) },
                )
            }
        }
        Text(
            if (st.sourceStatus.isNotEmpty()) st.sourceStatus else "Plug in an RTL-SDR or an ESP32-S3 (its native USB port) with an OTG cable.",
            color = Dim, fontSize = 12.sp,
        )
        Text("Sample rate", fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (r in st.rates.ifEmpty { listOf(2_400_000.0, 2_048_000.0, 1_024_000.0, 250_000.0) }) {
                FilterChip(selected = st.rate == r, onClick = { engine.rate(r) }, label = { Text(if (r >= 1e6) "%.3f MS/s".format(r / 1e6) else "%.1f kS/s".format(r / 1e3)) })
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
            Column(Modifier.weight(1f)) {
                Text("Center tuning", fontSize = 14.sp)
                Text("The VFO stays in the middle, tuning moves the whole spectrum", color = Dim, fontSize = 12.sp)
            }
            Switch(checked = st.centerTuning, onCheckedChange = { engine.centerTuning(it) })
        }
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
            Text("Automatic gain", Modifier.weight(1f), fontSize = 14.sp)
            Switch(checked = st.gain == null, onCheckedChange = { engine.gain(if (it) null else if (st.source == "esp") 60.0 else 25.4) })
        }
        if (st.gain != null) {
            var g by remember(st.gain) { mutableFloatStateOf(st.gain.toFloat()) }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("%.1f dB".format(g), Modifier.width(70.dp), fontSize = 14.sp)
                val gmax = (st.gains.maxOrNull() ?: 49.6).toFloat().coerceAtLeast(1f)
                Slider(value = g.coerceIn(0f, gmax), onValueChange = { g = it }, valueRange = 0f..gmax, onValueChangeFinished = { engine.gain(g.toDouble()) })
            }
        }
        Text("FFT size", fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (n in listOf(1024, 2048, 4096, 8192)) {
                FilterChip(selected = st.bins == n, onClick = { engine.fft(n) }, label = { Text("$n") })
            }
        }

        HorizontalDivider(Modifier.padding(top = 12.dp))
        Section("Ground station")
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Follow the phone's GPS", Modifier.weight(1f), fontSize = 14.sp)
            Switch(checked = st.followGps, onCheckedChange = onFollowGps)
        }
        if (st.followGps && gpsInfo.isNotEmpty()) Text(gpsInfo, color = Dim, fontSize = 12.sp)
        var lat by remember(st.lat) { mutableStateOf("%.5f".format(st.lat)) }
        var lon by remember(st.lon) { mutableStateOf("%.5f".format(st.lon)) }
        var alt by remember(st.altM) { mutableStateOf("%.0f".format(st.altM)) }
        var loc by remember(st.locator) { mutableStateOf(st.locator) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(lat, { lat = it }, label = { Text("Latitude") }, singleLine = true, modifier = Modifier.weight(1f),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal))
            OutlinedTextField(lon, { lon = it }, label = { Text("Longitude") }, singleLine = true, modifier = Modifier.weight(1f),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(alt, { alt = it }, label = { Text("Altitude m") }, singleLine = true, modifier = Modifier.weight(1f),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number))
            Button(onClick = {
                val la = lat.replace(',', '.').toDoubleOrNull()
                val lo = lon.replace(',', '.').toDoubleOrNull()
                if (la != null && lo != null) engine.station(la, lo, alt.toDoubleOrNull())
            }) { Text("Set") }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(loc, { loc = it }, label = { Text("Maidenhead locator") }, singleLine = true, modifier = Modifier.weight(1f))
            Button(onClick = { engine.locator(loc) }) { Text("Set") }
        }

        HorizontalDivider(Modifier.padding(top = 12.dp))
        Section("Radar")
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Compass (turn the radar with the phone, aim at the satellite)", Modifier.weight(1f), fontSize = 14.sp)
            Switch(checked = compassOn, onCheckedChange = onCompass)
        }

        HorizontalDivider(Modifier.padding(top = 12.dp))
        Section("About")
        Text("SatHunterSDR ${BuildConfig.VERSION_NAME} (${BuildConfig.GIT_REV})", fontSize = 14.sp)
        Text("Z2Labs · GPL-3.0-or-later · github.com/z2labs/SatHunter-SDR", color = Dim, fontSize = 12.sp)
        Text("Satellite data: SatNOGS DB (CC BY-SA 4.0), CelesTrak.", color = Dim, fontSize = 12.sp)
    }
}
