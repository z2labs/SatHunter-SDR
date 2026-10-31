package io.z2labs.sathunter

import android.content.Context
import android.opengl.GLES30
import android.opengl.GLSurfaceView
import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import kotlin.math.max
import kotlin.math.min

/**
 * Spectrum + waterfall drawn by the GPU (OpenGL ES 3). New FFT rows go into a ring texture
 * (one glTexSubImage2D per row), the colour map is a 256x1 texture looked up in the shader.
 * The view window (zoom) is [viewLo, viewHi] in 0..1 of the span around [dispCenter].
 * Every row remembers the centre frequency it was taken at, and the shader places it on the
 * frequency axis accordingly: when the receiver is retuned (drag) the history slides with it
 * instead of jumping, and the new rows fill in from the side.
 */
class WaterfallView(context: Context) : GLSurfaceView(context) {
    val renderer = WaterfallRenderer()

    init {
        setEGLContextClientVersion(3)
        setRenderer(renderer)
        renderMode = RENDERMODE_CONTINUOUSLY
    }
}

class WaterfallRenderer : GLSurfaceView.Renderer {
    @Volatile var viewLo = 0f
    @Volatile var viewHi = 1f
    @Volatile var spectrumFraction = 0.32f
    /** Frequency axis shown: centre and width (Hz). NaN = the newest row's own centre. */
    @Volatile var dispCenter = Double.NaN
    @Volatile var dispRate = Double.NaN
    /** dB range in use (auto range), for the UI scale */
    @Volatile var rangeLo = -110f
    @Volatile var rangeHi = -40f

    private val rowsH = 512
    private var texW = 0
    private var head = 0
    private var rowBuf = FloatArray(64 * 2048)
    private val meta = DoubleArray(128)
    private val rowCenter = DoubleArray(rowsH)
    private val rowRate = DoubleArray(rowsH) // 0 = empty row
    private val offBuf: FloatBuffer = ByteBuffer.allocateDirect(4 * 2 * rowsH).order(ByteOrder.nativeOrder()).asFloatBuffer()
    private var lineCenter = 0.0
    private var lineRate = 0.0
    private var lastCenter = 0.0
    private var lastRate = 0.0
    private var bytes: ByteBuffer = ByteBuffer.allocateDirect(2048)
    private var line = FloatArray(2048)
    private var haveLine = false
    private var floorS = -100f
    private var peakS = -50f

    private var progWf = 0
    private var progLine = 0
    private var texWf = 0
    private var texCmap = 0
    private var texOff = 0
    private var vbo: FloatBuffer = ByteBuffer.allocateDirect(4 * 2 * 4 * 8192).order(ByteOrder.nativeOrder()).asFloatBuffer()
    private var width = 1
    private var height = 1

