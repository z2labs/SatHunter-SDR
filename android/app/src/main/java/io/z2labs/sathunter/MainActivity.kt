package io.z2labs.sathunter

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Radio
import androidx.compose.material.icons.filled.SatelliteAlt
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import io.z2labs.sathunter.ui.Bg
import io.z2labs.sathunter.ui.ReceiverScreen
import io.z2labs.sathunter.ui.SatHunterTheme
import io.z2labs.sathunter.ui.SatScreen
import io.z2labs.sathunter.ui.SettingsScreen

class MainActivity : ComponentActivity() {
    private lateinit var engine: Engine
    private val audio = AudioOut()
    private lateinit var compass: Compass
    private lateinit var gps: Gps
    private val prefs by lazy { getSharedPreferences("ui", MODE_PRIVATE) }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        engine = Engine(filesDir.absolutePath)
        compass = Compass(this)
        gps = Gps(this) { l ->
            engine.gps(l.latitude, l.longitude, if (l.hasAltitude()) l.altitude else 0.0)
            compass.setPosition(l.latitude, l.longitude, l.altitude)
        }
        setContent { SatHunterTheme { App() } }
    }

    override fun onResume() {
        super.onResume()
        engine.startPolling()
        if (prefs.getBoolean("compass", true)) compass.start()
        if (engine.status.value.followGps && hasLocation()) gps.start()
    }

    override fun onPause() {
        engine.stopPolling()
        compass.stop()
        gps.stop()
        super.onPause()
    }

    override fun onDestroy() {
        audio.stop()
        super.onDestroy()
    }

    private fun hasLocation() =
        ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

    @Composable
    private fun App() {
        val st by engine.status.collectAsState()
        val list by engine.satList.collectAsState()
        val detail by engine.detail.collectAsState()
        val error by engine.error.collectAsState()
        val pose by compass.pose.collectAsState()
        val gpsInfo by gps.info.collectAsState()
        var tab by remember { mutableIntStateOf(0) }
        var muted by remember { mutableStateOf(false) }
        var compassOn by remember { mutableStateOf(prefs.getBoolean("compass", true)) }
        var radarOn by remember { mutableStateOf(prefs.getBoolean("radar", true)) }
        val askLocation = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { r ->
            if (r.values.any { it }) gps.start()
        }

        // First start: play right away (the demo source when no SDR is plugged in)
        LaunchedEffect(Unit) {
            if (prefs.getBoolean("autostart", true)) engine.start()
            if (st.followGps && !hasLocation()) {
                askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
            }
        }
        LaunchedEffect(st.running) { if (st.running) audio.start() else audio.stop() }
        LaunchedEffect(tab) { engine.wantList = tab == 1; if (tab == 1) engine.refreshList() }
        LaunchedEffect(muted) { audio.muted = muted }

        fun setCompass(on: Boolean) {
            compassOn = on
            prefs.edit().putBoolean("compass", on).apply()
            if (on) compass.start() else compass.stop()
        }

        Scaffold(
            containerColor = Bg,
            bottomBar = {
                NavigationBar {
                    NavigationBarItem(selected = tab == 0, onClick = { tab = 0 }, icon = { Icon(Icons.Filled.Radio, null) }, label = { Text("Receiver") })
                    NavigationBarItem(selected = tab == 1, onClick = { tab = 1 }, icon = { Icon(Icons.Filled.SatelliteAlt, null) }, label = { Text("Satellites") })
                    NavigationBarItem(selected = tab == 2, onClick = { tab = 2 }, icon = { Icon(Icons.Filled.Settings, null) }, label = { Text("Settings") })
                }
            },
        ) { pad ->
            Box(Modifier.fillMaxSize().padding(pad)) {
                when (tab) {
                    0 -> ReceiverScreen(
                        st = st, detail = detail, pose = pose, compassOn = compassOn, radarOn = radarOn, muted = muted,
                        onPlay = { if (st.running) engine.stop() else engine.start() },
                        onMute = { muted = !muted },
                        onTune = { engine.tuneSmooth(it) },
                        onTuneBy = { engine.tuneSmooth(engine.currentVfo() + it) },
                        onMode = { engine.mode(it) },
                        onCompass = { setCompass(!compassOn) },
                    )
                    1 -> SatScreen(list, detail, engine, radarOn) {
                        radarOn = it
                        prefs.edit().putBoolean("radar", it).apply()
                    }
                    else -> SettingsScreen(
                        st = st, engine = engine, compassOn = compassOn, onCompass = { setCompass(it) }, gpsInfo = gpsInfo,
                        onFollowGps = { on ->
                            engine.followGps(on)
                            if (on) {
                                if (hasLocation()) gps.start()
                                else askLocation.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
                            } else gps.stop()
                        },
                    )
                }
            }
        }
        error?.let { e ->
            AlertDialog(
                onDismissRequest = { engine.clearError() },
                confirmButton = { TextButton(onClick = { engine.clearError() }) { Text("OK") } },
                text = { Text(e) },
            )
        }
    }
}
