package io.z2labs.sathunter

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import kotlin.concurrent.thread

/** Plays the engine's 48 kHz audio while the receiver runs. */
class AudioOut {
    @Volatile var muted = false
    /** Software gain, 0..2 (1 = as demodulated); the phone's volume keys work on top */
    @Volatile var volume = 1f
    @Volatile private var run = false
    private var worker: Thread? = null

    fun start() {
        if (run) return
        run = true
        worker = thread(name = "audio-out", priority = Thread.MAX_PRIORITY) {
            val min = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_FLOAT)
            val track = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .build()
                )
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setSampleRate(48000)
                        .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                        .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                        .build()
                )
                .setBufferSizeInBytes(maxOf(min * 4, 48000 / 5 * 4))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
            val buf = FloatArray(2400)
            try {
                track.play()
                while (run) {
                    val n = Native.audio(buf)
                    if (n <= 0) {
                        Thread.sleep(5)
                        continue
                    }
                    val g = if (muted) 0f else volume
                    if (g != 1f) for (i in 0 until n) buf[i] = (buf[i] * g).coerceIn(-1f, 1f)
                    track.write(buf, 0, n, AudioTrack.WRITE_BLOCKING)
                }
            } catch (_: Exception) {
            } finally {
                try { track.stop() } catch (_: Exception) {}
                track.release()
            }
        }
    }

    fun stop() {
        run = false
        worker?.join(500)
        worker = null
    }
}
