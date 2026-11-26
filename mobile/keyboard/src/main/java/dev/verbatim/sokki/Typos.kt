// Sokki: autocorrect that fixes typos and never swaps a real word (like Gboard).
//
// HeliBoard's own autocorrect also replaces correctly typed words when its
// dictionary or the sentence favours another one. Robin: Gboard "never
// swapped a word ... it only corrected small letter swaps", and "geil" must
// stay "geil". So Suggest asks this object whether the typed word is replaced,
// and by what.
//
// One score decides (2026-11-20, chosen over a stack of hand rules: on the real
// keyboard 29.9% of typos fixed instead of 21.4%, 0.6% fixed wrong instead of
// 4.9%, no correct word changed). Every word one slip from the typed one (any
// letter, the first included) that is at least CAND_ZIPF common competes with
// the typed word itself on
//     log10 P(word | previous word) + log10 P(typed | word)
// P(word | previous word): the pair table, else 0.4 x P(word) from the word
// table. P(typed | word): per-letter slip rates measured on typing37k. The typed
// word, when it is no word: log10 P = OOV. The best word replaces the typed one
// when its share of all scores is at least TAU. When no word is one slip away, common words two
// slips away compete the same way (assets/sokki/common.txt). Neighbouring keys come from the
// keyboard on screen (useKeyboard). Known words (common, compounds,
// personal) are never changed, nor the first letter of a word typed with a
// capital in mid-sentence (a name: Qwen, Ollama). tools/typo_model.py is the
// Python twin (keep the two equal); tools/score_context.py and
// tools/bench_device.py measure it.
//
// Tables: assets/sokki/words.bin (tools/gen_known_words.py) holds every word of
// wordfreq's large German and English lists with its frequency;
// assets/sokki/bigrams.bin (tools/gen_bigrams.py) how often a word follows
// another, from Wikipedia and Tatoeba sentences.
//
// Undoing a fix with backspace adds the typed word to the personal list
// (<app files>/sokki_words.txt, one word per line), and it is never changed
// again.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

import android.content.Context
import android.util.Log
import helium314.keyboard.keyboard.Keyboard
import java.io.File
import java.nio.ByteBuffer
import java.util.Locale
import java.util.concurrent.Executors

object Typos {
    private const val TAG = "SokkiTypos"
    private const val COMMON = 3.0 // zipf from which a word is known (never changed)
    private const val MIN_LEN = 3
    private const val MAX_BUCKET = 16
    // English stem + German ending (publishen, geforwarded); one-letter endings made typos words (losts)
    private val DENGLISH_ENDINGS = listOf("en", "st", "et", "te", "ten")
    private val QWERTZ = listOf("qwertzuiopü", "asdfghjklöä", "yxcvbnm")
    private val POS: Map<Char, Pair<Int, Double>> = buildMap {
        QWERTZ.forEachIndexed { r, row -> row.forEachIndexed { i, c -> put(c, r to i + 0.5 * r) } }
    }

    // The score (see the top of this file). Swept in tools/score_context.py: these keep correct
    // words unchanged on every test set; a laxer prior (-12) or weight (0.75) fixes ~6 points more
    // typos but changed real words (Ollama -> Llama).
    private const val OOV = -10.0 // log10 P of a typed non-word
    private const val TAU = 0.95 // share the best word needs
    private const val CAND_ZIPF = 2.0 // candidates are at least this common
    const val BOS = "<s>"
    private val BACKOFF = Math.log10(0.4)
    // log10 P of one slip per letter, typing37k (8.5M letters, 51k typos)
    private val SLIP_NEAR = Math.log10(0.00308 / 4.2) // a neighbouring key (one of ~4)
    private val SLIP_FAR = Math.log10(0.00118 / 21) // another key
    private val SLIP_MISSING = Math.log10(0.00171)
    private val SLIP_DOUBLE = Math.log10(0.00017) // an extra letter: doubled ...
    private val SLIP_EXTRA_NEAR = Math.log10(0.00032 / 8.4) // ... next to a neighbour ...
    private val SLIP_EXTRA_FAR = Math.log10(0.00068 / 24) // ... or any
    private val SLIP_SWAP = Math.log10(0.00033)
    private const val FIRST = -0.3 // slips on the first letter are 2-3 times rarer
    private val MATCH = Math.log10(1 - 0.0075)
    private const val LETTERS = "abcdefghijklmnopqrstuvwxyzäöü"
    private val BASE = mapOf('ä' to 'a', 'ö' to 'o', 'ü' to 'u')

