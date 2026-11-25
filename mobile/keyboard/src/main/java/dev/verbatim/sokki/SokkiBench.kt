// SPDX-License-Identifier: GPL-3.0-only
package dev.verbatim.sokki

import android.util.Log
import helium314.keyboard.keyboard.Keyboard
import helium314.keyboard.keyboard.KeyboardSwitcher
import helium314.keyboard.event.Event
import helium314.keyboard.latin.common.CoordinateUtils
import helium314.keyboard.latin.BuildConfig
import helium314.keyboard.latin.NgramContext
import helium314.keyboard.latin.Suggest
import helium314.keyboard.latin.SuggestedWords
import helium314.keyboard.latin.SuggestedWords.SuggestedWordInfo
import helium314.keyboard.latin.WordComposer
import helium314.keyboard.latin.common.StringUtils
import helium314.keyboard.latin.settings.Defaults
import helium314.keyboard.latin.settings.Settings
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import kotlin.concurrent.thread

/**
 * Debug builds only: replays typed words through the keyboard's own decoder and autocorrect, to
 * score them on the device and to time them (mobile/keyboard/tools/bench_device.py).
 *
 * Input <files>/sokki_bench.jsonl, one {"typed", "prev": [words before it, reading order], "start":
 * true if the sentence starts with prev} per line. Runs once when the keyboard opens and the file
 * exists (renamed to sokki_bench.running), writing sokki_bench-out.jsonl:
 * {"typed", "auto": the word autocorrect puts in or null, "native": HeliBoard's own pick or null,
 *  "inDict", "cands": [[word, score, dictionary], ...], "us", "usDecode", "usTypos" (Typos'
 *  score)} (microseconds).
 */
object SokkiBench {
    private const val TAG = "SokkiBench"

    @JvmField @Volatile var on = false
    // written by Suggest while on
    @JvmField var native: String? = null
    @JvmField var typedLangs: Set<String> = emptySet()
    @JvmField var decodeNanos = 0L
    @JvmField var typosNanos = 0L

    @JvmStatic fun maybeRun(dir: File, suggest: Suggest) {
        if (!BuildConfig.DEBUG || on) return
        val input = File(dir, "sokki_bench.jsonl")
        if (!input.exists()) return
        val running = File(dir, "sokki_bench.running")
        if (!input.renameTo(running)) return
        on = true
        thread(name = TAG) {
            try {
                // a keyboard that just started may not have its layout yet
                var keyboard = KeyboardSwitcher.getInstance().keyboard
                for (i in 0 until 100) {
                    if (keyboard != null) break
                    Thread.sleep(200)
                    keyboard = KeyboardSwitcher.getInstance().keyboard
                }
                if (keyboard == null) throw IllegalStateException("no keyboard")
                for (i in 0 until 100) if (Typos.ready()) break else Thread.sleep(200)
                val n = run(suggest, keyboard, running, File(dir, "sokki_bench-out.jsonl"))
                Log.i(TAG, "done: $n words")
            } catch (e: Exception) {
                Log.e(TAG, "failed", e)
            } finally {
                on = false
            }
        }
    }

    private fun context(prev: List<String>, start: Boolean): NgramContext {
        val infos = prev.takeLast(3).reversed().map { NgramContext.WordInfo(it) }.toMutableList()
        if (start && infos.size < 3) infos.add(NgramContext.WordInfo.BEGINNING_OF_SENTENCE_WORD_INFO)
        return if (infos.isEmpty()) NgramContext.EMPTY_PREV_WORDS_INFO else NgramContext(*infos.toTypedArray())
    }

    private fun run(suggest: Suggest, keyboard: Keyboard, input: File, output: File): Int {
        val settings = Settings.getValues()
        // HeliBoard's own threshold at its default confidence: the field the keyboard opened in
        // may have autocorrect off, which sets an unreachable threshold
        val threshold = 0.5f - 0.5f * Math.pow(Defaults.PREF_AUTO_CORRECT_CONFIDENCE.toDouble(), 0.33).toFloat()
        Log.i(TAG, "threshold $threshold (field: ${settings.mAutoCorrectionThreshold})")
        suggest.setAutoCorrectionThreshold(threshold)
        var n = 0
        output.bufferedWriter().use { out ->
            input.forEachLine { line ->
                val item = JSONObject(line)
                val typed = item.getString("typed")
                val prev = item.optJSONArray("prev")?.let { a -> List(a.length()) { a.getString(it) } }.orEmpty()
                val codePoints = StringUtils.toCodePointArray(typed)
                // typed key by key, as when typing (setComposingWord would mark the word resumed: no autocorrect)
                val coordinates = keyboard.getCoordinates(codePoints)
                val composer = WordComposer()
                for (i in codePoints.indices) composer.applyProcessedEvent(composer.processEvent(
                    Event.createEventForCodePointFromAlreadyTypedText(codePoints[i],
                        CoordinateUtils.xFromArray(coordinates, i), CoordinateUtils.yFromArray(coordinates, i))))
                native = null
                typedLangs = emptySet()
                decodeNanos = 0
                typosNanos = 0
                val t0 = System.nanoTime()
                val words = suggest.getSuggestedWords(composer, context(prev, item.optBoolean("start", false)), keyboard,
                    settings.mSettingsValuesForSuggestion, true, SuggestedWords.INPUT_STYLE_TYPING, 0)
                val total = System.nanoTime() - t0
                val cands = JSONArray()
                for (i in 0 until words.size()) {
                    val info = words.getInfo(i)
                    if (info.isKindOf(SuggestedWordInfo.KIND_TYPED) || cands.length() >= 10) continue
                    cands.put(JSONArray().put(info.mWord).put(info.mScore)
                        .put("${info.mSourceDict.mDictType}:${info.mSourceDict.mLocale}"))
                }
                out.write(JSONObject()
                    .put("typed", typed)
                    .put("auto", if (words.mWillAutoCorrect) words.getWord(SuggestedWords.INDEX_OF_AUTO_CORRECTION) else JSONObject.NULL)
                    .put("native", native ?: JSONObject.NULL)
                    .put("inDict", words.mTypedWordValid)
                    .put("typedLangs", JSONArray(typedLangs.toList()))
                    .put("cands", cands)
                    .put("us", total / 1000)
                    .put("usDecode", decodeNanos / 1000)
                    .put("usTypos", typosNanos / 1000)
                    .toString())
                out.write("\n")
                n++
            }
        }
        suggest.setAutoCorrectionThreshold(settings.mAutoCorrectionThreshold)
        return n
    }
}
