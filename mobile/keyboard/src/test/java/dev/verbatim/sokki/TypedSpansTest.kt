// SPDX-License-Identifier: GPL-3.0-only
package dev.verbatim.sokki

import org.junit.Assert.assertEquals
import org.junit.Test

class TypedSpansTest {
    private fun ranges(t: TypedSpans, text: String) = t.codePoints(text).map { it.toList() }

    @Test fun typingThenDictationThenTyping() {
        val t = TypedSpans()
        var text = ""
        fun step(next: String, typed: Boolean) { t.apply(text, next, typed); text = next }
        step("Hi", true)
        step("Hi Anna", true)
        step("Hi Anna, the plan works.", false)        // dictated
        step("Hi Anna, the plan works. See you", true)
        assertEquals(listOf(listOf(0, 7), listOf(24, 32)), ranges(t, text))
    }

    @Test fun editsShiftAndCutRanges() {
        val t = TypedSpans()
        t.apply("", "abc def", true)
        t.apply("abc def", "abc XY def", false)            // pasted in the middle
        assertEquals(listOf(listOf(0, 4), listOf(7, 10)), ranges(t, "abc XY def"))
        t.apply("abc XY def", "abc XY", true)              // deleted " def"
        assertEquals(listOf(listOf(0, 4)), ranges(t, "abc XY"))
    }

    @Test fun autocorrectionIsNotTyped() {
        val t = TypedSpans()
        t.apply("", "teh", true)
        t.apply("teh", "the ", false)                      // autocorrect on space
        assertEquals(listOf(listOf(0, 1)), ranges(t, "the "))
    }

    @Test fun caseOnlyCorrections() {
        assertEquals(true, TypedSpans.caseOnly("Hi anna", "Hi Anna "))
        assertEquals(true, TypedSpans.caseOnly("bis morgen", "bis Morgen,"))
        assertEquals(true, TypedSpans.caseOnly("See", "See "))
        assertEquals(false, TypedSpans.caseOnly("teh", "the "))
        assertEquals(false, TypedSpans.caseOnly("done.we", "done. We"))
    }

    @Test fun codePointsCountEmojiOnce() {
        val t = TypedSpans()
        t.apply("", "😀", false)                // an emoji, not typed
        t.apply("😀", "😀ok", true)
        assertEquals(listOf(listOf(1, 3)), ranges(t, "😀ok"))
    }
}