    // Measured on Robin's own writing (tools/robin_eval.py, 2026-11-23): his real typos fixed
    // 46.9% -> 63.4%, nouns typed in lower case capitalised 47% -> 63%. Same values as typo_model.py.
    private val SWAP_BONUS = Math.log10(5.0) // half of Robin's typos are two swapped letters
    private const val UMLAUT = -0.2 // log10 P of the plain vowel for an umlaut (a missed long-press)
    private val TRANSLIT = Regex("ae|oe|ue")
    private val WORD = Regex("[A-Za-zÄÖÜäöüß]+")
    private val DIPHTHONG_SZ = Regex("(ei|ai|au|eu|äu|ie)ß")
    private const val SWITCH = -1.0 // log10 P(an English-only word in a German sentence)
    private const val COMPOUND_MIN = 9 // a typed non-word this long may be a compound with a slip
    private val RUNNER_UP = Math.log10(5.0) // a compound fix must lead the next one 5x
    private const val OOV_PER_LETTER = 0.25 // each letter over 4 makes a typed non-word 10^0.25 less likely
    private val GERMAN_PREFIXES = listOf("un", "ab", "an", "auf", "aus", "be", "ein", "ent", "er", "ge", "mit", "nach", "um",
        "ver", "vor", "weg", "zer", "zu", "rein", "raus", "rum", "ran", "runter", "hin", "her")
    private val GERMAN_ENDINGS = listOf("ere", "eres", "erem", "eren", "erer", "est", "end", "ens", "em", "es", "er", "en", "e",
        "st", "t", "te", "ten", "s")
    // German words a noun follows: an English word only gets a German capital after one (den Link,
    // ne Weile; not "ist easy"). After a personal pronoun the word is a verb (ich krieg, sich stelle).
    private val PRONOUNS = setOf("ich", "du", "er", "sie", "es", "wir", "ihr", "man", "sich")
    private val DETERMINERS = ("der die das den dem des ein eine einen einem einer eines kein keine keinen keinem keiner " +
        "mein meine meinen meinem meiner dein deine deinen deinem deiner sein seine seinen seinem seiner ihre ihren " +
        "ihrem ihrer unser unsere unseren unserem unserer euer eure euren eurem diese dieser dieses diesen diesem " +
        "jede jeder jedes jeden jedem alle allen viele vielen mehr mehrere mehreren welche welcher welches welchen " +
        "zum zur im am beim vom ins ans aufs für mit von zu bei nach aus über unter auf in an um durch ohne gegen " +
        "pro per via vor hinter neben zwischen ne nen nem ner").split(" ").toSet()

    // British -ise spellings of a common US -ize word are words (optimised; Robin's Mac, 26.11.2026)
    private val BRITISH = listOf("isations" to "izations", "isation" to "ization", "ising" to "izing", "isers" to "izers",
        "ised" to "ized", "ises" to "izes", "iser" to "izer", "ise" to "ize", "ysed" to "yzed", "yse" to "yze")
    // Earlier words of the text that also count for its language (typo_model.LANG_CONTEXT) and their weight. Off:
    // on Robin's writing (26.11.2026) 10, 20 or 40 words fixed 0.3 points fewer typos, no more nouns capitalised.
    private const val LANG_CONTEXT = 0
    private const val CONTEXT_WEIGHT = 1.0

    /** Text before the cursor (LatinIME's cached copy), for the text's language. */
    @JvmField var textBefore: (() -> CharSequence?)? = null

    private val io = Executors.newSingleThreadExecutor()
    // common.txt: (first letter, length) -> those words run together; the words a typo may be two slips from
    @Volatile private var near: Map<Int, String>? = null
    // The keyboard on screen: letter -> (centre x, centre y, width, height); null: the QWERTZ rows
    @Volatile private var keys: Map<Char, DoubleArray>? = null
    @Volatile private var keysOf: Any? = null
    @Volatile private var bloom: Bloom? = null
    @Volatile private var bigrams: Bloom? = null
    private val personal = HashSet<String>()
    private var personalFile: File? = null

    @JvmStatic fun init(context: Context) {
        val app = context.applicationContext
        io.execute {
            try {
                val data = app.assets.open("sokki/words.bin").use { it.readBytes() }
                val file = File(app.filesDir, "sokki_words.txt")
                val words = if (file.exists()) file.readLines().map { it.trim() }.filter { it.isNotEmpty() } else emptyList()
                synchronized(personal) {
                    personalFile = file
                    personal.addAll(words.map { fold(it) })
                }
                val pairs = app.assets.open("sokki/bigrams.bin").use { it.readBytes() }
                val common = app.assets.open("sokki/common.txt").use { it.readBytes() }.toString(Charsets.UTF_8)
                load(data, pairs, common)
                Log.i(TAG, "word table ${data.size} bytes, pairs ${pairs.size} bytes, ${words.size} personal words")
            } catch (e: Exception) {
                Log.w(TAG, "no word table, autocorrect stays off", e)
            }
        }
    }

    /** The tables from assets/sokki (words.bin, bigrams.bin, common.txt); JVM tests call it directly. */
    internal fun load(words: ByteArray, pairs: ByteArray, common: String) {
        val runs = HashMap<Int, StringBuilder>()
        for (w in common.split('\n')) if (w.isNotEmpty() && w.length < 64) runs.getOrPut(nearKey(w[0], w.length)) { StringBuilder() }.append(w)
        near = runs.mapValues { it.value.toString() }
        bigrams = Bloom.load(pairs)
        bloom = Bloom.load(words)
    }

    /** Neighbouring keys from these key positions (letter -> centre x, centre y, width, height); null: QWERTZ rows. */
    internal fun setKeys(k: Map<Char, DoubleArray>?) {
        keys = k
    }

    private fun nearKey(first: Char, length: Int) = first.code * 64 + length

    /**
     * Use the letter keys of [keyboard] (the one on screen) for neighbouring keys, so a slip is
     * judged on the layout the user types on (QWERTY, QWERTZ, AZERTY, with or without umlaut keys).
     */
    @JvmStatic fun useKeyboard(keyboard: Keyboard?) {
        if (keyboard == null || keyboard === keysOf || !keyboard.mId.element.isAlphabet) return
        val found = HashMap<Char, DoubleArray>()
        for (k in keyboard.sortedKeys) {
            val code = k.code
            if (code <= 0 || Character.isSupplementaryCodePoint(code) || !Character.isLetter(code)) continue
            val c = code.toChar().lowercaseChar()
            if (c !in found) found[c] = doubleArrayOf(k.x + k.width / 2.0, k.y + k.height / 2.0, k.width.toDouble(), k.height.toDouble())
        }
        keysOf = keyboard
        if (found.size < 20) return // no letter layout Typos knows: keep the last one
        if (keys?.let { old -> old.size == found.size && found.all { (c, v) -> old[c]?.contentEquals(v) == true } } == true) return
        keys = found
        Log.i(TAG, "keyboard keys set (${found.size} letters, ${found.keys.sorted().joinToString("")})")
    }

