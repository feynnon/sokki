// Sokki: the space after punctuation that was typed without one.
//
// "done.we can" -> "done. We can", "klar,dass" -> "klar, dass". HeliBoard
// (like Gboard) adds no space after a typed "." or ","; the next word sticks
// to it. Robin's journal (19.-24.11.2026, 2,700 words) had 14 such misses
// and one "z.B." that had to stay.
//
// Decided when the word after the punctuation ends (space or punctuation
// typed), not when its first letter comes: only then can "sokki.org" be told
// from "sokki.Organ", and a link at the end of a message is never touched.
// SPDX-License-Identifier: GPL-3.0-only

package dev.verbatim.sokki

object Spacing {
    /** After these the next word gets a space. */
    private const val PUNCT = ".,!?;:"
    /** After these the next word starts a sentence. */
    private const val SENTENCE_END = ".!?"

    /**
     * "name.<this>" is a web address or a file, not two sentences: every
     * country ending, the common other web endings, common file types.
     * Left out on purpose, because they are everyday words that start
     * sentences: at, it, es, be, no, in, so, to, is, my, am, as, by, do, er,
     * im, re, je, na, mr, ms, ps, pro, online. In 2 million German and
     * English web sentences (Leipzig Corpora, 24.11.2026) each starts a
     * sentence far more often than it ends a domain ("Es" 17,345 times,
     * "x.es" once). A capital after the mark ("ende.Berlin") is always a
     * new sentence: web addresses are written in lower case.
     */
    private val ENDINGS = setOf(
        // web: country endings
        "ac", "ae", "af", "ag", "ai", "al", "ar", "au", "aw", "az", "ba", "bb", "bd", "bg", "bh",
        "bi", "bj", "bm", "bn", "bo", "br", "bs", "bt", "bw", "bz", "ca", "cc", "cd", "cf", "cg",
        "ch", "ci", "cl", "cm", "cn", "co", "cr", "cu", "cv", "cw", "cx", "cy", "cz", "de", "dj",
        "dk", "dm", "dz", "ec", "ee", "eg", "eu", "fi", "fj", "fk", "fm", "fo", "fr", "ga", "gd",
        "ge", "gf", "gg", "gh", "gi", "gl", "gm", "gn", "gp", "gr", "gt", "gu", "gy", "hk", "hn",
        "hr", "ht", "hu", "id", "ie", "il", "iq", "ir", "jm", "jo", "jp", "ke", "kg", "kh",
        "ki", "km", "kn", "kr", "kw", "ky", "kz", "lb", "lc", "li", "lk", "lr", "ls", "lt", "lu",
        "lv", "ly", "mc", "md", "me", "mg", "mk", "ml", "mm", "mn", "mp", "mq", "mt",
        "mu", "mv", "mw", "mx", "mz", "nc", "nf", "ng", "ni", "nl", "np", "nr", "nz", "om",
        "pa", "pf", "pg", "ph", "pk", "pl", "pr", "pt", "pw", "py", "qa", "ro", "rs", "ru",
        "rw", "sb", "sc", "sd", "se", "sg", "sh", "si", "sk", "sl", "sm", "sn", "sr", "st", "sv",
        "sx", "sy", "sz", "tc", "td", "tg", "th", "tj", "tk", "tl", "tm", "tn", "tr", "tt", "tv",
        "tw", "tz", "ua", "ug", "uk", "us", "uy", "uz", "va", "vc", "ve", "vg", "vi", "vn", "vu",
        "ws", "ye", "yt", "za", "zm", "zw",
        // web: other endings
        "com", "org", "net", "edu", "gov", "mil", "int", "info", "biz", "name", "mobi",
        "app", "dev", "page", "io", "xyz", "site", "store", "shop", "tech", "blog",
        "cloud", "news", "live", "design", "studio", "agency", "digital", "email", "space",
        "website", "club", "media", "group", "solutions", "software", "academy", "berlin",
        "hamburg", "koeln", "bayern", "nrw", "wien", "swiss", "gmbh",
        // files
        "txt", "md", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "json", "xml",
        "html", "htm", "php", "js", "ts", "tsx", "jsx", "py", "kt", "java", "rs", "sh", "yml",
        "yaml", "toml", "png", "jpg", "jpeg", "gif", "svg", "webp", "heic", "mp3", "mp4", "mov",
        "wav", "m4a", "zip", "apk", "exe", "dmg", "ics", "vcf",
    )

    /** Abbreviations: the space goes in, but no capital after them ("bzw. das"). */
    private val ABBREVIATIONS = setOf(
        "bzw", "usw", "ca", "evtl", "ggf", "inkl", "vgl", "sog", "bspw", "zzgl", "ggü", "nr", "dr",
        "hr", "fr", "str", "tel", "mio", "mrd", "ehem", "geb", "jh",
        "etc", "vs", "mr", "mrs", "ms", "approx", "incl", "fig", "st", "jr", "sr",
    )

    /**
     * [token] is the text before the cursor back to the last whitespace, just
     * after a word was finished and before the separator that ended it is
     * typed. Returns the token with the missing space put in (and the next
     * word capitalised after a sentence end when [autoCaps]), or null to
     * leave it as it is.
     */
    @JvmStatic
    fun fix(token: String, autoCaps: Boolean): String? {
        if (token.length < 4) return null
        // Links, mail addresses, paths, handles, code.
        if (token.any { it in "@/\\=_#&%+<>|~" } || token.contains("www.", ignoreCase = true)) return null
        // The last word: letters only, right after one punctuation mark.
        var start = token.length
        while (start > 0 && token[start - 1].isLetter()) start--
        val right = token.substring(start)
        if (right.isEmpty() || start < 2) return null
        val p = token[start - 1]
        if (p !in PUNCT) return null
        // The word before: at least two letters, right before the mark.
        // One letter is an abbreviation ("z.B.", "e.g.", "u.a."); digits
        // are numbers and versions ("1,5", "v1.2").
        var leftStart = start - 1
        while (leftStart > 0 && token[leftStart - 1].isLetter()) leftStart--
        val left = token.substring(leftStart, start - 1)
        if (left.length < 2) return null
        // Something else glued before it: "a.b.c", "v1.2", "...". After a comma
        // or another mark it's still a sentence ("sokki.org,see" -> "sokki.org, see").
        if (p == '.' && leftStart > 0 && token[leftStart - 1] !in "\"'(„“‚‘«»") return null
        val capitalised = right[0].isUpperCase() && right.any { it.isLowerCase() }
        if (p == '.' && !capitalised && right.lowercase() in ENDINGS) return null
        if ((p == ':' || p == ';') && right.length < 2) return null // smileys ":D", ";P"
        val next = if (autoCaps && p in SENTENCE_END && left.lowercase() !in ABBREVIATIONS) right.replaceFirstChar { it.uppercaseChar() } else right
        return token.substring(0, start) + " " + next
    }
}
