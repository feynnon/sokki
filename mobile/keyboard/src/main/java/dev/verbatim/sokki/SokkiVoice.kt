// Sokki: voice typing in the keyboard. Tap the mic (top right of the
// suggestion strip), speak, tap again. The text goes into whatever field is
// open while you speak, as on the Mac: a streaming model adds words about
// every half second, and words it has moved past never change, so they are
// typed for good at once; only the word still being heard is underlined.
// Through the paired Mac when it's reachable (faster, more accurate: its
// Parakeet on the GPU); then the whole text stays underlined, as it may still
// change, and is typed for good at the stop. Otherwise, or when the Mac stops
// answering, all on the phone. The speech engine is Rust
// (mobile/src-tauri/src/keyboard_voice.rs); the model is the app's download.
//
// Feels instant: recording starts on the tap, even while the engine is
// still loading (the audio waits and is transcribed once it's ready), and
// the engine is loaded and warmed up as soon as the keyboard opens, and
// brought back into memory when it opens after a while.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.inputmethod.InputConnection
import android.widget.Toast
import java.io.File
import kotlin.math.sqrt

object SokkiVoice {
    private const val TAG = "SokkiVoice"
    private const val RATE = 16_000
    private const val CHUNK = RATE / 20 // 50 ms: level updates
    private const val BLOCK = RATE / 5 // 200 ms: sent to the engine
    private const val DONE = "\u0000done"
    private const val ERROR = "\u0000error:"
    private const val TEXT = "\u0000text:"
    private const val FINAL = "\u0000final:"
    private const val LIVE = "\u0000live:"
    const val MODEL_FILE = "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf"
    /** The model before 2026-11-25; the app replaces it by itself. */
    private const val OLD_MODEL_FILE = "parakeet-ultra-Q8_0.gguf"
    /** Re-warm on keyboard open when the engine was last used this long ago:
     * Android pushes an idle keyboard's model out of memory. */
    private const val REWARM_AFTER_MS = 30_000L

    enum class State { IDLE, LISTENING, PROCESSING }

    /** Main thread: the state changed. */
    var onState: ((State) -> Unit)? = null
    /** Main thread, ~20 times a second while listening: loudness 0..1. */
    var onLevel: ((Float) -> Unit)? = null
    /** Main thread: a dictation ended that typed text (the keyboard catches up). */
    var onTyped: (() -> Unit)? = null

    @Volatile var state = State.IDLE
        private set
    /** Dictating: the keyboard leaves the field to Sokki until it's done. */
    val busy get() = state != State.IDLE
    private val main = Handler(Looper.getMainLooper())
    private var appContext: Context? = null
    private var connection: (() -> InputConnection?)? = null
    private var typedAny = false
    /** The word still being heard is in the field, underlined. */
    private var composing = false
    /** " " before this dictation's text when the field had a word right before it. */
    private var lead: String? = null
    /** How much of this dictation's text is typed for good. */
    private var typed = 0
    /** The text as last shown. */
    private var lastText = ""
    /** Each start is a new session; a poll loop of an older one ends. */
    @Volatile private var session = 0

    // The engine library is large: loaded on a background thread.
    @Volatile private var libraryLoaded = false
    @Volatile private var libraryFailed = false
    /** When the engine was last warmed or used (elapsedRealtime), 0 = never. */
    @Volatile private var lastWarm = 0L

    @JvmStatic private external fun nativeWarmUp(modelPath: String)
    @JvmStatic private external fun nativeStart(modelPath: String)
    @JvmStatic private external fun nativePush(pcm: ShortArray)
    @JvmStatic private external fun nativeStop()
    @JvmStatic private external fun nativePoll(): String?
    @JvmStatic private external fun nativeLinkUp(modelPath: String)

    /** Where the Sokki app keeps the speech model (its Dictate tab downloads it). */
    fun modelFile(context: Context) =
        File(File(context.applicationContext.dataDir, "models"), MODEL_FILE)

    private fun loadLibrary(): Boolean {
        if (libraryLoaded) return true
        if (libraryFailed) return false
        synchronized(this) {
            if (libraryLoaded) return true
            try {
                System.loadLibrary("mobile_lib")
                libraryLoaded = true
            } catch (e: Throwable) {
                Log.e(TAG, "speech engine missing", e)
                libraryFailed = true
            }
        }
        return libraryLoaded
    }

    /** Keyboard opened: load and warm up the engine in the background, so
     * the first dictation is fast; after a while unused, bring it back into
     * memory (on Robin's Pixel none of the model was left after a day, and
     * the first words took 2 s longer). */
    fun prepare(context: Context) {
        val now = android.os.SystemClock.elapsedRealtime()
        val model = modelFile(context)
        if (!model.exists()) return
        // The link to the Mac, every time (cheap when it's open).
        if (libraryLoaded) Thread { nativeLinkUp(model.path) }.start()
        if (state != State.IDLE || (lastWarm != 0L && now - lastWarm < REWARM_AFTER_MS)) return
        lastWarm = now
        Thread {
            if (loadLibrary()) nativeWarmUp(model.path)
        }.start()
    }