    /**
     * The word that replaces [typed] when it is committed, or null to leave it as typed. Also puts
     * in a German capital (die Liste, den Link) or the dictionary's spelling (Straße).
     * [candidates]: the dictionary's suggestions with their language ("de", "en", null for the
     * user's own words); their spelling wins, and a capital only comes from German. [typedLangs]:
     * which dictionaries know [typed] as it is ("de", "en", "user"). [prevWords]: the sentence's
     * words before it, nearest last, [BOS] where the sentence starts. [next]: the word after, if known.
     */
    @JvmStatic fun correction(typed: String, candidates: List<Pair<String, String?>>, typedLangs: Set<String>,
                              prevWords: List<String>, next: String?): String? {
        val table = bloom ?: return null
        if (typed.length < MIN_LEN || !typed.all { it.isLetter() } || typed.drop(1).any { it.isUpperCase() }) return null
        val t = fold(typed)
        val valid = HashSet(typedLangs)
        if (synchronized(personal) { t in personal }) valid.add("user")
        val last = prevWords.lastOrNull()
        val context by lazy { contextWords() }
        // The same word spelled as the dictionary has it: a capital (montag -> Montag) or ß. A capital
        // only from German (allen stays, not "Allen" from an English name list).
        var capPair = candidates.firstOrNull { (c, lang) ->
            c.isNotEmpty() && fold(c) == t && c != typed && c.drop(1).none { it.isUpperCase() } &&
                (lang == null || lang == "de" || c[0] == typed[0]) &&
                (c[0] == typed[0] || (typed[0].isLowerCase() && c[0].isUpperCase()))
        }
        // a capitalised word gets the dictionary's spelling too: Heisst -> Heißt
        if (capPair == null && typed[0].isUpperCase()) capPair = candidates.firstOrNull { (c, lang) ->
            c.isNotEmpty() && fold(c) == t && c[0].isLowerCase() && (lang == null || lang == "de") &&
                c.replaceFirstChar { it.uppercaseChar() } != typed
        }?.let { (c, lang) -> c.replaceFirstChar { it.uppercaseChar() } to lang }
        val cap = capPair?.first
        fun asTyped(w: String) = if (typed[0].isUpperCase()) w.replaceFirstChar { it.uppercaseChar() } else w
        // ae/oe/ue for ä/ö/ü (DIN 5007) when the typed word is neither German nor English: wuerde -> würde,
        // fuer -> für; the plain vowel stays (wurde), so nothing is guessed. Not when ue is a swapped eu,
        // the most common slip: huete may be heute, so it doesn't become hüte.
        if (valid.all { it == "user" }) {
            val spots = TRANSLIT.findAll(t).map { it.range.first }.toList()
            if (spots.isNotEmpty() && spots.none { i -> table.commonIn('d', t.substring(0, i) + "e" + t[i] + t.substring(i + 2)) }) {
                val use = spots.take(4)
                var best: String? = null
                var bestScore = 0.0
                for (mask in 1 until (1 shl use.size)) {
                    var w = t
                    for (k in use.indices.reversed()) if (mask shr k and 1 == 1) {
                        val i = use[k]
                        w = w.substring(0, i) + when (w[i]) { 'a' -> 'ä'; 'o' -> 'ö'; else -> 'ü' } + w.substring(i + 2)
                    }
                    // a common German word, else one German builds (zerdrücken, Steuerschlüssel)
                    val score = if (table.commonIn('d', w)) table.zipf(w)
                        else if (!table.commonIn('e', w) && knownGerman(table, w)) 0.0 else continue
                    if (best == null || score > bestScore) {
                        best = w
                        bestScore = score
                    }
                }
                if (best != null) return asTyped(candidates.firstOrNull { fold(it.first) == best }?.first ?: best)
            }
        }
        // ss right after ei/au/eu/äu/ie is ß when the German dictionary spells it so (weiss -> weiß,
        // ausser -> außer): after a diphthong German writes ß; dass and muss stay
        if ("ss" in t && (typed.all { it.isLowerCase() } || last == null || last == BOS)) {
            candidates.firstOrNull { (c, lang) ->
                (lang == null || lang == "de") && fold(c) == t && 'ß' in c && DIPHTHONG_SZ.containsMatchIn(c.lowercase())
            }?.let { return asTyped(it.first) }
        }
        if (valid == setOf("user") && typed.all { it.isLowerCase() } && cap != null && cap[0].isUpperCase() && capPair?.second == "de")
            valid.clear() // the history learned a noun typed in lower case: the dictionary has its capital
        fun truecase(): String? {
            // after zum/beim/vom a German verb is a noun: zum Auswählen (not "zum ersten Mal")
            if (typed.all { it.isLowerCase() } && last != null && fold(last) in setOf("zum", "beim", "vom") && infinitive(table, t))
                return typed.replaceFirstChar { it.uppercaseChar() }
            if (!typed.all { it.isLowerCase() } || last == null || last == BOS || !table.noun(t)) return null
            if (fold(last) in PRONOUNS || sentenceLang(table, prevWords, next, context) != "de") return null
            // an English word only after a German article, preposition or inflected adjective (den Link,
            // kleines Update), with no English word after
            if (table.commonIn('e', t) && (!(fold(last) in DETERMINERS || adjective(table, last)) || lean(table, next) == 'e'))
                return null
            if (umlautTwin(table, t, table.zipf(t))) return null // hort is more likely hört than Hort
            return if (cap != null && cap[0].isUpperCase()) cap else typed.replaceFirstChar { it.uppercaseChar() }
        }
        // A word only English (or the user's history) has, whose umlaut twin is 100x more common German,
        // is that twin in a German sentence: fur -> für, uber -> über (a missed long-press)
        if (valid.isNotEmpty() && valid.all { it == "en" || it == "user" } && sentenceLang(table, prevWords, next, context) == "de" &&
            lean(table, typed) != 'd') {
            val twin = umlautForm(table, t)
            if (twin != null && table.zipf(twin) >= table.zipf(t) + 2) {
                val spelled = candidates.firstOrNull { fold(it.first) == twin }?.first ?: twin
                return if (typed[0].isUpperCase()) spelled.replaceFirstChar { it.uppercaseChar() } else spelled
            }
        }
        // In a German sentence an English-only word one slip from a German word competes with it on the
        // pair table, paying a language switch as in bilingual decoders: mir bite die -> mir bitte die
        if (valid == setOf("en") && lean(table, typed) == 'e' && last != null && last != BOS &&
            sentenceLang(table, prevWords, next, context) == "de") {
            englishInGerman(table, t, fold(last))?.let { word ->
                val spelled = candidates.firstOrNull { fold(it.first) == word }?.first ?: word
                return if (typed[0].isUpperCase()) spelled.replaceFirstChar { it.uppercaseChar() } else spelled
            }
        }
        if (valid.isNotEmpty()) {
            // Words the keyboard knows are never changed, only capitalised; in German a noun gets its
            // capital even if English has the lower-case word (den Link).
            truecase()?.let { return it }
            if (cap != null && valid == setOf("en") && cap[0] != typed[0] && sentenceLang(table, prevWords, next, context) == "de" &&
                last != null && last != BOS && fold(last) in DETERMINERS && lean(table, next) != 'e') return cap
            return null
        }
        if (cap == null && known(table, typed)) return truecase()
        val pairs = bigrams
        // hash states of "<b>:<prev> " for each bucket: a pair lookup then only hashes the word
        val prevStates = if (last == null || pairs == null) null else {
            val tail = ((if (last == BOS) BOS else fold(last)) + " ").toByteArray(Charsets.UTF_8)
            LongArray(MAX_BUCKET + 1) { b -> Bloom.hash(Bloom.PREFIX[b], tail) }
        }
        fun lm(bytes: ByteArray, z: Double): Double {
            if (prevStates != null) {
                for (b in MAX_BUCKET downTo 1) if (pairs!!.containsHash(Bloom.hash(prevStates[b], bytes))) return b / 2.0 - 8 + 0.25
                return BACKOFF + z - 9
            }
            return z - 9
        }
        val tBytes = t.toByteArray(Charsets.UTF_8)
        val zt = table.zipf(tBytes)
        // a listed word the dictionaries don't know competes like an unknown one
        // a longer typed non-word is less likely meant (gespeicehrt)
        val oov = OOV - OOV_PER_LETTER * maxOf(0, t.length - 4)
        var best = (if (zt == 0.0 || cap == null) oov else lm(tBytes, zt)) + t.length * MATCH
        var bestWord = t
        val scores = ArrayList<Double>(32)
        scores.add(best)
        // a non-word missing two or more umlauts: Geschaftsfuhrer -> Geschäftsführer
        umlautForm(table, t)?.let { u ->
            val bytes = u.toByteArray(Charsets.UTF_8)
            if (t.indices.count { t[it] != u[it] } >= 2 && table.containsHash(Bloom.hash(Bloom.CANDIDATE, bytes))) {
                val score = lm(bytes, table.zipf(bytes)) + channel(t, u)
                scores.add(score)
                if (score > best || (score == best && u > bestWord)) {
                    best = score
                    bestWord = u
                }
            }
        }
        // A capital in mid-sentence: a name or a noun; its first letter stays (Qwen is not "wen").
        val keepFirst = typed[0].isUpperCase() && last != BOS
        for (c in oneSlip(t)) {
            if (keepFirst && (BASE[c[0]] ?: c[0]) != t[0]) continue
            val bytes = c.toByteArray(Charsets.UTF_8)
            // one probe for most strings; a false hit must also pass the zipf probes
            if (!table.containsHash(Bloom.hash(Bloom.CANDIDATE, bytes))) continue
            val zc = table.zipf(bytes)
            if (zc < CAND_ZIPF) continue
            val score = lm(bytes, zc) + channel(t, c)
            scores.add(score)
            if (score > best || (score == best && c > bestWord)) {
                best = score
                bestWord = c
            }
        }
        // No word one slip away: common words two slips away compete (funtkionirt -> funktioniert).
        // Robin's Mac typing, 26.11.2026: long words with a swap and a second slip were left as typed.
        var two: HashSet<String>? = null
        val runs = near
        if (scores.size == 1 && runs != null) {
            val bag = IntArray(31)
            for (ch in t) bag[slot(ch)]++
            var slips: Set<String>? = null
            for (n in maxOf(1, t.length - 2)..t.length + 2) {
                val run = runs[nearKey(t[0], n)] ?: continue
                var i = 0
                while (i < run.length) {
                    val at = i
                    i += n
                    if (bagDistance(bag, run, at, n) > 2) continue
                    val c = run.substring(at, at + n)
                    if ((keepFirst && (BASE[c[0]] ?: c[0]) != t[0]) || !table.candidate(c)) continue
                    if (slips == null) slips = oneSlip(t)
                    val bytes = c.toByteArray(Charsets.UTF_8)
                    val zc = table.zipf(bytes)
                    if (c in slips || zc < COMMON) continue
                    val score = lm(bytes, zc) + channel(t, c)
                    (two ?: HashSet<String>().also { two = it }).add(c)
                    scores.add(score)
                    if (score > best || (score == best && c > bestWord)) {
                        best = score
                        bestWord = c
                    }
                }
            }
        }
        val sure = 1.0 / scores.sumOf { Math.pow(10.0, it - best) } >= TAU
        if (bestWord != t && sure) {
            var spelled = candidates.firstOrNull { fold(it.first) == bestWord }?.first ?: bestWord
            if (two?.contains(bestWord) == true) {
                // the spelling of the sentence's language: million in English, Million in German
                val lang = sentenceLang(table, prevWords, next, context)
                spelled = candidates.firstOrNull { fold(it.first) == bestWord && it.second == lang }?.first ?: spelled
                if (spelled == bestWord && lang == "de" && table.noun(bestWord)) spelled = bestWord.replaceFirstChar { it.uppercaseChar() }
            }
            return if (typed[0].isUpperCase()) spelled.replaceFirstChar { it.uppercaseChar() } else spelled
        }
        // a long non-word one slip from a German compound gets it (Rchnungsthema -> Rechnungsthema)
        if (bestWord == t && zt == 0.0 && t.length >= COMPOUND_MIN && sentenceLang(table, prevWords, next, context) != "en") {
            compoundFix(table, t)?.let { fix ->
                return if (typed[0].isUpperCase()) fix.replaceFirstChar { it.uppercaseChar() } else fix
            }
        }
        truecase()?.let { return it }
        if (cap == null) return null
        if (cap[0] != typed[0] && (sentenceLang(table, prevWords, next, context) == "en" ||
                (last != null && last != BOS && fold(last) in PRONOUNS))) return null
        return cap
    }

