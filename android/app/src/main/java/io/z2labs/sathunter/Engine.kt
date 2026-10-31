package io.z2labs.sathunter

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

data class Status(
    val running: Boolean = false,
    val source: String = "demo",
    val centerHz: Double = 100e6,
    val vfoHz: Double = 100e6,
    val rate: Double = 2.4e6,
    val mode: String = "WFM",
    val bw: Double = 180e3,
    val gain: Double? = null,
    val levelDb: Double = -120.0,
    val bins: Int = 2048,
    val specCenterHz: Double = 100e6,
    val specRate: Double = 2.4e6,
    val timeShift: Double = 0.0,
    val lat: Double = 47.4979,
    val lon: Double = 19.0402,
    val altM: Double = 120.0,
    val locator: String = "",
    val followGps: Boolean = true,
    val sourceName: String = "",
    val sourceStatus: String = "",
    val devices: List<String> = emptyList(),
    val rates: List<Double> = emptyList(),
    val gains: List<Double> = emptyList(),
    val centerTuning: Boolean = false,
    val biasTee: Boolean = false,
    val hasBiasTee: Boolean = false,
    val ppm: Double = 0.0,
    val decimation: Int = 1,
    /// Sample blocks the DSP could not keep up with (since the source started)
    val dropped: Long = 0,
)

data class PassInfo(
    val always: Boolean,
    val aos: Double,
    val los: Double,
    val tmax: Double,
    val maxEl: Double,
    val aosAz: Double,
    val losAz: Double,
)

data class SatRow(
    val norad: Long,
    val name: String,
    val hu: Boolean,
    val fav: Boolean,
    val pass: PassInfo,
    val freq: Double,
    val modes: String,
    val bands: String,
    /** SatNOGS: share of good observations of its best transmitter (%), and how many */
    val success: Double = 0.0,
    val good: Int = 0,
)

data class FilterState(
    val bands: Set<String> = setOf("TwoM", "SeventyCm", "SBand", "ThirteenCm"),
    val amateurOnly: Boolean = true,
    val activeOnly: Boolean = true,
    val favouritesOnly: Boolean = false,
    val minEl: Double = 5.0,
    val search: String = "",
    /** "pass" = next pass first, "health" = best heard on SatNOGS first */
    val sort: String = "pass",
) {
    fun toJson(): JSONObject = JSONObject()
        .put("bands", JSONArray(bands.toList()))
        .put("amateur_only", amateurOnly)
        .put("active_only", activeOnly)
        .put("favourites_only", favouritesOnly)
        .put("min_el", minEl)
        .put("search", search)
        .put("sort", sort)
}

data class BandInfo(val id: String, val name: String)

data class SatList(
    val now: Double = 0.0,
    val rows: List<SatRow> = emptyList(),
    val satellites: Int = 0,
    val transmitters: Int = 0,
    val status: String = "",
    val busy: Boolean = false,
    val progress: Pair<Int, Int>? = null,
    val ageHours: Double? = null,
    val filter: FilterState = FilterState(),
    val bands: List<BandInfo> = emptyList(),
)

data class TxInfo(
    val uuid: String,
    val description: String,
    val mode: String,
    val freq: Double,
    val low: Double,
    val high: Double,
    val baud: Double,
    val active: Boolean,
    val matches: Boolean,
)

data class TrackData(val t0: Double, val t1: Double, val step: Double, val tmax: Double, val az: FloatArray, val el: FloatArray)

data class OtherSat(val name: String, val az: Double, val el: Double)

data class SatDetail(
    val norad: Long,
    val name: String,
    val hungarian: Boolean,
    val favourite: Boolean,
    val tx: List<TxInfo>,
    val selectedTx: String?,
    val az: Double?,
    val el: Double?,
    val rangeKm: Double?,
    val pass: PassInfo?,
    val track: TrackData?,
    val others: List<OtherSat>,
    val tracking: Boolean,
    val userOffset: Double,
    val dopplerHz: Double,
    val rxHz: Double,
    val now: Double,
    val simulated: Boolean,
)

