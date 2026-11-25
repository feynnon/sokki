// Sokki (debug builds only): the dictation status in the keyboard's light and
// dark strip, cycling listening (simulated speech) -> transcribing -> idle,
// so the design can be checked without a microphone.
//   adb shell am start -n dev.verbatim.mobile/dev.verbatim.sokki.VoiceStatusPreviewActivity
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import kotlin.math.abs
import kotlin.math.sin
import kotlin.random.Random

class VoiceStatusPreviewActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private val views = mutableListOf<Pair<VoiceStatusView, ImageView>>()
    private var start = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        fun dp(v: Int) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(Color.GRAY); setPadding(0, dp(60), 0, 0) }
        for (variant in 0..4) for ((bg, ink) in listOf(0xFFE9EBEE.toInt() to 0xFF111214.toInt(), 0xFF17181B.toInt() to 0xFFECEDEF.toInt())) {
            val strip = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; setBackgroundColor(bg); gravity = Gravity.CENTER_VERTICAL }
            strip.addView(android.widget.TextView(this).apply {
                text = if (variant == 0) "old" else ('A' + variant - 1).toString()
                setTextColor(0xFF8D929B.toInt()); textSize = 12f; gravity = Gravity.CENTER
            }, LinearLayout.LayoutParams(dp(44), dp(40)))
            val status = VoiceStatusView(this).apply { setInk(ink); this.variant = variant }
            strip.addView(status, LinearLayout.LayoutParams(0, dp(40), 1f))
            val button = ImageView(this).apply { scaleType = ImageView.ScaleType.CENTER }
            strip.addView(button, LinearLayout.LayoutParams(dp(44), dp(40)))
            root.addView(FrameLayout(this).apply { setBackgroundColor(bg); setPadding(0, dp(4), 0, dp(4)); addView(strip) })
            views += status to button
        }
        setContentView(root)
        start = System.currentTimeMillis()
        tick()
    }

    private var phase: SokkiVoice.State? = null

    private fun tick() {
        val t = (System.currentTimeMillis() - start) % 11_000
        val s = when {
            t < 6_500 -> SokkiVoice.State.LISTENING
            t < 9_500 -> SokkiVoice.State.PROCESSING
            else -> SokkiVoice.State.IDLE
        }
        // syllables: bursts of ~180 ms, short pauses between words
        val word = (t / 900) % 3 != 2L
        val syl = abs(sin(t / 180.0 * Math.PI)).toFloat()
        val level = if (word) (0.25f + 0.6f * syl) * (0.8f + 0.2f * Random.nextFloat()) else 0.03f
        for ((v, b) in views) {
            if (s != phase) {
                v.show(s)
                b.setImageDrawable(if (s == SokkiVoice.State.LISTENING) SokkiDrawables.stop(this) else null)
                v.visibility = if (s == SokkiVoice.State.IDLE) android.view.View.INVISIBLE else android.view.View.VISIBLE
            }
            v.level(level)
        }
        phase = s
        main.postDelayed(::tick, 30)
    }

    override fun onDestroy() {
        main.removeCallbacksAndMessages(null)
        super.onDestroy()
    }
}
