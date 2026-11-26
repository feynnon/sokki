// Sokki: a journal of what the user writes, in every app (when switched on).
//
// One entry per piece of writing: the text a field held just before it was
// emptied (a message sent) or left, with the app, the time, and the text the
// field already held when it was opened (so edits of existing text can be
// told apart). Covers typing, dictation and pasting alike.
//
// Never in password fields, fields that ask for no learning (incognito), or
// in incognito mode. Stays on the phone: <app files>/typing_log/journal-<date>.jsonl
// (left out of backups). Off by default; on while <app files>/typing_log/journal_on
// exists, so the Sokki app's developer page (another process) can switch it.
// A dictation adds what the speech engine wrote ("dictated"), so the entry shows
// raw transcript next to the text that was finally sent. "typed" lists the
// parts of the text typed key by key ([start, end) in code points): not
// dictation, pastes, suggestion picks, autocorrections, swipes or the prefill.
// The Mac's word learning reads only those (same field on the Mac).
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.inputmethodservice.InputMethodService
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.util.Log
import android.view.inputmethod.EditorInfo
import androidx.core.content.edit
import helium314.keyboard.latin.settings.Settings
import helium314.keyboard.latin.utils.prefs
import org.json.JSONObject
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.Executors

object Journal {
    private const val TAG = "SokkiJournal"
    private const val MAX = 4_000 // chars read around the cursor, each side (long notes: the part being edited)
    private const val THROTTLE_MS = 150L // read the field at most this often; a send tap comes later than this