    /** 'd' / 'e' for a word only common in German / English, else null. */
    private fun lean(table: Bloom, w: String?): Char? {
        if (w.isNullOrEmpty() || w == BOS) return null
        val t = fold(w)
        val d = table.commonIn('d', t)
        val e = table.commonIn('e', t)
        return if (d && !e) 'd' else if (e && !d) 'e' else null
    }

    /**
     * "de" or "en" for the sentence so far: the last three words count 1, 1/2, 1/4 (German +1,
     * English -1 for a word only common in that language), the next word 1/2. German wins ties:
     * it is the user's first language.
     */
    private fun sentenceLang(table: Bloom, prevWords: List<String>, next: String?, context: List<String>): String {
        var score = 0.0
        var weight = 1.0
        for (w in prevWords.takeLast(3).asReversed()) {
            score += weight * when (lean(table, w)) { 'd' -> 1; 'e' -> -1; else -> 0 }
            weight /= 2
        }
        score += 0.5 * when (lean(table, next)) { 'd' -> 1; 'e' -> -1; else -> 0 }
        if (LANG_CONTEXT > 0 && context.isNotEmpty()) {
            val ws = context.takeLast(LANG_CONTEXT)
            score += CONTEXT_WEIGHT * ws.sumOf { when (lean(table, it)) { 'd' -> 1.0; 'e' -> -1.0; else -> 0.0 } } / ws.size
        }
        return if (score < 0) "en" else "de"
    }

