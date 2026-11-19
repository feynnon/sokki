// Sokki: icons drawn in code, matching the Mac app.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.LayerDrawable

object SokkiDrawables {
    /** Stop: a red square on a soft red disc (the recording pill's stop). */
    fun stop(context: Context): Drawable {
        val dp = context.resources.displayMetrics.density
        val disc = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(0x2EFF453A) // red at 18 %
            setSize((30 * dp).toInt(), (30 * dp).toInt())
        }
        val square = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = 2.5f * dp
            setColor(VoiceStatusView.RED)
            setSize((11 * dp).toInt(), (11 * dp).toInt())
        }
        return LayerDrawable(arrayOf(disc, square)).apply {
            val inset = ((30 - 11) / 2 * dp).toInt()
            setLayerInset(1, inset, inset, inset, inset)
        }
    }
}