/** Talks to the Rust engine: commands, and polled state for the UI. */
class Engine(dataDir: String) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val _status = MutableStateFlow(Status())
    private val _list = MutableStateFlow(SatList())
    private val _detail = MutableStateFlow<SatDetail?>(null)
    private val _error = MutableStateFlow<String?>(null)
    val status: StateFlow<Status> = _status
    val satList: StateFlow<SatList> = _list
    val detail: StateFlow<SatDetail?> = _detail
    val error: StateFlow<String?> = _error

    @Volatile var wantList = false
    private var poller: Job? = null

    // Tuning while dragging: the newest request wins, sent one at a time
    private val tuneReq = MutableStateFlow<Double?>(null)
    @Volatile private var lastTune = 0.0
    @Volatile private var lastTuneAt = 0L
    // Moving the received span (dragging the spectrum): same scheme for the centre frequency
    private val centerReq = MutableStateFlow<Double?>(null)
    @Volatile private var lastCenter = 0.0
    @Volatile private var lastCenterAt = 0L

    init {
        Native.init(dataDir)
        scope.launch {
            centerReq.collect { hz ->
                if (hz != null) {
                    cmd("center", JSONObject().put("hz", hz))
                    refreshStatus()
                }
            }
        }
        scope.launch {
            tuneReq.collect { hz ->
                if (hz != null) {
                    cmd("tune", JSONObject().put("hz", hz))
                    refreshStatus()
                }
            }
        }
    }

    /** VFO including a tune request still on its way. */
    fun currentVfo(): Double =
        if (System.currentTimeMillis() - lastTuneAt < 1000) lastTune else _status.value.vfoHz

    /** Hardware centre including a request still on its way. */
    fun currentCenter(): Double =
        if (System.currentTimeMillis() - lastCenterAt < 1000) lastCenter else _status.value.centerHz

    fun centerSmooth(hz: Double) {
        lastCenter = hz
        lastCenterAt = System.currentTimeMillis()
        centerReq.value = hz
    }

    fun tuneSmooth(hz: Double) {
        lastTune = hz
        lastTuneAt = System.currentTimeMillis()
        tuneReq.value = hz
    }

    fun startPolling() {
        if (poller != null) return
        poller = scope.launch {
            var k = 0
            while (isActive) {
                refreshStatus()
                refreshDetail()
                if (wantList && k % 4 == 0) refreshList()
                k++
                delay(250)
            }
        }
    }

    fun stopPolling() {
        poller?.cancel()
        poller = null
    }

    /** Sends a command; a failure is shown to the user. */
    fun cmd(name: String, args: JSONObject = JSONObject()): JSONObject {
        args.put("cmd", name)
        val r = try {
            JSONObject(Native.command(args.toString()))
        } catch (e: Exception) {
            JSONObject().put("ok", false).put("error", e.message ?: "error")
        }
        if (!r.optBoolean("ok")) _error.value = r.optString("error")
        return r
    }

    fun clearError() {
        _error.value = null
    }

    fun async(name: String, args: JSONObject = JSONObject(), then: (() -> Unit)? = null) {
        scope.launch {
            cmd(name, args)
            refreshStatus()
            then?.invoke()
        }
    }

    fun start() = async("start")
    fun source(id: String) = async("source", JSONObject().put("id", id))
    fun usbAttach(fd: Int, vid: Int, pid: Int, product: String) =
        async("usb_attach", JSONObject().put("fd", fd).put("vid", vid).put("pid", pid).put("product", product))
    fun usbDetach(vid: Int, pid: Int) = async("usb_detach", JSONObject().put("vid", vid).put("pid", pid))
    fun stop() = async("stop")
    fun tune(hz: Double) = async("tune", JSONObject().put("hz", hz))
    fun mode(m: String) = async("mode", JSONObject().put("mode", m))
    fun bandwidth(hz: Double) = async("bandwidth", JSONObject().put("hz", hz))
    fun gain(db: Double?) = async("gain", JSONObject().put("db", db ?: JSONObject.NULL))
    fun rate(hz: Double) = async("rate", JSONObject().put("hz", hz))
    fun fft(size: Int) = async("fft", JSONObject().put("size", size))
    fun station(lat: Double, lon: Double, alt: Double?) =
        async("station", JSONObject().put("lat", lat).put("lon", lon).put("alt_m", alt ?: JSONObject.NULL))
    fun locator(loc: String) = async("locator", JSONObject().put("loc", loc))
    fun gps(lat: Double, lon: Double, alt: Double) = async("gps", JSONObject().put("lat", lat).put("lon", lon).put("alt_m", alt))
    fun followGps(on: Boolean) = async("follow_gps", JSONObject().put("on", on))
    fun centerTuning(on: Boolean) = async("center_tuning", JSONObject().put("on", on))
    fun biasTee(on: Boolean) = async("bias_tee", JSONObject().put("on", on))
    fun ppm(ppm: Double) = async("ppm", JSONObject().put("ppm", ppm))
    fun decimation(factor: Int) = async("decimation", JSONObject().put("factor", factor))
    fun filter(f: FilterState) = async("filter", JSONObject().put("filter", f.toJson())) { refreshList() }
    fun favourite(norad: Long, on: Boolean) = async("favourite", JSONObject().put("norad", norad).put("on", on)) { refreshList() }
    fun satUpdate() = async("sat_update")
    fun select(norad: Long, tx: String? = null) =
        async("sat_select", JSONObject().put("norad", norad).put("tx", tx ?: JSONObject.NULL)) { refreshDetail() }
    fun track(on: Boolean) = async("sat_track", JSONObject().put("on", on))
    fun resetOffset() = async("sat_reset_offset")
    fun simulate() = async("simulate")
    fun realTime() = async("real_time")

    private fun refreshStatus() {
        val j = try { JSONObject(Native.command("""{"cmd":"status"}""")) } catch (_: Exception) { return }
        if (!j.optBoolean("ok")) return
        val st = j.optJSONObject("station")
        val recent = System.currentTimeMillis() - lastTuneAt < 400
        val recentCenter = System.currentTimeMillis() - lastCenterAt < 400
        val centered = j.optBoolean("center_tuning")
        _status.value = Status(
            running = j.optBoolean("running"),
            source = j.optString("source"),
            centerHz = if (recentCenter) lastCenter else j.optDouble("center_hz"),
            vfoHz = if (recent) lastTune else if (recentCenter && centered) lastCenter else j.optDouble("vfo_hz"),
            centerTuning = centered,
            biasTee = j.optBoolean("bias_tee"),
            hasBiasTee = j.optBoolean("has_bias_tee"),
            ppm = j.optDouble("ppm", 0.0),
            decimation = j.optInt("decimation", 1),
            dropped = j.optLong("dropped", 0),
            rate = j.optDouble("rate"),
            mode = j.optString("mode"),
            bw = j.optDouble("bw"),
            gain = if (j.isNull("gain")) null else j.optDouble("gain"),
            levelDb = j.optDouble("level_db", -120.0),
            bins = j.optInt("bins", 2048),
            specCenterHz = j.optDouble("spectrum_center_hz").let { if (it == 0.0 || it.isNaN()) j.optDouble("center_hz") else it },
            specRate = j.optDouble("spectrum_rate").let { if (it == 0.0 || it.isNaN()) j.optDouble("rate") else it },
            timeShift = j.optDouble("time_shift", 0.0),
            lat = st?.optDouble("lat") ?: 47.4979,
            lon = st?.optDouble("lon") ?: 19.0402,
            altM = st?.optDouble("alt_m") ?: 120.0,
            locator = j.optString("locator"),
            followGps = j.optBoolean("follow_gps", true),
            sourceName = j.optString("source_name"),
            sourceStatus = j.optString("source_status"),
            devices = j.optJSONArray("devices")?.let { a -> (0 until a.length()).map { a.getString(it) } } ?: emptyList(),
            rates = j.optJSONArray("source_rates")?.let { a -> (0 until a.length()).map { a.getDouble(it) } } ?: emptyList(),
            gains = j.optJSONArray("source_gains")?.let { a -> (0 until a.length()).map { a.getDouble(it) } } ?: emptyList(),
        )
    }

    fun refreshList() {
        val j = try { JSONObject(Native.command("""{"cmd":"sat_list"}""")) } catch (_: Exception) { return }
        if (!j.optBoolean("ok")) return
        val rows = j.optJSONArray("rows") ?: JSONArray()
        val list = ArrayList<SatRow>(rows.length())
        for (k in 0 until rows.length()) {
            val r = rows.getJSONObject(k)
            list.add(
                SatRow(
                    norad = r.optLong("norad"),
                    name = r.optString("name"),
                    hu = r.optBoolean("hu"),
                    fav = r.optBoolean("fav"),
                    pass = pass(r.getJSONObject("pass")),
                    freq = r.optDouble("freq"),
                    modes = r.optString("modes"),
                    bands = r.optString("bands"),
                    success = r.optDouble("success", 0.0),
                    good = r.optInt("good", 0),
                )
            )
        }
        val f = j.optJSONObject("filter")
        val bandsArr = f?.optJSONArray("bands") ?: JSONArray()
        val filter = FilterState(
            bands = (0 until bandsArr.length()).map { bandsArr.getString(it) }.toSet(),
            amateurOnly = f?.optBoolean("amateur_only", true) ?: true,
            activeOnly = f?.optBoolean("active_only", true) ?: true,
            favouritesOnly = f?.optBoolean("favourites_only", false) ?: false,
            minEl = f?.optDouble("min_el", 5.0) ?: 5.0,
            search = f?.optString("search", "") ?: "",
            sort = f?.optString("sort", "pass") ?: "pass",
        )
        val bi = j.optJSONArray("bands") ?: JSONArray()
        val stats = j.optJSONObject("stats")
        val pr = j.optJSONArray("progress")
        _list.value = SatList(
            now = j.optDouble("now"),
            rows = list,
            satellites = stats?.optInt("satellites") ?: 0,
            transmitters = stats?.optInt("transmitters") ?: 0,
            status = j.optString("status"),
            busy = j.optBoolean("busy"),
            progress = if (pr != null && pr.length() == 2) Pair(pr.getInt(0), pr.getInt(1)) else null,
            ageHours = if (j.isNull("age_hours")) null else j.optDouble("age_hours"),
            filter = filter,
            bands = (0 until bi.length()).map { val b = bi.getJSONObject(it); BandInfo(b.optString("id"), b.optString("name")) },
        )
    }

    private fun refreshDetail() {
        val j = try { JSONObject(Native.command("""{"cmd":"sat_detail"}""")) } catch (_: Exception) { return }
        if (!j.optBoolean("ok") || j.isNull("selected")) {
            _detail.value = null
            return
        }
        val txa = j.optJSONArray("tx") ?: JSONArray()
        val txs = (0 until txa.length()).map {
            val t = txa.getJSONObject(it)
            TxInfo(
                t.optString("uuid"), t.optString("description"), t.optString("mode"), t.optDouble("freq"),
                t.optDouble("low"), t.optDouble("high"), t.optDouble("baud", 0.0), t.optBoolean("active"), t.optBoolean("matches"),
            )
        }
        val look = j.optJSONObject("look")
        val tr = j.optJSONObject("track")
        val track = tr?.let {
            val pts = it.optJSONArray("pts") ?: JSONArray()
            val az = FloatArray(pts.length())
            val el = FloatArray(pts.length())
            for (k in 0 until pts.length()) {
                val p = pts.getJSONArray(k)
                az[k] = p.getDouble(0).toFloat()
                el[k] = p.getDouble(1).toFloat()
            }
            TrackData(it.optDouble("t0"), it.optDouble("t1"), it.optDouble("step", 10.0), it.optDouble("tmax"), az, el)
        }
        val oa = j.optJSONArray("others") ?: JSONArray()
        _detail.value = SatDetail(
            norad = j.optLong("selected"),
            name = j.optString("name"),
            hungarian = j.optBoolean("hungarian"),
            favourite = j.optBoolean("favourite"),
            tx = txs,
            selectedTx = if (j.isNull("selected_tx")) null else j.optString("selected_tx"),
            az = look?.optDouble("az"),
            el = look?.optDouble("el"),
            rangeKm = look?.optDouble("range_km"),
            pass = j.optJSONObject("pass")?.let { pass(it) },
            track = track,
            others = (0 until oa.length()).map { val o = oa.getJSONObject(it); OtherSat(o.optString("name"), o.optDouble("az"), o.optDouble("el")) },
            tracking = j.optBoolean("tracking"),
            userOffset = j.optDouble("user_offset", 0.0),
            dopplerHz = j.optDouble("doppler_hz", 0.0),
            rxHz = j.optDouble("rx_hz", 0.0),
            now = j.optDouble("now"),
            simulated = j.optBoolean("simulated"),
        )
    }

    private fun pass(p: JSONObject) = PassInfo(
        always = p.optBoolean("always"),
        aos = p.optDouble("aos"),
        los = p.optDouble("los"),
        tmax = p.optDouble("tmax"),
        maxEl = p.optDouble("max_el"),
        aosAz = p.optDouble("aos_az"),
        losAz = p.optDouble("los_az"),
    )
}