    /**
     * An inflected German adjective before a noun: klein-es, neu-er, sinnvoll-en (its stem a German
     * word, as Hunspell's affix rules check), German only, neither a noun (Ergebnissen) nor a verb
     * (zieh-e: zieht; find-en: findet; soll-te: sollen; können). Not immer, trotzdem, wieder: their
     * stems are no words, or names (Wied).
     */
    private fun adjective(table: Bloom, w: String): Boolean {
        val a = fold(w)
        if (lean(table, a) != 'd' || table.noun(a) || infinitive(table, a)) return false
        if ((a.endsWith("te") || a.endsWith("ten")) && infinitive(table, a.substring(0, a.lastIndexOf("te")) + "en")) return false
        for (end in arrayOf("em", "en", "er", "es", "e")) {
            if (a.endsWith(end)) {
                val stem = a.dropLast(end.length)
                return stem.length >= 3 && table.commonIn('d', stem) && !table.noun(stem) &&
                    !table.commonIn('d', stem + "t") && !table.commonIn('d', stem + "et")
            }
        }
        return false
    }

    /** A German infinitive whose stem conjugates (auswählen: auswählt; ändern: ändert); not "ersten". */
    private fun infinitive(table: Bloom, t: String): Boolean {
        if (!table.commonIn('d', t)) return false
        for ((end, stemEnd) in listOf("ern" to "ert", "eln" to "elt", "en" to "t"))
            if (t.endsWith(end) && table.commonIn('d', t.dropLast(end.length) + stemEnd)) return true
        return false
    }

    /** The common German word that is folded [t] with ä/ö/ü for a/o/u (fur -> für), or null. */
    private fun umlautForm(table: Bloom, t: String): String? {
        val spots = t.indices.filter { t[it] in "aou" }.take(4)
        var best: String? = null
        for (mask in 1 until (1 shl spots.size)) {
            val w = t.toCharArray()
            spots.forEachIndexed { k, i -> if (mask shr k and 1 == 1) w[i] = when (w[i]) { 'a' -> 'ä'; 'o' -> 'ö'; else -> 'ü' } }
            val s = String(w)
            if (table.commonIn('d', s) && (best == null || table.zipf(s) > table.zipf(best))) best = s
        }
        return best
    }

    /** A form of folded [t] with ä/ö/ü for a/o/u that is at least 10x more common (konnen -> können). */
    private fun umlautTwin(table: Bloom, t: String, z: Double): Boolean {
        val spots = t.indices.filter { t[it] in "aou" }.take(4)
        for (mask in 1 until (1 shl spots.size)) {
            val w = t.toCharArray()
            spots.forEachIndexed { k, i -> if (mask shr k and 1 == 1) w[i] = when (w[i]) { 'a' -> 'ä'; 'o' -> 'ö'; else -> 'ü' } }
            if (table.zipf(String(w)) >= z + 1) return true
        }
        return false
    }

