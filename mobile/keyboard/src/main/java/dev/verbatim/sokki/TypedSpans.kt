// Sokki: which parts of a field's text were typed key by key, for the
// journal's "typed" ranges (the Mac's word learning reads only those).
// Each change of the field is a diff against the text before; its inserted
// part is typed or not, and earlier parts shift with later edits.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

class TypedSpans {
    /** Sorted, non-overlapping [start, end) UTF-16 ranges of typed text. */
    private val ranges = ArrayList<IntArray>()

    fun reset() = ranges.clear()

    /** The field went from [old] to [new]; the inserted part was [typed]. */
    fun apply(old: String, new: String, typed: Boolean) {
        var p = 0
        while (p < old.length && p < new.length && old[p] == new[p]) p++
        var s = 0
        while (s < old.length - p && s < new.length - p && old[old.length - 1 - s] == new[new.length - 1 - s]) s++
        val delEnd = old.length - s
        val insEnd = new.length - s
        val delta = (insEnd - p) - (delEnd - p)
        val next = ArrayList<IntArray>()
        for (r in ranges) {
            if (r[1] <= p) next.add(r)
            else if (r[0] >= delEnd) next.add(intArrayOf(r[0] + delta, r[1] + delta))
            else {
                if (r[0] < p) next.add(intArrayOf(r[0], p))
                if (r[1] > delEnd) next.add(intArrayOf(delEnd + delta, r[1] + delta))
            }
        }
        if (typed && insEnd > p) next.add(intArrayOf(p, insEnd))
        next.sortBy { it[0] }
        ranges.clear()
        for (r in next) {
            val lastR = ranges.lastOrNull()
            if (lastR != null && r[0] <= lastR[1]) lastR[1] = maxOf(lastR[1], r[1]) else ranges.add(r.copyOf())
        }
    }

    companion object {
        /** The change from [old] to [new] only changes letter case, plus a
         * space or mark typed after it: "anna" -> "Anna " is still typed. */
        @JvmStatic fun caseOnly(old: String, new: String): Boolean {
            var p = 0
            while (p < old.length && p < new.length && old[p] == new[p]) p++
            var s = 0
            while (s < old.length - p && s < new.length - p && old[old.length - 1 - s] == new[new.length - 1 - s]) s++
            val deleted = old.substring(p, old.length - s)
            val inserted = new.substring(p, new.length - s)
            val word = inserted.trimEnd { !it.isLetterOrDigit() }
            return word.equals(deleted, ignoreCase = true) || inserted.equals(deleted, ignoreCase = true)
        }
    }

    /** The ranges in code points of [text] (what the journal writes). */
    fun codePoints(text: String): List<IntArray> = ranges.map {
        intArrayOf(text.codePointCount(0, it[0].coerceAtMost(text.length)), text.codePointCount(0, it[1].coerceAtMost(text.length)))
    }
}
