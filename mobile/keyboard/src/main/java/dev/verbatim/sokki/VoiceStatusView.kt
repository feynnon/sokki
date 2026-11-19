// Sokki: the suggestion strip while dictating, in Sokki's design language
// (the Mac's recording pill and dictation overlay):
// - listening: pulsing red dot · scrolling waveform · timer, centred. Thin
//   bars mirrored around a centre line, newest on the right sliding left,
//   louder bars taller and brighter, fading out at the left edge.
// - transcribing: a small accent spinner · "Transcribing".
// Calm greys, one blue accent, red only for recording.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.util.AttributeSet
import android.util.TypedValue
import android.view.View
import kotlin.math.max
import kotlin.math.min
import kotlin.math.sin

class VoiceStatusView @JvmOverloads constructor(
    context: Context, attrs: AttributeSet? = null,
) : View(context, attrs) {

    private var state = SokkiVoice.State.IDLE
    private var since = 0L
    private val dp = resources.displayMetrics.density

    // Waveform (same numbers as the Mac pill's Wave.tsx, in dp).
    private val barW = 2 * dp
    private val step = 4 * dp
    private val waveW = 132 * dp
    private val waveH = 20 * dp
    private val bars = (waveW / step).toInt() + 1
    private val history = FloatArray(bars)
    private var level = 0f
    private var env = 0f
    private var peak = 0f
    private var lastBar = 0L

    private var ink = Color.WHITE // key text colour of the keyboard theme
    private var accent = DARK_ACCENT
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val label = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        textSize = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, 13f, resources.displayMetrics)
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        isFakeBoldText = false
    }
    private val rect = RectF()

    /** The keyboard theme's text colour; the accent follows light or dark. */
    fun setInk(textColor: Int) {
        ink = textColor
        accent = if (luminance(textColor) > 0.5) DARK_ACCENT else LIGHT_ACCENT
        invalidate()
    }

    fun show(s: SokkiVoice.State) {
        if (s == SokkiVoice.State.LISTENING && state != s) {
            history.fill(0f); level = 0f; env = 0f; peak = 0f
            lastBar = System.currentTimeMillis()
        }
        if (s != state) since = System.currentTimeMillis()
        state = s
        invalidate()
    }

    fun level(l: Float) {
        level = l
    }

    override fun onDraw(canvas: Canvas) {
        when (state) {
            SokkiVoice.State.LISTENING -> drawListening(canvas)
            SokkiVoice.State.PROCESSING -> drawTranscribing(canvas)
            SokkiVoice.State.IDLE -> return
        }
        postInvalidateOnAnimation()
    }

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
        val elapsed = (now - since) / 1000
        val time = "%d:%02d".format(elapsed / 60, elapsed % 60)
        label.color = withAlpha(ink, 0.65f)
        val timeW = label.measureText("0:00")
        val dot = 7 * dp
        val gap = 10 * dp
        val total = dot + gap + waveW + gap + timeW
        val cy = height / 2f
        var x = (width - total) / 2f

        // Recording dot, pulsing like the pill's.
        val pulse = 0.55f + 0.45f * ((sin((now - since) / 1600.0 * 2 * Math.PI) + 1) / 2).toFloat()
        paint.color = withAlpha(RED, pulse)
        canvas.drawCircle(x + dot / 2, cy, dot / 2, paint)
        x += dot + gap

        // Waveform, sliding left between new bars.
        val slide = ((now - lastBar).toFloat() / BAR_MS) * step
        for (i in 0 until bars) {
            val bx = x + i * step - slide
            if (bx < x - barW || bx > x + waveW) continue
            val v = history[i]
            val edge = min(1f, (bx - x + step) / (step * 6))
            val h = max(2 * dp, v * waveH)
            paint.color = withAlpha(ink, (0.35f + 0.65f * min(1f, v * 1.6f)) * edge)
            rect.set(bx, cy - h / 2, bx + barW, cy + h / 2)
            canvas.drawRoundRect(rect, barW / 2, barW / 2, paint)
        }
        x += waveW + gap

        canvas.drawText(time, x, cy - (label.descent() + label.ascent()) / 2, label)
    }

    private fun drawTranscribing(canvas: Canvas) {
        val text = "Transcribing"
        label.color = withAlpha(ink, 0.65f)
        val size = 13 * dp
        val gap = 8 * dp
        val total = size + gap + label.measureText(text)
        val cy = height / 2f
        val x = (width - total) / 2f
        // Spinner: a faint ring with an accent arc, one turn per 0.7 s.
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 2 * dp
        rect.set(x + dp, cy - size / 2 + dp, x + size - dp, cy + size / 2 - dp)
        paint.color = withAlpha(ink, 0.15f)
        canvas.drawOval(rect, paint)
        paint.color = accent
        paint.strokeCap = Paint.Cap.ROUND
        val turn = ((System.currentTimeMillis() - since) % 700) / 700f * 360f
        canvas.drawArc(rect, turn - 90, 90f, false, paint)
        paint.style = Paint.Style.FILL
        canvas.drawText(text, x + size + gap, cy - (label.descent() + label.ascent()) / 2, label)
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
    }
}