    /**
     * A German verb form without its ending: rechne(n), sammle (sammeln), and stems that also
     * conjugate: informier(en/t), leg(en/t); with a clitic s (machs = mach es). Not "zieg"
     * (ziegen are goats: no "ziegt").
     */
    private fun verbStem(table: Bloom, t: String): Boolean {
        for (stem in listOf(t, if (t.endsWith("s")) t.dropLast(1) else null)) {
            if (stem == null || stem.length < 3) continue
            if (stem.endsWith("e") && table.commonIn('d', stem + "n")) return true
            if ((stem.endsWith("le") || stem.endsWith("re")) &&
                table.commonIn('d', stem.dropLast(2) + "e" + stem[stem.length - 2] + "n")) return true
            if (table.commonIn('d', stem + "en") && table.commonIn('d', stem + "t")) return true
        }
        return false
    }

    /**
     * A German word built from a common one by inflection or a prefix, as Hunspell's affix rules
     * and TongueSwitcher do it: fokussier-st, einzigartig-ere, initial-em, generierung-s,
     * un-qualifiziert, ab-zu-klopfen. The remainder must be a common German word (or its verb).
     */
    private fun inflected(table: Bloom, t: String): Boolean {
        fun common(w: String) = w.length >= 4 && (table.commonIn('d', w) || table.commonIn('d', w + "en") ||
            table.commonIn('d', w + "n") || (table.zipf(w) >= 2.5 && !table.commonIn('e', w)))
        val stems = mutableListOf(t)
        for (pre in GERMAN_PREFIXES) {
            if (t.startsWith(pre) && t.length - pre.length >= 4) {
                val rest = t.substring(pre.length)
                stems.add(if (rest.startsWith("zu") && rest.length >= 6) rest.substring(2) else rest)
                stems.add(rest)
            }
        }
        for (w in stems) {
            if (w != t && common(w)) return true
            for (end in GERMAN_ENDINGS) if (w.endsWith(end) && common(w.dropLast(end.length))) return true
        }
        return false
    }

    /**
     * The German compound one slip from folded [t], split where one part is a German word as typed
     * and the other, a word no list has, is one slip from one (bchhaltungs|thema, stuer|ergebnisse),
     * or null when none or two compete; as Hunspell's and LanguageTool's compound suggestions do.
     * The first part may end in the linking s (unternehmen-s|profil).
     */
    private fun compoundFix(table: Bloom, t: String): String? {
        fun common(w: String, left: Boolean) = w.length >= 4 && (table.commonIn('d', w) ||
            (left && w.endsWith("s") && w.length >= 5 && table.commonIn('d', w.dropLast(1))))
        val found = HashMap<String, Double>()
        for (i in 4 until t.length - 3) {
            val a = t.substring(0, i)
            val b = t.substring(i)
            for (left in booleanArrayOf(true, false)) {
                val fixed = if (left) b else a
                val typedPart = if (left) a else b
                // the slipped part is no listed word itself (farb|kodiert, prio|liste, wort|blob stay)
                if (!common(fixed, !left) || table.candidate(typedPart)) continue
                for (c in oneSlip(typedPart)) {
                    if (c.length < 4 || !common(c, left)) continue
                    val w = if (left) c + b else a + c
                    val s = channel(typedPart, c) + table.zipf(if (table.commonIn('d', c)) c else c.removeSuffix("s"))
                    if (s > (found[w] ?: -99.0)) found[w] = s
                }
            }
        }
        if (found.isEmpty()) return null
        val ranked = found.entries.sortedByDescending { it.value }
        if (ranked.size > 1 && ranked[0].value - ranked[1].value < RUNNER_UP) return null
        return ranked[0].key
    }

    /** log10 P([w] | [prev]) from the pair table, as [correction] scores it; both folded. */
    private fun pairLm(w: String, z: Double, prev: String): Double {
        val pairs = bigrams ?: return z - 9
        for (b in MAX_BUCKET downTo 1) if (pairs.contains("$b:$prev $w")) return b / 2.0 - 8 + 0.25
        return BACKOFF + z - 9
    }

    /** The German word one slip from English-only [t] that beats it after [prev] (bite -> bitte), or null. */
    private fun englishInGerman(table: Bloom, t: String, prev: String): String? {
        val zt = table.zipf(t)
        var best = pairLm(t, zt, prev) + t.length * MATCH + SWITCH
        var bestWord = t
        val scores = arrayListOf(best)
        for (c in oneSlip(t)) {
            if (!table.commonIn('d', c) || table.commonIn('e', c)) continue
            val zc = table.zipf(c)
            if (zc < zt + 1.0) continue // the German word is 10x more common
            val s = pairLm(c, zc, prev) + channel(t, c)
            scores.add(s)
            if (s > best || (s == best && c > bestWord)) {
                best = s
                bestWord = c
            }
        }
        if (bestWord == t || 1.0 / scores.sumOf { Math.pow(10.0, it - best) } < TAU) return null
        return bestWord
    }

    /** Every string one slip from folded [t]: a letter extra, missing, changed, or two swapped. */
    private fun oneSlip(t: String): Set<String> {
        val out = HashSet<String>(64 * (t.length + 1))
        for (i in 0..t.length) for (ch in LETTERS) out.add(t.substring(0, i) + ch + t.substring(i))
        for (i in t.indices) {
            out.add(t.removeRange(i, i + 1))
            if (i + 1 < t.length) out.add(t.substring(0, i) + t[i + 1] + t[i] + t.substring(i + 2))
            for (ch in LETTERS) out.add(t.substring(0, i) + ch + t.substring(i + 1))
        }
        out.remove(t)
        return out
    }

