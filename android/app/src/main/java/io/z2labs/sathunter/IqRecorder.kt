package io.z2labs.sathunter

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject

/**
 * IQ recording to a file the user can reach: Android 10+ straight into Downloads/SatHunter
 * (the engine writes through the descriptor of the new Downloads entry), older Androids into
 * the app's folder (Android/data/io.z2labs.sathunter/files/recordings).
 * The file is a 2-channel 16-bit WAV named like SDR++ recordings, with the receiver settings
 * and satellite inside, so it can be played back in SDR++ or sent in for analysis.
 */
class IqRecorder(private val ctx: Context, private val engine: Engine) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var uri: Uri? = null
    private val _saved = MutableStateFlow<String?>(null)
    /** Where the last recording went, for a short note on screen */
    val saved: StateFlow<String?> = _saved

    fun clearSaved() {
        _saved.value = null
    }

    fun toggle(recording: Boolean) {
        scope.launch { if (recording) stop() else start() }
    }

    private fun start() {
        val name = engine.cmd("record_name").optString("name").ifEmpty { return }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.MIME_TYPE, "audio/wav")
                put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/SatHunter")
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
            val r = ctx.contentResolver
            val u = r.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: return
            val fd = try {
                r.openFileDescriptor(u, "rw")?.detachFd()
            } catch (_: Exception) {
                null
            }
            if (fd == null) {
                r.delete(u, null, null)
                return
            }
            val res = engine.cmd("record_start", JSONObject().put("fd", fd).put("name", name))
            if (res.optBoolean("ok")) uri = u else r.delete(u, null, null)
        } else {
            val dir = ctx.getExternalFilesDir("recordings") ?: ctx.filesDir
            engine.cmd("record_start", JSONObject().put("dir", dir.absolutePath).put("name", name))
        }
        engine.refreshNow()
    }

    private fun stop() {
        val res = engine.cmd("record_stop")
        val name = res.optString("name")
        finishedElsewhere()
        if (name.isNotEmpty()) {
            _saved.value = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) "Saved to Downloads/SatHunter/$name"
            else "Saved to Android/data/io.z2labs.sathunter/files/recordings/$name"
        }
        engine.refreshNow()
    }

    /** The receiver stopped by itself (the engine closed the file): publish what was written. */
    @Synchronized
    fun finishedElsewhere() {
        uri?.let { u ->
            ctx.contentResolver.update(u, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            uri = null
        }
    }
}
