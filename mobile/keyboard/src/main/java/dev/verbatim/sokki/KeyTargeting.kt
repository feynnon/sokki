// Sokki: letter-aware key edges ("dynamic hit targets", as on the iPhone).
//
// The keys you see never move. A tap near the edge of a letter key counts
// for the neighbouring letter if that letter is much more likely after the
// two letters before (after "sc", a tap between h and j is h). The middle of
// every key always gives that key ("anchored" targets, Gunawardana, Paek &
// Meek 2010), so nothing becomes impossible to type. Nothing is replaced
// after the fact, and nothing is shown.
//
// Same model and numbers as mobile/keyboard/tools/letter_model.py, which
// builds the letter tables (assets/sokki/letters_<lang>.bin) and replays
// recorded taps to measure whether this helps. Off by default until a real
// typing test shows it does (Settings.PREF_SOKKI_KEY_TARGETING).
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.util.Log
import helium314.keyboard.keyboard.Key
import helium314.keyboard.keyboard.Keyboard
import helium314.keyboard.latin.settings.Settings

object KeyTargeting {
    private const val TAG = "SokkiKeyTargeting"
    private const val LETTERS = "abcdefghijklmnopqrstuvwxyzäöüß"
    private const val N = LETTERS.length + 1 // + start of word
    private const val SCALE = 16f

    // Keep equal to letter_model.py. Distances in key widths / heights.
    const val SIGMA_X = 0.30f
    const val SIGMA_Y = 0.30f
    const val WEIGHT = 1.0f
    const val CORE = 0.5f
    const val REACH = 1.0f

    /** Text before the cursor (LatinIME's cached copy; cheap). */
    @JvmField var textBefore: (() -> CharSequence?)? = null

    private var appContext: Context? = null
    private val tables = HashMap<String, ByteArray?>()

    @JvmStatic fun init(context: Context) {
        appContext = context.applicationContext
    }

    @JvmStatic fun enabled(): Boolean = Settings.getValues()?.mSokkiKeyTargeting == true

    private fun table(lang: String): ByteArray? = synchronized(tables) {
        tables.getOrPut(lang) {
            try {
                appContext?.assets?.open("sokki/letters_$lang.bin")?.use { it.readBytes() }
                    ?.takeIf { it.size == N * N * N }
            } catch (_: Exception) {
                null // no table for this language: geometry only
            }
        }
    }

    private fun sym(c: Char?): Int {
        if (c == null) return 0
        val i = LETTERS.indexOf(c.lowercaseChar())
        return if (i < 0) 0 else i + 1
    }

    private fun letterIndex(key: Key): Int {
        val code = key.code
        if (code <= 0 || Character.isSupplementaryCodePoint(code)) return -1
        return LETTERS.indexOf(code.toChar().lowercaseChar())
    }

    /**
     * The key a tap at (x, y) (keyboard coordinates, as in KeyDetector) counts
     * for, given the key it [hit] by geometry. Returns [hit] unless targeting
     * is on and the tap is near a letter key's edge.
     */
    @JvmStatic fun adjust(keyboard: Keyboard, hit: Key?, x: Int, y: Int): Key? {
        if (hit == null || !enabled() || !keyboard.mId.element.isAlphabet) return hit
        if (letterIndex(hit) < 0) return hit
        val box = hit.hitBox
        if (box.width() <= 0 || box.height() <= 0) return hit
        if (Math.abs(x - box.exactCenterX()) < CORE * box.width() / 2
            && Math.abs(y - box.exactCenterY()) < CORE * box.height() / 2) return hit
        val table = table(keyboard.mId.locale.language) ?: return hit
        val before = try { textBefore?.invoke() } catch (e: Exception) {
            Log.w(TAG, "text before cursor", e); null
        } ?: ""
        val c2 = sym(before.lastOrNull())
        val c1 = if (c2 != 0 && before.length >= 2) sym(before[before.length - 2]) else 0
        var best = hit
        var bestScore = Float.NEGATIVE_INFINITY
        for (k in keyboard.getNearestKeys(x, y)) {
            val li = letterIndex(k)
            if (li < 0) continue
            val b = k.hitBox
            if (b.width() <= 0 || b.height() <= 0) continue
            val dx = (x - b.exactCenterX()) / b.width()
            val dy = (y - b.exactCenterY()) / b.height()
            if (dx * dx + dy * dy > REACH * REACH * 2.25f) continue
            val lp = -(table[(c1 * N + c2) * N + li + 1].toInt() and 0xff) / SCALE
            val score = -0.5f * ((dx / SIGMA_X) * (dx / SIGMA_X) + (dy / SIGMA_Y) * (dy / SIGMA_Y)) + WEIGHT * lp
            if (score > bestScore) {
                bestScore = score
                best = k
            }
        }
        return best
    }
}
