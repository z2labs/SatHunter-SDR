package io.z2labs.sathunter

/** JNI entry points of the Rust core (core/crates/sh-android). */
object Native {
    init {
        System.loadLibrary("sathunter")
    }

    /** Start the engine with its data folder (settings, satellite cache, logs). */
    @JvmStatic external fun init(dir: String)

    /** JSON command in, JSON reply out: {"ok": true, ...} or {"ok": false, "error": "..."}. */
    @JvmStatic external fun command(json: String): String

    /** New spectrum rows (dBFS) into [out]; returns (rows shl 16) or (bins / 16), 0 = nothing new. */
    @JvmStatic external fun spectrumRows(out: FloatArray): Int

    /** 48 kHz mono audio into [out]; returns the sample count. */
    @JvmStatic external fun audio(out: FloatArray): Int
}
