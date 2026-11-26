// SPDX-License-Identifier: GPL-3.0-only
package dev.verbatim.sokki

import org.junit.AfterClass
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.BeforeClass
import org.junit.Test
import java.io.File

/**
 * Typos on the real word tables (assets/sokki), with made-up words. With build/typos-parity/in.tsv
 * present (tools/typos_parity.py writes it) it also answers every case there into out.tsv, which the
 * script compares with typo_model.py.
 */
class TyposTest {
    companion object {
        private val assets = File("src/main/assets/sokki")

        @BeforeClass @JvmStatic fun load() {
            Typos.load(File(assets, "words.bin").readBytes(), File(assets, "bigrams.bin").readBytes(),
                File(assets, "common.txt").readText())
        }

        @AfterClass @JvmStatic fun reset() {
            Typos.setKeys(null)
            Typos.textBefore = null
        }

        /** A QWERTY phone layout: rows shifted by half a key, keys 10 x 10. */
        val qwerty: Map<Char, DoubleArray> = buildMap {
            listOf("qwertyuiop", "asdfghjkl", "zxcvbnm").forEachIndexed { r, row ->
                row.forEachIndexed { i, c -> put(c, doubleArrayOf(i * 10 + 5.0 + r * 5, r * 10 + 5.0, 10.0, 10.0)) }
            }
        }
    }

    private fun fix(typed: String, vararg prev: String) = Typos.correction(typed, emptyList(), emptySet(), prev.toList(), null)

    @Test fun aWordTwoSlipsFromACommonOneIsFixed() {
        assertEquals("funktioniert", fix("fuktionirt", "das"))
    }

    @Test fun britishSpellingsStay() {
        assertNull(fix("optimised", "we"))
        assertNull(fix("prioritise", "together"))
    }

    @Test fun neighboursFollowTheKeyboardOnScreen() {
        Typos.setKeys(null) // QWERTZ rows
        assertNull(fix("tou", "thank"))
        assertEquals("very", fix("verz", "ist")) // y and z swapped: the QWERTY habit on QWERTZ
        Typos.setKeys(qwerty)
        assertEquals("you", fix("tou", "thank"))
        assertNull(fix("verz", "ist"))
        Typos.setKeys(null)
    }

    /** typed, previous words, next, languages, candidates (word/lang), text words before: one case per line. */
    @Test fun parity() {
        val dir = File("build/typos-parity")
        val input = File(dir, "in.tsv")
        if (!input.exists()) return
        val out = StringBuilder()
        var context = emptyList<String>()
        Typos.textBefore = { context.joinToString(" ") + " x" }
        for (line in input.readLines()) {
            if (line.startsWith("#keys\t")) {
                val spec = line.substringAfter('\t')
                Typos.setKeys(if (spec == "none") null else spec.split(';').associate { e ->
                    e[0] to e.substring(2).split(',').map { it.toDouble() }.toDoubleArray()
                })
                out.append(line).append('\n')
                continue
            }
            val f = line.split('\t')
            fun words(s: String) = if (s.isEmpty()) emptyList() else s.split(' ')
            val candidates = words(f[4]).map { it.substringBeforeLast('/') to it.substringAfterLast('/').takeIf { l -> l != "-" } }
            context = words(f[5])
            val answer = Typos.correction(f[0], candidates, f[3].split(',').filter { it.isNotEmpty() }.toSet(), words(f[1]),
                f[2].ifEmpty { null })
            out.append(f[0]).append('\t').append(answer ?: "").append('\n')
        }
        File(dir, "out.tsv").writeText(out.toString())
        Typos.setKeys(null)
        Typos.textBefore = null
    }
}