    /** The mic key. Idle: start listening. Listening: stop (the rest is
     * transcribed). Processing: ignored, it's nearly done. */
    fun toggle(context: Context, input: () -> InputConnection?) {
        when (state) {
            State.IDLE -> start(context, input)
            State.LISTENING -> stop()
            State.PROCESSING -> {}
        }
    }

    private fun openApp(context: Context, message: String) {
        Toast.makeText(context, message, Toast.LENGTH_LONG).show()
        context.packageManager.getLaunchIntentForPackage(context.packageName)?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            it.putExtra("sokki.from_keyboard", true)
            context.startActivity(it)
        }
    }

    private fun start(context: Context, input: () -> InputConnection?) {
        if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            openApp(context, "Allow the microphone in Sokki, then tap the mic again.")
            return
        }
        val model = modelFile(context)
        if (!model.exists()) {
            val updating = File(model.parentFile, OLD_MODEL_FILE).exists()
            openApp(
                context,
                if (updating) "Sokki is getting its faster speech model (750 MB). Try again in a few minutes."
                else "Download the speech model in Sokki (Dictate tab) first.",
            )
            return
        }
        appContext = context.applicationContext
        connection = input
        typedAny = false
        composing = false
        lead = null
        typed = 0
        lastText = ""
        val mine = ++session
        state = State.LISTENING
        onState?.invoke(State.LISTENING) // right away, on the tap
        val test = testRecording(context)
        Thread { listen(mine, model.path, test) }.start()
        main.postDelayed({ poll(mine) }, 150)
    }

    /** Records right away; audio waits until the engine is loaded. */
    @SuppressLint("MissingPermission") // checked in start()
    private fun listen(mine: Int, modelPath: String, test: File?) {
        val pending = ArrayList<ShortArray>()
        var started = false
        val block = ShortArray(BLOCK)
        var filled = 0
        fun deliver(chunk: ShortArray, n: Int) {
            // Loudness for the animation.
            var sum = 0.0
            for (i in 0 until n) sum += chunk[i] * chunk[i].toDouble()
            val rms = sqrt(sum / maxOf(1, n)) / 32768.0
            // Loudness in dB, -50 dB (quiet room) .. -8 dB (loud voice) to 0..1,
            // so syllables and pauses show instead of a solid block.
            val db = 20 * kotlin.math.log10(maxOf(rms, 1e-5))
            val level = ((db + 50) / 42).coerceIn(0.0, 1.0).toFloat()
            main.post { if (session == mine) onLevel?.invoke(level) }
            // Into 200 ms blocks for the engine.
            var i = 0
            while (i < n) {
                val take = minOf(n - i, BLOCK - filled)
                System.arraycopy(chunk, i, block, filled, take)
                filled += take
                i += take
                if (filled == BLOCK) {
                    if (!started && libraryLoaded) {
                        nativeStart(modelPath)
                        started = true
                        for (p in pending) nativePush(p)
                        pending.clear()
                    }
                    if (started) nativePush(block.copyOf()) else pending.add(block.copyOf())
                    filled = 0
                }
            }
        }

        // Library not loaded yet (keyboard just opened): load it while recording.
        if (!libraryLoaded) Thread { loadLibrary() }.start()

        if (test != null) {
            playTest(test, ::deliver)
        } else {
            val min = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
            val rec = AudioRecord(
                MediaRecorder.AudioSource.VOICE_RECOGNITION, RATE,
                AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT,
                maxOf(min, BLOCK * 4),
            )
            val chunk = ShortArray(CHUNK)
            try {
                rec.startRecording()
                while (state == State.LISTENING && session == mine) {
                    val n = rec.read(chunk, 0, chunk.size)
                    if (n > 0) deliver(chunk, n)
                }
            } catch (e: Exception) {
                Log.e(TAG, "recording failed", e)
            } finally {
                try { rec.stop() } catch (_: Exception) {}
                rec.release()
            }
        }
        // Stopped: wait for the engine if it's still loading, then hand over the rest.
        while (!libraryLoaded && !libraryFailed) Thread.sleep(50)
        if (libraryFailed) {
            main.post { done(mine, "Sokki's speech engine is missing.") }
            return
        }
        if (!started) {
            nativeStart(modelPath)
            for (p in pending) nativePush(p)
        }
        if (filled > 0) nativePush(block.copyOf(filled))
        nativeStop()
    }

    /** Debug builds only: a WAV (16 kHz mono 16-bit) at
     * Android/data/<app>/files/sokki_test.wav is "spoken" instead of the
     * microphone, so the keyboard can be tested without one. */
    private fun testRecording(context: Context): File? {
        if (!helium314.keyboard.latin.BuildConfig.DEBUG) return null
        return File(context.getExternalFilesDir(null), "sokki_test.wav").takeIf { it.exists() }
    }

    private fun playTest(file: File, deliver: (ShortArray, Int) -> Unit) {
        val bytes = file.readBytes()
        val samples = ShortArray((bytes.size - 44) / 2) {
            ((bytes[44 + it * 2].toInt() and 0xff) or (bytes[45 + it * 2].toInt() shl 8)).toShort()
        }
        var i = 0
        while (state == State.LISTENING && i < samples.size) {
            val n = minOf(CHUNK, samples.size - i)
            deliver(samples.copyOfRange(i, i + n), n)
            i += n
            Thread.sleep(50)
        }
        if (state == State.LISTENING) main.post { stop() }
    }

    /** Stops listening; the rest is transcribed and typed. */
    fun stop() {
        if (state != State.LISTENING) return
        state = State.PROCESSING
        main.post { onState?.invoke(State.PROCESSING) }
    }

    /** Types finished pieces into the open field until the last one. */
    private fun poll(mine: Int) {
        if (mine != session) return
        if (libraryLoaded) {
            while (true) {
                val text = nativePoll() ?: break
                when {
                    text == DONE -> return done(mine, null)
                    text.startsWith(ERROR) -> return done(mine, text.removePrefix(ERROR))
                    text.startsWith(TEXT) -> show(text.removePrefix(TEXT), false)
                    text.startsWith(FINAL) -> show(text.removePrefix(FINAL), true)
                    text.startsWith(LIVE) -> showLive(text.removePrefix(LIVE))
                }
            }
        }
        main.postDelayed({ poll(mine) }, 40)
    }

    private fun done(mine: Int, error: String?) {
        if (mine != session) return
        session++
        // An error mid-way: keep the provisional words rather than lose them.
        if (composing) connection?.invoke()?.finishComposingText()
        composing = false
        if (error == null && typedAny) appContext?.let { countDictation(it) }
        lastWarm = android.os.SystemClock.elapsedRealtime()
        state = State.IDLE
        onState?.invoke(State.IDLE)
        if (typedAny) {
            Journal.dictated(lastText)
            onTyped?.invoke()
        }
        connection = null
        if (error != null) appContext?.let { Toast.makeText(it, error, Toast.LENGTH_LONG).show() }
    }

    /** Counted for the app's "get Sokki on your computer" card (after 20).
     * A plain file, because the app reads it from its own process. */
    private fun countDictation(context: Context) {
        val file = File(context.filesDir, "keyboard_dictations")
        val n = file.takeIf { it.exists() }?.readText()?.trim()?.toIntOrNull() ?: 0
        file.writeText((n + 1).toString())
    }

    /** Space before the first text: after a word already in the field.
     * Some fields (web pages) don't report the text before the cursor right
     * away, so later pieces of this dictation always get a space between. */
    private fun space(ic: InputConnection): String {
        if (typedAny) return " "
        return lead ?: run {
            val before = ic.getTextBeforeCursor(1, 0)
            (if (!before.isNullOrEmpty() && !before.last().isWhitespace()) " " else "").also { lead = it }
        }
    }

    /** Through the Mac: the text so far, all of it still changeable, so all
     * underlined; the final text replaces it. */
    private fun showLive(text: String) {
        if (typed > 0) return show(text, false) // never: the Mac's text is all-or-final
        val ic = connection?.invoke() ?: return
        if (text.isNotBlank()) {
            ic.setComposingText(space(ic) + text, 1)
            composing = true
        } else if (composing) {
            ic.setComposingText("", 1)
            composing = false
        }
        lastText = text
    }

    /** The dictation's text so far ([final]: all of it). Words the model
     * has moved past don't change: they're typed for good; the last word may
     * still grow, so it stays underlined until the next text. */
    private fun show(text: String, final: Boolean) {
        val ic = connection?.invoke() ?: return
        val stable = if (final) text.length else maxOf(0, text.lastIndexOf(' '))
        ic.beginBatchEdit()
        if (stable > typed) {
            if (!text.startsWith(lastText.substring(0, typed))) {
                // Never seen in tests (asr-bench counts these); the typed words stay.
                Log.w(TAG, "the model changed words already typed")
            }
            val add = text.substring(typed, stable)
            ic.commitText(if (typed == 0) space(ic) + add else add, 1) // replaces the underlined word
            typed = stable
            typedAny = true
            composing = false
        }
        val rest = text.substring(minOf(typed, text.length))
        if (rest.isNotBlank()) {
            ic.setComposingText(if (typed == 0) space(ic) + rest else rest, 1)
            composing = true
        } else if (composing) {
            ic.setComposingText("", 1)
            composing = false
        }
        ic.endBatchEdit()
        lastText = text
    }
}