    /** log10 P(typed | word), both folded: the likeliest slips (weighted Damerau-Levenshtein). */
    private fun channel(typed: String, word: String): Double {
        val n = word.length
        val m = typed.length
        val d = Array(n + 1) { DoubleArray(m + 1) { -1e9 } }
        d[0][0] = 0.0
        for (i in 0..n) for (j in 0..m) {
            if (i == 0 && j == 0) continue
            val first = if (i <= 1) FIRST else 0.0
            var best = -1e9
            if (i > 0 && j > 0) {
                best = if (word[i - 1] == typed[j - 1]) d[i - 1][j - 1] + MATCH
                else if (BASE[word[i - 1]] == typed[j - 1]) d[i - 1][j - 1] + UMLAUT
                else d[i - 1][j - 1] + (if (neighbours(typed[j - 1], word[i - 1])) SLIP_NEAR else SLIP_FAR) + first
            }
            if (i > 0) best = maxOf(best, d[i - 1][j] + SLIP_MISSING + first)
            if (j > 0) {
                val ch = typed[j - 1]
                val around = word.substring(maxOf(0, i - 1), minOf(n, i + 1))
                val kind = if (ch in around) SLIP_DOUBLE else if (around.any { neighbours(ch, it) }) SLIP_EXTRA_NEAR else SLIP_EXTRA_FAR
                best = maxOf(best, d[i][j - 1] + kind + (if (i == 0) FIRST else 0.0))
            }
            if (i > 1 && j > 1 && word[i - 1] == typed[j - 2] && word[i - 2] == typed[j - 1])
                best = maxOf(best, d[i - 2][j - 2] + SLIP_SWAP + SWAP_BONUS + (if (i == 2) FIRST else 0.0))
            d[i][j] = best
        }
        return d[n][m]
    }

    /**
     * A letter that slipped across the space: ("th", "ecar") -> ("the", "car"), or
     * null. Only when [typed] is no word, both new words are common, the new first
     * word is more common than [prev] (not "a sking" -> "as king"), and neither word
     * has capitals after its first letter (LLM, MCP).
     */
    @JvmStatic fun spaceFix(prev: String, typed: String): Array<String>? {
        val table = bloom ?: return null
        if (prev.isEmpty() || !prev.all { it.isLetter() } || !typed.all { it.isLetter() } || typed.length < 3) return null
        if (known(table, typed)) return null
        if ((prev.drop(1) + typed.drop(1)).any { it.isUpperCase() }) return null
        val a = prev + typed[0]
        val b = typed.substring(1)
        val za = table.zipf(fold(a))
        return if (za >= COMMON && table.zipf(fold(b)) >= COMMON && za > table.zipf(fold(prev))) arrayOf(a, b) else null
    }

    private fun known(table: Bloom, w: String): Boolean {
        if (synchronized(personal) { fold(w) in personal }) return true
        return knownGerman(table, w)
    }

    /** [known] without the user's own words: common, or a word German builds from common ones. */
    private fun knownGerman(table: Bloom, w: String): Boolean {
        val t = fold(w)
        val z = table.zipf(t)
        if (umlautTwin(table, t, z)) return false // konnen, uberlegt: web forms of umlaut words
        if (z >= COMMON || british(table, t)) return true
        return builtWord(table, t) || verbStem(table, t) || inflected(table, t)
    }

    /** Folded [t] is the British spelling of a common English -ize/-yze word (optimised, analysed). */
    private fun british(table: Bloom, t: String): Boolean =
        BRITISH.any { (gb, us) -> t.endsWith(gb) && t.length - gb.length >= 3 && table.commonIn('e', t.dropLast(gb.length) + us) }

    /** The text's words before the one being typed, nearest last (the current message: after the last line break). */
    private fun contextWords(): List<String> {
        if (LANG_CONTEXT == 0) return emptyList()
        val text = try { textBefore?.invoke()?.toString() } catch (e: Exception) { null } ?: return emptyList()
        val line = text.substring(text.lastIndexOf('\n') + 1)
        val words = WORD.findAll(line).map { it.value }.toMutableList()
        if (words.isNotEmpty() && line.last().isLetter()) words.removeAt(words.size - 1) // the word being typed
        return words.takeLast(LANG_CONTEXT)
    }

    /** Both tables are loaded (debug: SokkiBench waits for it). */
    @JvmStatic fun ready() = bloom != null && bigrams != null

    /** The user undid a fix: keep [word] as typed from now on. */
    @JvmStatic fun learn(word: String) {
        val w = fold(word.trim())
        if (w.isEmpty()) return
        val file = synchronized(personal) { if (personal.add(w)) personalFile else null } ?: return
        io.execute {
            try {
                file.appendText(w + "\n")
            } catch (e: Exception) {
                Log.w(TAG, "can't save personal word", e)
            }
        }
    }

    /** Lower case, ß -> ss, as wordfreq stores words. */
    private fun fold(w: String) = w.lowercase(Locale.ROOT).replace("ß", "ss")

