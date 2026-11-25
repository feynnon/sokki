// Sokki: records every tap in everyday typing (when switched on), and every
// autocorrect fix and undo, to learn
// the user's own typing pattern and to measure whether smart key edges help
// in real use (mobile/keyboard/tools/typing_eval.py daily).
//
// Never records in password fields or fields that ask for no learning
// (incognito), or when HeliBoard's incognito mode is on. Stays on the phone:
// <app files>/typing_log/taps-<date>.jsonl. Off by default
// (Settings.PREF_SOKKI_RECORD_TAPS).
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.util.Log
import helium314.keyboard.keyboard.Key
import helium314.keyboard.keyboard.KeyDetector
import helium314.keyboard.keyboard.Keyboard
import helium314.keyboard.latin.settings.Settings
import org.json.JSONObject
import java.io.File
import java.io.Writer
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors

object TapLog {
    private const val TAG = "SokkiTapLog"
    private var dir: File? = null
    private val io = Executors.newSingleThreadExecutor()
    private var writer: Writer? = null
    private var day: String? = null
    private var lastKeyboard: Keyboard? = null

    @JvmStatic fun init(context: Context) {
        dir = File(context.applicationContext.filesDir, "typing_log")
    }

    @JvmStatic fun enabled(): Boolean {
        val v = Settings.getValues() ?: return false
        return v.mSokkiRecordTaps && !v.mIncognitoModeEnabled
    }

    @JvmStatic fun logTap(detector: KeyDetector, sent: Key, x: Int, y: Int) {
        val keyboard = detector.keyboard ?: return
        val tx = detector.getTouchX(x)
        val ty = detector.getTouchY(y)
        val geo = detector.detectGeometricKey(tx, ty)
        val v = Settings.getValues()
        val layout = if (keyboard !== lastKeyboard) TypingTest.layout(keyboard) else null
        lastKeyboard = keyboard
        val tap = JSONObject()
            .put("t", "tap")
            .put("ms", System.currentTimeMillis())
            .put("app", v?.mInputAttributes?.mTargetApplicationPackageName ?: "")
            .put("x", tx).put("y", ty)
            .put("geo", geo?.code ?: JSONObject.NULL)
            .put("out", sent.code)
            .put("kt", v?.mSokkiKeyTargeting == true)
        io.execute {
            try {
                val w = writerForToday() ?: return@execute
                if (layout != null) { w.write(layout.toString()); w.write("\n") }
                w.write(tap.toString()); w.write("\n"); w.flush()
            } catch (e: Exception) {
                Log.w(TAG, "write", e)
            }
        }
    }

    /**
     * An autocorrect fix: [how] "word" (one word) or "space" (a letter moved
     * across the space), what was [typed] and what it became. With the taps
     * around it, this shows which fixes help (typing_eval.py fixes).
     */
    @JvmStatic fun logFix(how: String, typed: String, to: String) = logEvent("fix", how, typed, to)

    /** The user undid a fix with backspace right after it. */
    @JvmStatic fun logUndo(typed: String, to: String) = logEvent("undo", null, typed, to)

    private fun logEvent(t: String, how: String?, typed: String, to: String) {
        if (!enabled()) return
        val v = Settings.getValues()
        val e = JSONObject()
            .put("t", t)
            .put("ms", System.currentTimeMillis())
            .put("app", v?.mInputAttributes?.mTargetApplicationPackageName ?: "")
            .put("typed", typed).put("to", to)
        if (how != null) e.put("how", how)
        io.execute {
            try {
                val w = writerForToday() ?: return@execute
                w.write(e.toString()); w.write("\n"); w.flush()
            } catch (ex: Exception) {
                Log.w(TAG, "write", ex)
            }
        }
    }

    private fun writerForToday(): Writer? {
        val d = dir ?: return null
        val today = SimpleDateFormat("yyyy-MM-dd", Locale.ROOT).format(Date())
        if (today != day || writer == null) {
            try { writer?.close() } catch (_: Exception) {}
            d.mkdirs()
            writer = File(d, "taps-$today.jsonl").let { java.io.FileWriter(it, true).buffered() }
            day = today
            lastKeyboard = null // new file: write the layout again with the next tap
        }
        return writer
    }
}
