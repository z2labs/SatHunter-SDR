package io.z2labs.sathunter.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.StarOutline
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.z2labs.sathunter.Engine
import io.z2labs.sathunter.FilterState
import io.z2labs.sathunter.SatDetail
import io.z2labs.sathunter.SatList
import io.z2labs.sathunter.SatRow

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SatScreen(list: SatList, detail: SatDetail?, engine: Engine, radarOn: Boolean, onRadar: (Boolean) -> Unit) {
    var showFilter by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize()) {
        // Data status
        Row(Modifier.fillMaxWidth().background(Panel).padding(horizontal = 12.dp, vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                val age = list.ageHours
                Text(
                    when {
                        age == null -> "No satellite data yet"
                        age < 48 -> "${list.rows.size} passes · data ${age.toInt()} h old"
                        else -> "${list.rows.size} passes · data ${(age / 24).toInt()} days old"
                    },
                    fontWeight = FontWeight.SemiBold,
                )
                Text("${list.satellites} satellites, ${list.transmitters} downlinks", color = Dim, fontSize = 12.sp)
                if (list.status.isNotEmpty()) Text(list.status, color = SatYellow, fontSize = 12.sp)
            }
            TextButton(onClick = { showFilter = !showFilter }) { Text(if (showFilter) "Done" else "Filter") }
            IconButton(onClick = { engine.satUpdate() }, enabled = !list.busy) { Icon(Icons.Filled.Refresh, contentDescription = "Update data") }
        }
        list.progress?.let { (done, total) ->
            LinearProgressIndicator(progress = { done.toFloat() / maxOf(1, total) }, modifier = Modifier.fillMaxWidth())
        }
        if (list.busy) LinearProgressIndicator(modifier = Modifier.fillMaxWidth())

        if (showFilter) FilterPanel(list, engine)

        detail?.let { SelectedCard(it, engine, radarOn, onRadar) }

        LazyColumn(Modifier.weight(1f).fillMaxWidth()) {
            if (list.rows.isEmpty() && list.ageHours != null && list.progress == null) {
                item {
                    Text(
                        "No pass in the next 24 h for this filter. Try more bands, switch off \"Active only\" or lower the minimum elevation.",
                        Modifier.padding(16.dp), color = Dim,
                    )
                }
            }
            items(list.rows, key = { it.norad }) { r ->
                SatRowView(r, list.now, selected = detail?.norad == r.norad, onStar = { engine.favourite(r.norad, !r.fav) }) {
                    engine.select(r.norad)
                }
                HorizontalDivider(color = Color(0xFF1B2532))
            }
        }
    }
}

