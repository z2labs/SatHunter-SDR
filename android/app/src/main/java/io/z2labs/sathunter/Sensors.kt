package io.z2labs.sathunter

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.hardware.GeomagneticField
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Build
import android.os.Bundle
import android.view.Surface
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlin.math.asin
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin

/**
 * Where the phone points.
 * flat = screen up: [heading] is the azimuth at the top of the screen (radar turns with the phone).
 * Raised: [az] / [el] is where the back of the phone points (aim mode).
 */
data class Pose(val valid: Boolean = false, val flat: Boolean = true, val heading: Double = 0.0, val az: Double = 0.0, val el: Double = 0.0)

class Compass(private val activity: Activity) : SensorEventListener {
    private val sm = activity.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val _pose = MutableStateFlow(Pose())
    val pose: StateFlow<Pose> = _pose
    @Volatile var declination = 5.5 // Central Europe until a position is known
    private var on = false
    private var smX = 0.0
    private var smY = 1.0
    private var init = false
    val available: Boolean get() = sm.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR) != null

    fun start() {
        if (on) return
        val s = sm.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR) ?: return
        on = sm.registerListener(this, s, SensorManager.SENSOR_DELAY_GAME)
        init = false
    }

    fun stop() {
        if (!on) return
        sm.unregisterListener(this)
        on = false
        _pose.value = Pose()
    }

    fun setPosition(lat: Double, lon: Double, alt: Double) {
        declination = GeomagneticField(lat.toFloat(), lon.toFloat(), alt.toFloat(), System.currentTimeMillis()).declination.toDouble()
    }

    @Suppress("DEPRECATION")
    private fun rotation(): Int =
        if (Build.VERSION.SDK_INT >= 30) activity.display?.rotation ?: Surface.ROTATION_0
        else activity.windowManager.defaultDisplay.rotation

    override fun onSensorChanged(e: SensorEvent) {
        val r = FloatArray(9)
        SensorManager.getRotationMatrixFromVector(r, e.values)
        // Device axes in world (east, north, up) coordinates: columns of R
        val xd = doubleArrayOf(r[0].toDouble(), r[3].toDouble(), r[6].toDouble())
        val yd = doubleArrayOf(r[1].toDouble(), r[4].toDouble(), r[7].toDouble())
        val zd = doubleArrayOf(r[2].toDouble(), r[5].toDouble(), r[8].toDouble())
        val back = doubleArrayOf(-zd[0], -zd[1], -zd[2])
        val pel = Math.toDegrees(asin(back[2].coerceIn(-1.0, 1.0)))
        val rot = rotation()
        val up = DoubleArray(3) {
            when (rot) {
                Surface.ROTATION_90 -> xd[it]
                Surface.ROTATION_180 -> -yd[it]
                Surface.ROTATION_270 -> -xd[it]
                else -> yd[it]
            }
        }
        val flat = pel < -50.0
        var az = 0.0
        val target = if (flat) {
            Math.toDegrees(atan2(up[0], up[1])) + declination
        } else {
            az = (Math.toDegrees(atan2(back[0], back[1])) + declination + 720.0) % 360.0
            az
        }
        // Smooth as a vector (no jump at north)
        val hx = sin(Math.toRadians(target))
        val hy = cos(Math.toRadians(target))
        if (!init) {
            smX = hx; smY = hy; init = true
        }
        smX += 0.25 * (hx - smX)
        smY += 0.25 * (hy - smY)
        val heading = (Math.toDegrees(atan2(smX, smY)) + 360.0) % 360.0
        _pose.value = Pose(true, flat, heading, az, pel)
    }

    override fun onAccuracyChanged(s: Sensor?, a: Int) {}
}

class Gps(context: Context, private val onFix: (Location) -> Unit) : LocationListener {
    private val lm = context.getSystemService(Context.LOCATION_SERVICE) as LocationManager
    private var on = false
    private val _info = MutableStateFlow("")
    val info: StateFlow<String> = _info

    @SuppressLint("MissingPermission")
    fun start() {
        if (on) return
        for (p in listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER)) {
            try {
                if (!lm.isProviderEnabled(p)) continue
                lm.getLastKnownLocation(p)?.let { onLocationChanged(it) }
                lm.requestLocationUpdates(p, 5000L, 20f, this)
                on = true
            } catch (_: SecurityException) {
                _info.value = "no location permission"
            } catch (_: Exception) {
            }
        }
        if (!on && _info.value.isEmpty()) _info.value = "location is off"
    }

    fun stop() {
        if (!on) return
        lm.removeUpdates(this)
        on = false
    }

    override fun onLocationChanged(l: Location) {
        _info.value = "GPS ±%.0f m".format(l.accuracy)
        onFix(l)
    }

    @Deprecated("Deprecated in Java")
    override fun onStatusChanged(provider: String?, status: Int, extras: Bundle?) {}
    override fun onProviderEnabled(provider: String) {}
    override fun onProviderDisabled(provider: String) {}
}
