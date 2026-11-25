// Sokki: the suggestion strip while dictating, in Sokki's design language
// (the Mac's recording pill, src/meeting-indicator/Wave.tsx):
// - listening: one compact waveform in the middle, the size of the Mac
//   pill's (64 x 20 dp, thin bars mirrored around a centre line, newest
//   sliding in from the right, louder = taller and brighter, fading out to
//   the left), the time quietly beside it. The red stop button says
//   "recording", so there is no second red dot.
// - transcribing: no words. The bars settle into a row of dots and a soft
//   wave of Sokki's blue travels through them until the text is in.
// Calm greys, one blue accent, red only for the stop button.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.util.AttributeSet
import android.util.TypedValue
import android.view.View
import kotlin.math.cos
import kotlin.math.exp
import kotlin.math.sin
import kotlin.math.max
import kotlin.math.min

class VoiceStatusView @JvmOverloads constructor(
    context: Context, attrs: AttributeSet? = null,
) : View(context, attrs) {

    private var state = SokkiVoice.State.IDLE
    private var since = 0L
    private val dp = resources.displayMetrics.density

    // Waveform: the Mac pill's numbers (Wave.tsx), in dp.
    private val barW = 2 * dp
    private val step = 4 * dp
    private val waveW = 64 * dp
    private val waveH = 20 * dp
    private val dotH = 2 * dp
    private val bars = (waveW / step).toInt() + 1
    private val history = FloatArray(bars)
    private val handoff = FloatArray(bars) // bar heights (0..1) when transcribing began
    private var level = 0f
    private var env = 0f
    private var peak = 0f
    private var lastBar = 0L

    private var ink = Color.WHITE // key text colour of the keyboard theme
    private var accent = DARK_ACCENT
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textSize = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 12f, resources.displayMetrics)
        typeface = Typeface.DEFAULT
        fontFeatureSettings = "tnum" // the time doesn't jitter
    }
    private val rect = RectF()

    /** The keyboard theme's text colour; the accent follows light or dark. */
    fun setInk(textColor: Int) {
        ink = textColor
        accent = if (luminance(textColor) > 0.5) DARK_ACCENT else LIGHT_ACCENT
        invalidate()
    }

    fun show(s: SokkiVoice.State) {
        val now = System.currentTimeMillis()
        if (s == SokkiVoice.State.LISTENING && state != s) {
            history.fill(0f); level = 0f; env = 0f; peak = 0f
            lastBar = now
        }
        if (s == SokkiVoice.State.PROCESSING && state == SokkiVoice.State.LISTENING)
            System.arraycopy(history, 0, handoff, 0, bars)
        else if (s == SokkiVoice.State.PROCESSING && state != s)
            handoff.fill(0f)
        if (s != state) since = now
        state = s
        // No words on screen; screen readers still hear what's happening.
        contentDescription = when (s) {
            SokkiVoice.State.LISTENING -> "Listening"
            SokkiVoice.State.PROCESSING -> "Transcribing"
            SokkiVoice.State.IDLE -> null
        }
        invalidate()
    }

    fun level(l: Float) {
        level = l
    }

    /** The look while dictating. 4 (four dots, chosen by Robin) is the
     *  default; 0 = waveform with the time, 1-3 = mini wave, pill, orb. */
    var variant = 4
        set(v) { field = v; invalidate() }

    override fun onDraw(canvas: Canvas) {
        when (state) {
            SokkiVoice.State.LISTENING -> if (variant == 0) drawListening(canvas) else { followLevel(); drawVariant(canvas, true) }
            SokkiVoice.State.PROCESSING -> if (variant == 0) drawWorking(canvas) else drawVariant(canvas, false)
            SokkiVoice.State.IDLE -> return
        }
        postInvalidateOnAnimation()
    }

    /** Loudness, smoothed: fast up, slow down. */
    private fun followLevel() {
        env += (level - env) * (if (level > env) 0.5f else 0.12f)
    }

    private fun drawVariant(canvas: Canvas, listening: Boolean) {
        val t = (System.currentTimeMillis() - since).toFloat()
        val cy = height / 2f
        val cx = (parent as? View)?.let { it.width / 2f - left } ?: (width / 2f)
        val loud = min(1f, env * 1.25f)
        // a slow, organic wobble so the shape is alive between syllables
        fun wobble(i: Int) = if (motion) 0.72f + 0.28f * sin(t / 1000f * 2f * PI_F * 1.7f + i * 1.3f) else 1f
        val settle = easeOut(min(1f, t / 280f))
        val strength = if (motion) easeOut(min(1f, t / 400f)) else 0f
        when (variant) {
            1 -> { // Mini wave: seven bars, about 40 dp
                val n = 7; val bw = 3 * dp; val st = 6 * dp; val hMax = 18 * dp; val dot = 3 * dp
                val w = floatArrayOf(0.45f, 0.7f, 0.9f, 1f, 0.9f, 0.7f, 0.45f)
                val x0 = cx - ((n - 1) * st + bw) / 2
                val p = -1.5f + (n + 2f) * smooth((t % 1000f) / 1000f)
                for (i in 0 until n) {
                    val x = x0 + i * st
                    if (listening) {
                        paint.color = withAlpha(ink, 0.85f)
                        pill(canvas, x, cy, bw, dot + (hMax - dot) * loud * w[i] * wobble(i))
                    } else {
                        val g = strength * exp(-((i - p) * (i - p)) / 1.6f)
                        paint.color = blend(withAlpha(ink, 0.3f), accent, if (motion) g else 0.6f)
                        pill(canvas, x, cy, bw, dot + g * (hMax * 0.6f - dot))
                    }
                }
            }
            2 -> { // Pill: a small blue capsule, like the Mac's recording pill
                val pw = 46 * dp; val ph = 24 * dp
                paint.color = withAlpha(accent, 0.16f)
                rect.set(cx - pw / 2, cy - ph / 2, cx + pw / 2, cy + ph / 2)
                canvas.drawRoundRect(rect, ph / 2, ph / 2, paint)
                paint.color = accent
                if (listening) {
                    val n = 5; val bw = 2.5f * dp; val st = 5.5f * dp; val hMax = 12 * dp; val dot = 2.5f * dp
                    val w = floatArrayOf(0.55f, 0.85f, 1f, 0.85f, 0.55f)
                    val x0 = cx - ((n - 1) * st + bw) / 2
                    for (i in 0 until n) pill(canvas, x0 + i * st, cy, bw, dot + (hMax - dot) * loud * w[i] * wobble(i))
                } else {
                    val d = 4 * dp; val st = 8 * dp
                    for (i in 0 until 3) {
                        val ph2 = ((t / 900f) - i * 0.16f).mod(1f)
                        val up = if (motion && ph2 < 0.5f) sin(ph2 * 2 * PI_F) else 0f
                        canvas.drawCircle(cx + (i - 1) * st, cy - up * 3.5f * dp, d / 2, paint)
                    }
                }
            }
            3 -> { // Orb: one dot breathing with the voice; a blue comet while transcribing
                if (listening) {
                    val r = 5 * dp + 4 * dp * loud
                    paint.color = withAlpha(ink, 0.10f + 0.10f * loud)
                    canvas.drawCircle(cx, cy, r + 3 * dp + 5 * dp * loud, paint)
                    paint.color = withAlpha(ink, 0.88f)
                    canvas.drawCircle(cx, cy, r, paint)
                } else {
                    val r = 5 * dp - 1.5f * dp * settle
                    paint.color = blend(withAlpha(ink, 0.88f), accent, settle)
                    canvas.drawCircle(cx, cy, r, paint)
                    val ring = 10 * dp
                    val head = if (motion) (t / 1100f) * 2 * PI_F else 0f
                    for (k in 0 until 10) {
                        val a = head - k * 0.22f
                        paint.color = withAlpha(accent, settle * (1f - k / 10f) * 0.9f)
                        canvas.drawCircle(cx + ring * cos(a), cy + ring * sin(a), (1.6f - k * 0.08f) * dp, paint)
                    }
                }
            }
            4 -> { // Four dots: stretch with the voice, bounce in turn while transcribing
                val n = 4; val d = 6 * dp; val st = 11 * dp
                val w = floatArrayOf(0.75f, 1f, 0.9f, 0.7f)
                val x0 = cx - ((n - 1) * st + d) / 2
                for (i in 0 until n) {
                    val x = x0 + i * st
                    if (listening) {
                        paint.color = withAlpha(ink, 0.85f)
                        pill(canvas, x, cy, d, d + 14 * dp * loud * w[i] * wobble(i))
                    } else {
                        val ph2 = ((t / 1000f) - i * 0.12f).mod(1f)
                        val up = if (motion && ph2 < 0.45f) sin(ph2 / 0.45f * PI_F) else 0f
                        paint.color = blend(withAlpha(ink, 0.35f), accent, if (motion) up * strength + 0.25f else 0.6f)
                        pill(canvas, x, cy - up * strength * 5 * dp, d, d)
                    }
                }
            }
        }
    }

    /** A rounded bar of width [w] and height [h], centred on [cy], left edge [x]. */
    private fun pill(canvas: Canvas, x: Float, cy: Float, w: Float, h: Float) {
        rect.set(x, cy - h / 2, x + w, cy + h / 2)
        canvas.drawRoundRect(rect, w / 2, w / 2, paint)
    }

    private val motion get() = ValueAnimator.areAnimatorsEnabled()

    /** Left edge of the waveform: waveform and time are centred together,
     *  and the dots of "transcribing" stay where the waveform was. */
    private val timeGap = 10 * dp
    private val waveX get() = mid - (waveW + timeGap + label.measureText("0:00")) / 2f

    /** The middle of the whole strip (the mic sits to the right of this view),
     *  in this view's coordinates. */
    private val mid get() = ((parent as? View)?.let { it.width / 2f - left } ?: (width / 2f))
        .coerceIn(waveW / 2f, width - waveW / 2f)

    private fun drawListening(canvas: Canvas) {
        val now = System.currentTimeMillis()
        // Fast attack, slow release, so a bar shows the syllable it caught.
        env += (level - env) * (if (level > env) 0.6f else 0.15f)
        peak = max(peak, env)
        while (now - lastBar >= BAR_MS) {
            System.arraycopy(history, 1, history, 0, bars - 1)
            history[bars - 1] = peak
            peak = env
            lastBar += BAR_MS
        }
        val cy = height / 2f
        val x0 = waveX
        val slide = if (motion) ((now - lastBar).toFloat() / BAR_MS) * step else 0f
        for (i in 0 until bars) {
            val bx = x0 + i * step - slide
            if (bx < x0 - barW || bx > x0 + waveW) continue
            val v = history[i]
            val edge = min(1f, (bx - x0 + step) / (step * 6)) // fade out to the left
            val h = max(dotH, v * waveH)
            paint.color = withAlpha(ink, (0.35f + 0.65f * min(1f, v * 1.6f)) * edge)
            bar(canvas, bx, cy, h)
        }

        // The time, quiet, beside the waveform (from the first second on).
        val elapsed = (now - since) / 1000
        if (elapsed >= 1) {
            val fade = min(1f, (now - since - 1000) / 240f)
            label.color = withAlpha(ink, 0.45f * fade)
            val time = "%d:%02d".format(elapsed / 60, elapsed % 60)
            canvas.drawText(time, x0 + waveW + timeGap, cy - (label.descent() + label.ascent()) / 2, label)
        }
    }

    private fun drawWorking(canvas: Canvas) {
        val t = System.currentTimeMillis() - since
        val cy = height / 2f
        // The last waveform settles into dots in the first 320 ms and glides
        // to the middle (the time beside it is gone)...
        val settle = easeOut(min(1f, t / 320f))
        val x0 = waveX + (mid - waveW / 2f - waveX) * settle
        // ...while a soft blue wave starts travelling through them, left to
        // right, once every 1.2 s (eased, so it glides in and out).
        val phase = ((t % CYCLE_MS) / CYCLE_MS.toFloat())
        val p = -2.5f + (bars + 4f) * smooth(phase)
        val strength = if (motion) easeOut(min(1f, t / 500f)) else 0f
        for (i in 0 until bars) {
            val bx = x0 + i * step
            val g = strength * exp(-((i - p) * (i - p)) / 5f)
            val wave = dotH + g * (waveH * 0.55f - dotH)
            val before = max(dotH, handoff[i] * waveH)
            val h = before + (wave - before) * settle
            val c = blend(withAlpha(ink, 0.28f), accent, if (motion) g else 0.6f)
            paint.color = c
            bar(canvas, bx, cy, h)
        }
    }

    private fun bar(canvas: Canvas, x: Float, cy: Float, h: Float) {
        rect.set(x, cy - h / 2, x + barW, cy + h / 2)
        canvas.drawRoundRect(rect, barW / 2, barW / 2, paint)
    }

    private fun easeOut(x: Float) = 1 - (1 - x) * (1 - x) * (1 - x)
    private fun smooth(x: Float) = x * x * (3 - 2 * x)

    private fun blend(a: Int, b: Int, f: Float): Int {
        val k = f.coerceIn(0f, 1f)
        fun ch(x: Int, y: Int) = (x + (y - x) * k).toInt()
        return Color.argb(ch(Color.alpha(a), Color.alpha(b)), ch(Color.red(a), Color.red(b)),
            ch(Color.green(a), Color.green(b)), ch(Color.blue(a), Color.blue(b)))
    }

    private fun withAlpha(c: Int, a: Float) =
        Color.argb((Color.alpha(c) * a).toInt().coerceIn(0, 255), Color.red(c), Color.green(c), Color.blue(c))

    private fun luminance(c: Int) =
        (0.2126 * Color.red(c) + 0.7152 * Color.green(c) + 0.0722 * Color.blue(c)) / 255

    companion object {
        const val RED = 0xFFFF453A.toInt()
        /** Sokki's accent: #8ab4ff on dark, #3567cc on light. */
        const val DARK_ACCENT = 0xFF8AB4FF.toInt()
        const val LIGHT_ACCENT = 0xFF3567CC.toInt()
        private const val BAR_MS = 55L
        private const val CYCLE_MS = 1200L
        private const val PI_F = Math.PI.toFloat()
    }
}