@Composable
private fun SatRowView(r: SatRow, now: Double, selected: Boolean, onStar: () -> Unit, onClick: () -> Unit) {
    val visible = r.pass.aos <= now
    Row(
        Modifier.fillMaxWidth().background(if (selected) Color(0xFF1B2A3A) else Color.Transparent).clickable(onClick = onClick)
            .horizontalScroll(rememberScrollState()).padding(end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = onStar) {
            Icon(if (r.fav) Icons.Filled.Star else Icons.Outlined.StarOutline, contentDescription = "Favourite", tint = if (r.fav) SatYellow else Dim)
        }
        Column(Modifier.width(170.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(r.name, color = if (visible) Up else MaterialColors.text, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (r.hu) {
                    Spacer(Modifier.width(6.dp))
                    Text("HU", fontSize = 10.sp, color = Color(0xFF0A0F16), modifier = Modifier.background(Color(0xFF5AE08C), RoundedCornerShape(4.dp)).padding(horizontal = 4.dp))
                }
            }
            Text(r.modes, color = Dim, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Column(Modifier.width(78.dp)) {
            Text(
                when {
                    r.pass.always -> "always"
                    visible -> "up ${fmtDuration(r.pass.los - now)}"
                    r.pass.aos - now < 3600 -> "in ${fmtDuration(r.pass.aos - now)}"
                    else -> fmtClock(r.pass.aos)
                },
                color = if (visible) Up else MaterialColors.text, fontFamily = FontFamily.Monospace, fontSize = 13.sp,
            )
            Text("max %.0f°".format(r.pass.maxEl), color = Dim, fontSize = 11.sp)
        }
        Text(if (r.freq > 0) fmtMhz(r.freq) else "", fontFamily = FontFamily.Monospace, fontSize = 13.sp, modifier = Modifier.width(92.dp))
        if (!r.pass.always) Text("%.0f° → %.0f°".format(r.pass.aosAz, r.pass.losAz), color = Dim, fontSize = 12.sp, modifier = Modifier.width(90.dp))
        Text(r.bands, color = Dim, fontSize = 12.sp, maxLines = 1)
    }
}

private object MaterialColors {
    val text = Color(0xFFE3EAF2)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FilterPanel(list: SatList, engine: Engine) {
    val f = list.filter
    var minEl by remember(f.minEl) { mutableFloatStateOf(f.minEl.toFloat()) }
    var search by remember(f.search) { mutableStateOf(f.search) }
    fun set(n: FilterState) = engine.filter(n)
    Column(Modifier.fillMaxWidth().background(Color(0xFF0F1620)).padding(horizontal = 12.dp, vertical = 6.dp)) {
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (b in list.bands) {
                FilterChip(
                    selected = b.id in f.bands,
                    onClick = { set(f.copy(bands = if (b.id in f.bands) f.bands - b.id else f.bands + b.id)) },
                    label = { Text(b.name) },
                )
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            FilterChip(selected = f.amateurOnly, onClick = { set(f.copy(amateurOnly = !f.amateurOnly)) }, label = { Text("Amateur only") })
            FilterChip(selected = f.activeOnly, onClick = { set(f.copy(activeOnly = !f.activeOnly)) }, label = { Text("Active only") })
            FilterChip(selected = f.favouritesOnly, onClick = { set(f.copy(favouritesOnly = !f.favouritesOnly)) }, label = { Text("Favourites only") })
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Min. elevation %.0f°".format(minEl), Modifier.width(140.dp), fontSize = 13.sp)
            Slider(
                value = minEl, onValueChange = { minEl = it }, valueRange = 0f..60f,
                onValueChangeFinished = { set(f.copy(minEl = minEl.toDouble())) }, modifier = Modifier.weight(1f),
            )
        }
        OutlinedTextField(
            value = search, onValueChange = { search = it; set(f.copy(search = it)) }, singleLine = true,
            label = { Text("Search: name, NORAD, mode") }, modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun SelectedCard(d: SatDetail, engine: Engine, radarOn: Boolean, onRadar: (Boolean) -> Unit) {
    var txMenu by remember { mutableStateOf(false) }
    val tx = d.tx.firstOrNull { it.uuid == d.selectedTx }
    Column(Modifier.fillMaxWidth().background(Color(0xFF16212D)).padding(horizontal = 12.dp, vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = { engine.favourite(d.norad, !d.favourite) }, modifier = Modifier.size(32.dp)) {
                Icon(if (d.favourite) Icons.Filled.Star else Icons.Outlined.StarOutline, contentDescription = "Favourite", tint = if (d.favourite) SatYellow else Dim)
            }
            Text(d.name, fontWeight = FontWeight.Bold, fontSize = 18.sp, modifier = Modifier.weight(1f))
            Text("NORAD ${d.norad}${if (d.hungarian) " · Hungarian" else ""}", color = Dim, fontSize = 12.sp)
        }
        // Downlink
        Row {
            OutlinedButton(onClick = { txMenu = true }, modifier = Modifier.fillMaxWidth()) {
                Text(
                    tx?.let { "${fmtMhz(it.freq)} ${it.mode}${if (it.baud > 0) " ${it.baud.toInt()} Bd" else ""} · ${it.description}" } ?: "Select a downlink",
                    maxLines = 1, overflow = TextOverflow.Ellipsis,
                )
            }
            DropdownMenu(expanded = txMenu, onDismissRequest = { txMenu = false }) {
                for (t in d.tx) {
                    DropdownMenuItem(
                        text = {
                            Text(
                                "${fmtMhz(t.freq)} ${t.mode}${if (t.high > t.low) " (linear)" else ""} · ${t.description}${if (!t.active) " [inactive]" else ""}",
                                color = if (t.matches) MaterialColors.text else Dim,
                            )
                        },
                        onClick = { txMenu = false; engine.select(d.norad, t.uuid) },
                    )
                }
            }
        }
        val now = d.now
        val el = d.el
        Text(
            if (el != null) "Az %.1f°  El %.1f°  %.0f km".format(d.az ?: 0.0, el, d.rangeKm ?: 0.0) else "",
            fontFamily = FontFamily.Monospace, fontSize = 13.sp,
        )
        d.pass?.let { p ->
            Text(
                if (el != null && el > 0) "Overhead now, LOS in ${fmtDuration(p.los - now)}"
                else "AOS ${fmtClock(p.aos)} (in ${fmtDuration(p.aos - now)}), max %.0f°, az %.0f° → %.0f°".format(p.maxEl, p.aosAz, p.losAz),
                fontSize = 13.sp,
            )
        }
        if (tx != null) {
            Text("Doppler ${fmtHz(d.dopplerHz)} → ${fmtMhz(d.rxHz, 5)} MHz${if (el != null && el <= 0) " (waiting on the AOS frequency)" else ""}", fontSize = 13.sp, color = Accent)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Switch(checked = d.tracking, onCheckedChange = { engine.track(it) })
            Text(" Doppler tracking", fontSize = 13.sp)
            Spacer(Modifier.width(12.dp))
            Switch(checked = radarOn, onCheckedChange = onRadar)
            Text(" Radar", fontSize = 13.sp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            if (d.simulated) {
                Text("Simulated time", color = SatYellow, fontSize = 13.sp)
                OutlinedButton(onClick = { engine.realTime() }) { Text("Real time") }
            } else if (d.pass != null && !d.pass.always && (el ?: 0.0) <= 0) {
                Button(onClick = { engine.simulate() }) { Text("Simulate this pass now") }
            }
            if (kotlin.math.abs(d.userOffset) >= 1) {
                OutlinedButton(onClick = { engine.resetOffset() }) { Text("Offset ${fmtHz(d.userOffset)} ✕") }
            }
        }
    }
}