    private fun neighbours(a: Char, b: Char): Boolean {
        val k = keys
        if (k == null) {
            if ((a == 'y' && b == 'z') || (a == 'z' && b == 'y')) return true // QWERTY habit on QWERTZ
            val (r1, c1) = POS[a] ?: return false
            val (r2, c2) = POS[b] ?: return false
            return Math.abs(r1 - r2) <= 1 && Math.abs(c1 - c2) <= 1.0
        }
        val p = k[a] ?: return false
        val q = k[b] ?: return false
        if ((a == 'y' && b == 'z') || (a == 'z' && b == 'y')) {
            val z = k['z']
            val y = k['y']
            if (z != null && y != null && z[1] < y[1]) return true // QWERTY habit on QWERTZ (z in the top row)
        }
        val dx = Math.abs(p[0] - q[0])
        val dy = Math.abs(p[1] - q[1])
        val w = (p[2] + q[2]) / 2
        val h = (p[3] + q[3]) / 2
        if (dy < minOf(p[3], q[3]) / 2) return dx <= w * 1.05 // the same row: the key beside it
        // the row above or below: keys that overlap it sideways (not one diagonally past its corner)
        return dy <= h * 1.05 && dx < w * 0.95
    }

    /** a-z, ä ö ü ß; anything else shares one slot. */
    private fun slot(c: Char) = when (c) {
        in 'a'..'z' -> c - 'a'
        'ä' -> 26
        'ö' -> 27
        'ü' -> 28
        'ß' -> 29
        else -> 30
    }

    /** A lower bound on the slips between [bag]'s word and run[at, at + n): the letters one has more of. */
    private fun bagDistance(bag: IntArray, run: String, at: Int, n: Int): Int {
        val d = bag.copyOf()
        for (i in at until at + n) d[slot(run[i])]--
        var more = 0
        var fewer = 0
        for (x in d) if (x > 0) more += x else fewer -= x
        return maxOf(more, fewer)
    }

    /** A German compound of two common words, or an English stem with a German ending. */
    private fun builtWord(table: Bloom, w: String): Boolean {
        for (suf in DENGLISH_ENDINGS) {
            if (w.endsWith(suf) && w.length - suf.length >= 3 && table.commonIn('e', w.dropLast(suf.length))) return true
        }
        if (w.startsWith("ge") && (table.commonIn('e', w.drop(2)) || (w.endsWith("t") && table.commonIn('e', w.drop(2).dropLast(1)))))
            return true
        for (i in 3 until w.length - 2) {
            val a = w.substring(0, i)
            val b = w.substring(i)
            if (minOf(a.length, b.length) < 4 && maxOf(a.length, b.length) < 5) continue // vor+lagern, not han+hing
            if (table.commonIn('d', b) && (table.commonIn('d', a) || (a.endsWith("s") && table.commonIn('d', a.dropLast(1)))))
                return true
        }
        return false
    }

    /** words.bin: "SKW1", k (int), m (long), then m bits. Keys as in gen_known_words.py. */
    private class Bloom(private val k: Int, private val m: Long, private val bits: ByteArray) { // bits: the whole file
        fun contains(key: String) = containsHash(hash(OFFSET, key.toByteArray(Charsets.UTF_8)))

        /** [h1]: the FNV-1a hash of the key ([hash]). */
        fun containsHash(h1: Long): Boolean {
            var z = h1 + 0x9E3779B97F4A7C15uL.toLong()
            z = (z xor (z ushr 30)) * 0xBF58476D1CE4E5B9uL.toLong()
            z = (z xor (z ushr 27)) * 0x94D049BB133111EBuL.toLong()
            val h2 = (z xor (z ushr 31)) or 1L
            for (i in 0 until k) {
                val idx = java.lang.Long.remainderUnsigned(h1 + i * h2, m)
                if (bits[HEADER + (idx ushr 3).toInt()].toInt() and (1 shl (idx and 7).toInt()) == 0) return false
            }
            return true
        }

        /** Zipf of a folded word in half steps (bucket middle); 0 if unlisted. */
        fun zipf(w: String) = zipf(w.toByteArray(Charsets.UTF_8))

        /** The same for the word's UTF-8 bytes: keys "<b>:<word>" hashed on from "<b>:". */
        fun zipf(word: ByteArray): Double {
            for (b in MAX_BUCKET downTo 1) if (containsHash(hash(PREFIX[b], word))) return b / 2.0 + 0.25
            return 0.0
        }

        fun commonIn(lang: Char, w: String) = contains("$lang:$w")

        /** The word list has this folded word ("w:" key, zipf 2+): it may replace a typed word. */
        fun candidate(w: String) = containsHash(hash(CANDIDATE, w.toByteArray(Charsets.UTF_8)))

        /** German text writes this folded word with a capital (90%+ of its uses mid-sentence). */
        fun noun(w: String) = contains("C:$w")

        companion object {
            const val HEADER = 16
            private val OFFSET = 0xCBF29CE484222325uL.toLong()

            /** FNV-1a 64, continued from [state] over [bytes]. */
            fun hash(state: Long, bytes: ByteArray): Long {
                var h = state
                for (b in bytes) h = (h xor (b.toLong() and 0xff)) * 0x100000001B3L
                return h
            }

            /** The hash state after "w:": words that may be an autocorrect candidate. */
            val CANDIDATE = hash(OFFSET, "w:".toByteArray(Charsets.US_ASCII))

            /** The hash state after "<b>:", for b = 0..MAX_BUCKET. */
            val PREFIX = LongArray(MAX_BUCKET + 1) { b -> hash(OFFSET, "$b:".toByteArray(Charsets.US_ASCII)) }

            fun load(data: ByteArray): Bloom {
                val buf = ByteBuffer.wrap(data)
                val magic = ByteArray(4).also { buf.get(it) }
                require(String(magic, Charsets.US_ASCII) == "SKW1") { "not a word table" }
                val k = buf.getInt()
                val m = buf.getLong()
                require(data.size - HEADER >= m / 8) { "word table cut short" }
                return Bloom(k, m, data)
            }
        }
    }
}
