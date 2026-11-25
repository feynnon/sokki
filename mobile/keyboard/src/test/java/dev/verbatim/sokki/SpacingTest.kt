// SPDX-License-Identifier: GPL-3.0-only
package dev.verbatim.sokki

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Made-up text shaped like the misses in Robin's journal; no real messages. */
class SpacingTest {
    private fun fixed(token: String) = Spacing.fix(token, autoCaps = true)

    @Test fun missingSpaceGetsOne() {
        val cases = mapOf(
            "done.we" to "done. We",           // sentence end, next word lower case
            "morgen.also" to "morgen. Also",
            "ready.let" to "ready. Let",
            "wirklich?ja" to "wirklich? Ja",
            "super!danke" to "super! Danke",
            "Liste,ok" to "Liste, ok",         // comma: space, no capital
            "klar,dass" to "klar, dass",
            "Hinweis:bitte" to "Hinweis: bitte",
            "gut;dann" to "gut; dann",
            "Ende.Danach" to "Ende. Danach",   // already a capital
            "bzw.das" to "bzw. das",           // abbreviation: space, no capital
            "Dr.Meier" to "Dr. Meier",
            "etc.and" to "etc. and",
            "(fertig.dann" to "(fertig. Dann",
            "„Hallo.wie" to "„Hallo. Wie",
            "später.übermorgen" to "später. Übermorgen",
            "fertig.es" to "fertig. Es",       // everyday words that are also country endings
            "done.it" to "done. It",
            "klar.at" to "klar. At",
            "ende.Berlin" to "ende. Berlin",   // a capital: a new sentence, even for an ending
            "gut.Online" to "gut. Online",
            "sokki.org,see" to "sokki.org, see", // a domain, then a comma
            "notes.pdf!thanks" to "notes.pdf! Thanks",
        )
        for ((typed, want) in cases) assertEquals(typed, want, fixed(typed))
    }

    @Test fun noCapitalWithoutAutoCaps() {
        assertEquals("done. we", Spacing.fix("done.we", autoCaps = false))
    }

    @Test fun staysJoined() {
        val cases = listOf(
            "z.B", "e.g", "i.e", "u.a", "U.S",            // one-letter abbreviations
            "sokki.org", "example.com", "Sokki.de", "amazon.co", "co.uk", "domain.io",
            "shop.ch", "zalando.fr", "github.io", "bbc.uk", "sokki.app", "my.shop", "site.nl",
            "x.ai", "orf.wien", "example.berlin", "news.tv", "notes.me", "ASP.NET",
            "notes.txt", "Bericht.pdf", "index.html", "Node.js", "README.md", "Foto.JPG",
            "v1.2", "1,5", "3.50", "10:30", "24.11", "Version2.neu",   // numbers, versions
            "name@mail.de", "mail@example", "https://sokki", "www.sokki", "a/b.c",
            "foo_bar.baz", "#tag.weiter", "x=1,y",
            "cool:D", "gut;P",                             // smileys
            "a.b.c", "Hallo...wie", "Mo.-Fr",               // runs and ellipses
            "word", "Hallo.", "ab.", ".",                    // nothing after the mark
        )
        for (typed in cases) assertNull(typed, fixed(typed))
    }
}