    private var dir: File? = null
    private val io = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())

    // the field being written in
    private var ims: InputMethodService? = null
    private var on = false
    private var app = ""
    private var hint = ""
    private var opened = 0L
    private var prefill = ""
    private var last = ""
    private var lastChange = 0L
    private var lastRead = 0L
    private var pending = false
    /** Which parts of the field's text were typed key by key. */
    private val typed = TypedSpans()
    /** Since the last snapshot: a key was tapped / text came from elsewhere. */
    private var keyTapped = false
    private var fromElsewhere = false
    private var corrected = false

    /** A key was tapped (LatinIME.onEvent). */
    @JvmStatic fun keyTyped() { keyTapped = true }

    /** Autocorrect changed the word just typed: typed if only its case changed. */
    @JvmStatic fun autocorrected() { corrected = true }

    /** Text that wasn't typed key by key: a suggestion,
     * a swipe, a paste from the keyboard's clipboard, Sokki's spacing fix. */
    @JvmStatic fun otherInput() { fromElsewhere = true }

    /** Dictations into this field: what the speech engine wrote, before any change. */
    private val dictations = org.json.JSONArray()

    private var enabled = false

    @JvmStatic fun init(context: Context) {
        dir = File(context.applicationContext.filesDir, "typing_log")
        // Before 24.11.2026 the switch was a keyboard setting.
        val prefs = context.prefs()
        if (prefs.getBoolean(Settings.PREF_SOKKI_JOURNAL, false)) {
            setOn(context, true)
            prefs.edit { remove(Settings.PREF_SOKKI_JOURNAL) }
        }
    }

    private fun flag(context: Context) = File(context.applicationContext.filesDir, "typing_log/journal_on")

    @JvmStatic fun isOn(context: Context) = flag(context).exists()

    @JvmStatic fun setOn(context: Context, on: Boolean) {
        val f = flag(context)
        try {
            if (on) { f.parentFile?.mkdirs(); f.createNewFile() } else f.delete()
        } catch (e: Exception) {
            Log.w(TAG, "switch", e)
        }
    }

    private fun allowed(ei: EditorInfo?): Boolean {
        val v = Settings.getValues() ?: return false
        if (!enabled || v.mIncognitoModeEnabled || ei == null) return false
        val cls = ei.inputType and InputType.TYPE_MASK_CLASS
        val variation = ei.inputType and InputType.TYPE_MASK_VARIATION
        if (cls != InputType.TYPE_CLASS_TEXT) return false // numbers, PINs, phone numbers, dates
        return variation != InputType.TYPE_TEXT_VARIATION_PASSWORD
            && variation != InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
            && variation != InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD
    }

    /** A field was opened (LatinIME.onStartInputView, settings already loaded). */
    @JvmStatic fun onStart(service: InputMethodService, ei: EditorInfo?, restarting: Boolean) {
        if (restarting && on && ims === service) return
        flush()
        ims = service
        enabled = isOn(service)
        on = allowed(ei)
        if (!on) return
        app = ei?.packageName ?: ""
        hint = ei?.hintText?.toString()?.take(80) ?: ""
        opened = System.currentTimeMillis()
        prefill = read()
        last = prefill
        lastChange = 0L
        clearDictations()
        typed.reset()
        keyTapped = false
        fromElsewhere = false
        corrected = false
    }

    /** A dictation into this field ended (SokkiVoice); [raw] is the engine's text. */
    @JvmStatic fun dictated(raw: String) {
        if (!on || raw.isBlank()) return
        dictations.put(JSONObject().put("at", System.currentTimeMillis()).put("raw", raw))
    }

    private fun clearDictations() {
        while (dictations.length() > 0) dictations.remove(0)
    }

    /** The field's text or selection changed (LatinIME.onUpdateSelection). */
    @JvmStatic fun onChange() {
        if (!on) return
        if (SokkiVoice.busy) fromElsewhere = true // dictation writes into the field
        if (pending) return
        pending = true
        val wait = (lastRead + THROTTLE_MS - System.currentTimeMillis()).coerceAtLeast(0)
        main.postDelayed({ pending = false; snapshot() }, wait)
    }

    /** The field was left (LatinIME.onFinishInput). */
    @JvmStatic fun onFinish() {
        if (!on) return
        snapshot()
        flush()
        on = false
    }

    private fun snapshot() {
        if (!on) return
        lastRead = System.currentTimeMillis()
        val now = read()
        val wasTyped = keyTapped && !fromElsewhere && (!corrected || TypedSpans.caseOnly(last, now))
        keyTapped = false
        fromElsewhere = false
        corrected = false
        if (now == last) return
        if (now.isBlank() && last.isNotBlank()) {
            flush() // emptied: the message was sent
            prefill = ""
            typed.reset()
        } else {
            typed.apply(last, now, wasTyped)
            lastChange = lastRead
        }
        last = now
    }

    private fun read(): String {
        val ic = ims?.currentInputConnection ?: return ""
        return try {
            val before = ic.getTextBeforeCursor(MAX, 0) ?: ""
            val selected = ic.getSelectedText(0) ?: ""
            val after = ic.getTextAfterCursor(MAX, 0) ?: ""
            "$before$selected$after"
        } catch (e: Exception) {
            Log.w(TAG, "read", e); ""
        }
    }

    private fun flush() {
        val text = last
        if (!on || text.isBlank() || text == prefill || lastChange == 0L) { last = prefill; clearDictations(); return }
        val entry = JSONObject()
            .put("t", "entry")
            .put("opened", opened)
            .put("written", lastChange)
            .put("app", app)
            .put("hint", hint)
            .put("prefill", prefill)
            .put("text", text)
        if (dictations.length() > 0) entry.put("dictated", org.json.JSONArray(dictations.toString()))
        entry.put("typed", org.json.JSONArray().apply {
            for (r in typed.codePoints(text)) put(org.json.JSONArray().put(r[0]).put(r[1]))
        })
        clearDictations()
        last = ""
        prefill = ""
        lastChange = 0L
        val d = dir ?: return
        io.execute {
            try {
                d.mkdirs()
                val day = SimpleDateFormat("yyyy-MM-dd", Locale.ROOT).format(Date(entry.getLong("written")))
                File(d, "journal-$day.jsonl").appendText(entry.toString() + "\n")
            } catch (e: Exception) {
                Log.w(TAG, "write", e)
            }
        }
    }
}