    private val vsQuad = """#version 300 es
        layout(location = 0) in vec2 aPos;
        out vec2 vUv;
        void main() { vUv = aPos * 0.5 + 0.5; gl_Position = vec4(aPos, 0.0, 1.0); }
    """
    private val fsWf = """#version 300 es
        precision highp float;
        in vec2 vUv;
        uniform sampler2D uWf;
        uniform sampler2D uCmap;
        uniform highp sampler2D uOff; // per row: (row centre - shown centre) / row rate, shown rate / row rate
        uniform float uHead;   // newest row index + 1, in rows
        uniform float uRows;
        uniform vec2 uView;    // lo, hi in 0..1 of the shown span
        out vec4 o;
        void main() {
            float u = mix(uView.x, uView.y, vUv.x);
            float row = uHead - 0.5 - (1.0 - vUv.y) * uRows;   // newest at the top
            int ri = int(mod(floor(row), uRows));
            vec2 off = texelFetch(uOff, ivec2(ri, 0), 0).rg;
            float ur = (u - 0.5) * off.y - off.x + 0.5;         // where that frequency is in the row
            if (off.y <= 0.0 || ur < 0.0 || ur > 1.0) { o = texture(uCmap, vec2(0.0, 0.5)); return; }
            float val = texture(uWf, vec2(ur, (float(ri) + 0.5) / uRows)).r;
            o = texture(uCmap, vec2(val, 0.5));
        }
    """
    private val vsLine = """#version 300 es
        layout(location = 0) in vec2 aPos;
        uniform vec4 uColor;
        out vec4 vColor;
        void main() { vColor = uColor; gl_Position = vec4(aPos, 0.0, 1.0); }
    """
    private val fsLine = """#version 300 es
        precision mediump float;
        in vec4 vColor;
        out vec4 o;
        void main() { o = vColor; }
    """

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        progWf = program(vsQuad, fsWf)
        progLine = program(vsLine, fsLine)
        val t = IntArray(3)
        GLES30.glGenTextures(3, t, 0)
        texWf = t[0]
        texCmap = t[1]
        texOff = t[2]
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texOff)
        GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D, 0, GLES30.GL_RG32F, rowsH, 1, 0, GLES30.GL_RG, GLES30.GL_FLOAT, null)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MIN_FILTER, GLES30.GL_NEAREST)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MAG_FILTER, GLES30.GL_NEAREST)
        // Colour map: navy -> blue -> cyan -> yellow -> red -> white
        val stops = arrayOf(
            floatArrayOf(0f, 0.02f, 0.03f, 0.10f), floatArrayOf(0.25f, 0.05f, 0.15f, 0.55f),
            floatArrayOf(0.45f, 0.05f, 0.65f, 0.85f), floatArrayOf(0.65f, 0.95f, 0.90f, 0.20f),
            floatArrayOf(0.85f, 0.95f, 0.25f, 0.10f), floatArrayOf(1f, 1f, 1f, 1f),
        )
        val cm = ByteBuffer.allocateDirect(256 * 4)
        for (i in 0 until 256) {
            val x = i / 255f
            var k = 0
            while (k < stops.size - 2 && x > stops[k + 1][0]) k++
            val a = stops[k]
            val b = stops[k + 1]
            val f = ((x - a[0]) / (b[0] - a[0])).coerceIn(0f, 1f)
            for (c in 1..3) cm.put(((a[c] + (b[c] - a[c]) * f) * 255).toInt().coerceIn(0, 255).toByte())
            cm.put(255.toByte())
        }
        cm.position(0)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texCmap)
        GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D, 0, GLES30.GL_RGBA, 256, 1, 0, GLES30.GL_RGBA, GLES30.GL_UNSIGNED_BYTE, cm)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MIN_FILTER, GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MAG_FILTER, GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_S, GLES30.GL_CLAMP_TO_EDGE)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_T, GLES30.GL_CLAMP_TO_EDGE)
        texW = 0
        GLES30.glClearColor(0.04f, 0.06f, 0.09f, 1f)
    }

    override fun onSurfaceChanged(gl: GL10?, w: Int, h: Int) {
        width = max(1, w)
        height = max(1, h)
    }

    private fun ensureTexture(bins: Int) {
        if (bins == texW) return
        texW = bins
        head = 0
        rowRate.fill(0.0)
        val zero = ByteBuffer.allocateDirect(bins * rowsH)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texWf)
        GLES30.glPixelStorei(GLES30.GL_UNPACK_ALIGNMENT, 1)
        GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D, 0, GLES30.GL_R8, bins, rowsH, 0, GLES30.GL_RED, GLES30.GL_UNSIGNED_BYTE, zero)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MIN_FILTER, GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MAG_FILTER, GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_S, GLES30.GL_CLAMP_TO_EDGE)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_T, GLES30.GL_REPEAT)
        bytes = ByteBuffer.allocateDirect(bins)
        line = FloatArray(bins)
        haveLine = false
    }

    /** Pull new rows from the engine and upload them. */
    private fun pull() {
        while (true) {
            val r = Native.spectrumRows(rowBuf, meta)
            if (r == 0) return
            val rows = r ushr 16
            val bins = (r and 0xffff) * 16
            if (bins <= 0) return
            if (rowBuf.size < 64 * bins) {
                rowBuf = FloatArray(64 * bins)
                continue
            }
            ensureTexture(bins)
            for (k in 0 until rows) {
                val off = k * bins
                val c = meta[2 * k]
                val rate = meta[2 * k + 1]
                rowCenter[head] = c
                rowRate[head] = rate
                lastCenter = c
                lastRate = rate
                // The averaged spectrum line starts over when the receiver moves
                if (haveLine && (kotlin.math.abs(c - lineCenter) > 0.5 || rate != lineRate)) haveLine = false
                lineCenter = c
                lineRate = rate
                autoRange(off, bins)
                val lo = rangeLo
                val span = max(1f, rangeHi - lo)
                bytes.position(0)
                for (i in 0 until bins) {
                    val v = ((rowBuf[off + i] - lo) / span).coerceIn(0f, 1f)
                    bytes.put((v * 255f).toInt().toByte())
                    line[i] = if (haveLine) line[i] * 0.5f + rowBuf[off + i] * 0.5f else rowBuf[off + i]
                }
                haveLine = true
                bytes.position(0)
                GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texWf)
                GLES30.glPixelStorei(GLES30.GL_UNPACK_ALIGNMENT, 1)
                GLES30.glTexSubImage2D(GLES30.GL_TEXTURE_2D, 0, 0, head, bins, 1, GLES30.GL_RED, GLES30.GL_UNSIGNED_BYTE, bytes)
                head = (head + 1) % rowsH
            }
            if (rows < 64) return
        }
    }

    /** Noise floor (20th percentile of a sample of bins) and peak, smoothed. */
    private fun autoRange(off: Int, bins: Int) {
        val n = min(256, bins)
        val step = bins / n
        val s = FloatArray(n) { rowBuf[off + it * step] }
        s.sort()
        val floor = s[n / 5]
        var peak = -200f
        for (i in 0 until bins) peak = max(peak, rowBuf[off + i])
        floorS += (floor - floorS) * 0.05f
        peakS += (peak - peakS) * 0.05f
        rangeLo = floorS - 6f
        // At least 30 dB above the floor: a few dB of weak beacon still get a visible colour
        rangeHi = max(floorS + 30f, peakS + 6f)
    }

    override fun onDrawFrame(gl: GL10?) {
        try {
            pull()
        } catch (e: Throwable) {
            Log.w("SatHunter", "waterfall: $e")
        }
        GLES30.glViewport(0, 0, width, height)
        GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT)
        if (texW == 0) return
        val specH = (height * spectrumFraction).toInt()
        val wfH = height - specH
        val dc = dispCenter.let { if (it.isNaN()) lastCenter else it }
        val dr = dispRate.let { if (it.isNaN() || it <= 0.0) lastRate else it }
        // Per-row placement on the shown frequency axis
        offBuf.position(0)
        for (k in 0 until rowsH) {
            val rr = rowRate[k]
            if (rr > 0 && dr > 0) {
                offBuf.put(((rowCenter[k] - dc) / rr).toFloat()); offBuf.put((dr / rr).toFloat())
            } else {
                offBuf.put(0f); offBuf.put(0f)
            }
        }
        offBuf.position(0)
        GLES30.glActiveTexture(GLES30.GL_TEXTURE2)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texOff)
        GLES30.glPixelStorei(GLES30.GL_UNPACK_ALIGNMENT, 4)
        GLES30.glTexSubImage2D(GLES30.GL_TEXTURE_2D, 0, 0, 0, rowsH, 1, GLES30.GL_RG, GLES30.GL_FLOAT, offBuf)
        // Waterfall (bottom part)
        GLES30.glViewport(0, 0, width, wfH)
        GLES30.glUseProgram(progWf)
        GLES30.glActiveTexture(GLES30.GL_TEXTURE0)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texWf)
        GLES30.glUniform1i(GLES30.glGetUniformLocation(progWf, "uWf"), 0)
        GLES30.glActiveTexture(GLES30.GL_TEXTURE1)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, texCmap)
        GLES30.glUniform1i(GLES30.glGetUniformLocation(progWf, "uCmap"), 1)
        GLES30.glUniform1i(GLES30.glGetUniformLocation(progWf, "uOff"), 2)
        GLES30.glUniform1f(GLES30.glGetUniformLocation(progWf, "uHead"), head.toFloat())
        GLES30.glUniform1f(GLES30.glGetUniformLocation(progWf, "uRows"), rowsH.toFloat())
        GLES30.glUniform2f(GLES30.glGetUniformLocation(progWf, "uView"), viewLo, viewHi)
        quad()
        GLES30.glActiveTexture(GLES30.GL_TEXTURE0)
        // Spectrum (top part): filled area + line
        if (!haveLine) return
        GLES30.glViewport(0, wfH, width, specH)
        val bins = texW
        val lo = rangeLo
        val span = max(1f, rangeHi - lo)
        // Line bins under the shown axis (the line may be from a slightly different centre)
        val scale = if (lineRate > 0) dr / lineRate else 1.0
        val shift = if (lineRate > 0) (lineCenter - dc) / lineRate else 0.0
        fun bin(u: Double) = ((u - 0.5) * scale - shift + 0.5) * bins
        val b0 = bin(viewLo.toDouble())
        val b1 = bin(viewHi.toDouble())
        val n = min(max(2, (b1 - b0).toInt()), 4096)
        vbo.position(0)
        for (k in 0 until n) {
            val bf = b0 + (b1 - b0) * k / (n - 1)
            val i = bf.toInt()
            val x = -1f + 2f * k / max(1, n - 1)
            val y = if (i < 0 || i >= bins) -1f else -1f + 2f * ((line[i] - lo) / span).coerceIn(0f, 1f) * 0.92f
            vbo.put(x); vbo.put(y)
            vbo.put(x); vbo.put(-1f)
        }
        GLES30.glEnable(GLES30.GL_BLEND)
        GLES30.glBlendFunc(GLES30.GL_SRC_ALPHA, GLES30.GL_ONE_MINUS_SRC_ALPHA)
        GLES30.glUseProgram(progLine)
        val uColor = GLES30.glGetUniformLocation(progLine, "uColor")
        GLES30.glEnableVertexAttribArray(0)
        // Filled area: triangle strip over (top, bottom) pairs
        vbo.position(0)
        GLES30.glVertexAttribPointer(0, 2, GLES30.GL_FLOAT, false, 0, vbo)
        GLES30.glUniform4f(uColor, 0.31f, 0.70f, 1f, 0.18f)
        GLES30.glDrawArrays(GLES30.GL_TRIANGLE_STRIP, 0, n * 2)
        // Line: every other vertex (stride 16 bytes)
        vbo.position(0)
        GLES30.glVertexAttribPointer(0, 2, GLES30.GL_FLOAT, false, 16, vbo)
        GLES30.glUniform4f(uColor, 0.55f, 0.85f, 1f, 1f)
        GLES30.glLineWidth(2f)
        GLES30.glDrawArrays(GLES30.GL_LINE_STRIP, 0, n)
        GLES30.glDisableVertexAttribArray(0)
        GLES30.glDisable(GLES30.GL_BLEND)
    }

    private val quadVerts: FloatBuffer = ByteBuffer.allocateDirect(8 * 4).order(ByteOrder.nativeOrder()).asFloatBuffer().apply {
        put(floatArrayOf(-1f, -1f, 1f, -1f, -1f, 1f, 1f, 1f)); position(0)
    }

    private fun quad() {
        GLES30.glEnableVertexAttribArray(0)
        quadVerts.position(0)
        GLES30.glVertexAttribPointer(0, 2, GLES30.GL_FLOAT, false, 0, quadVerts)
        GLES30.glDrawArrays(GLES30.GL_TRIANGLE_STRIP, 0, 4)
        GLES30.glDisableVertexAttribArray(0)
    }

    private fun shader(type: Int, src: String): Int {
        val s = GLES30.glCreateShader(type)
        GLES30.glShaderSource(s, src.trimIndent())
        GLES30.glCompileShader(s)
        val ok = IntArray(1)
        GLES30.glGetShaderiv(s, GLES30.GL_COMPILE_STATUS, ok, 0)
        if (ok[0] == 0) Log.e("SatHunter", "shader: " + GLES30.glGetShaderInfoLog(s))
        return s
    }

    private fun program(vs: String, fs: String): Int {
        val p = GLES30.glCreateProgram()
        GLES30.glAttachShader(p, shader(GLES30.GL_VERTEX_SHADER, vs))
        GLES30.glAttachShader(p, shader(GLES30.GL_FRAGMENT_SHADER, fs))
        GLES30.glLinkProgram(p)
        val ok = IntArray(1)
        GLES30.glGetProgramiv(p, GLES30.GL_LINK_STATUS, ok, 0)
        if (ok[0] == 0) Log.e("SatHunter", "program: " + GLES30.glGetProgramInfoLog(p))
        return p
    }
}
