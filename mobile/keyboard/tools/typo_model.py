"""Sokki: the "typos only" autocorrect, as dev.verbatim.sokki.Typos runs it.

Keep equal to Typos.kt (bench_device.py checks: 0 of 8,000 decisions differed).
gen_known_words.py writes the word table, gen_bigrams.py the word-pair table;
`python typo_model.py <text file>` replays a text and prints every word the
keyboard would change.

One score decides (2026-11-20, score_context.py; it replaced a stack of hand
rules that fixed fewer typos and got five times as many wrong):
- Known words are never changed: zipf >= 3 in wordfreq's large German or
  English list, a German compound of two such words, an English stem with a
  German ending (publishen, geforwarded), or a word on the personal list.
- Every word one slip from the typed one (a letter extra, missing or changed,
  or two letters swapped; the first letter too) that is at least CAND_ZIPF
  common competes with the typed word itself on
      log10 P(word | previous word) + log10 P(typed | word).
  P(word | previous word) comes from the pair table, else 0.4 x P(word) from
  the word table; P(typed | word) from per-letter slip rates measured on
  typing37k (SLIP). The typed word, when it is no word: log10 P = OOV.
- The best word replaces the typed one when its share of all the scores is at
  least TAU. A lowercase noun the keyboard only knows capitalised competes the
  same way and is capitalised when it clearly wins ("am montag", not "I gott").
- A letter that slipped across the space is moved back when the typed word is
  no word, both new words are common and the first gets more common
  ("th ecar" -> "the car").
- A word typed with a capital in mid-sentence keeps its first letter (a name:
  Qwen is not "wen"; Robin's text, 2026-11-20).
- Words under 3 letters, with anything but letters, or with capitals after the
  first letter (CPQL, iPhone) are left as typed.

Checking it:
    ~/.cache/sokki-eval-venv/bin/python mobile/keyboard/tools/typo_model.py my_messages.txt
SPDX-License-Identifier: GPL-3.0-only
"""

import math
import os
import re
import struct
import sys

MAGIC = b"SKW1"
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3
MASK = (1 << 64) - 1

COMMON = 3.0  # zipf from which a word is known (never changed)
MIN_LEN = 3

QWERTZ = ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
_POS = {c: (r, i + 0.5 * r) for r, row in enumerate(QWERTZ) for i, c in enumerate(row)}


def fold(w):
    """Casefold as wordfreq does for German and English: lower case, ß -> ss."""
    return w.lower().replace("ß", "ss")


def neighbours(a, b):
    if {a, b} == {"y", "z"}:  # QWERTY habit on QWERTZ
        return True
    if a not in _POS or b not in _POS:
        return False
    (r1, c1), (r2, c2) = _POS[a], _POS[b]
    return abs(r1 - r2) <= 1 and abs(c1 - c2) <= 1.0


def _hashes(key, k, m):
    h1 = FNV_OFFSET
    for b in key.encode("utf-8"):
        h1 = ((h1 ^ b) * FNV_PRIME) & MASK
    z = (h1 + 0x9E3779B97F4A7C15) & MASK
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
    h2 = (z ^ (z >> 31)) | 1
    return [((h1 + i * h2) & MASK) % m for i in range(k)]


class Bloom:
    def __init__(self, n, p):
        self.m = max(64, int(-n * math.log(p) / (math.log(2) ** 2)) // 64 * 64)
        self.k = max(1, round(self.m / n * math.log(2)))
        self.bits = bytearray(self.m // 8)

    def add(self, key):
        for i in _hashes(key, self.k, self.m):
            self.bits[i >> 3] |= 1 << (i & 7)

    def __contains__(self, key):
        return all(self.bits[i >> 3] & (1 << (i & 7)) for i in _hashes(key, self.k, self.m))

    def dump(self):
        return MAGIC + struct.pack(">iq", self.k, self.m) + bytes(self.bits)

    @classmethod
    def load(cls, data):
        assert data[:4] == MAGIC
        b = cls.__new__(cls)
        b.k, b.m = struct.unpack(">iq", data[4:16])
        b.bits = bytearray(data[16:])
        return b


# Keys in the table: "<b>:<word>" with b = floor(2 * zipf) (1..16, the larger
# of German and English), "d:<word>" / "e:<word>" for words with zipf >= 3 in
# German / English, and "w:<word>" for words with zipf >= CAND_ZIPF.
MAX_BUCKET = 16


def bucket(zipf):
    return max(1, min(MAX_BUCKET, int(zipf * 2)))


TABLE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src", "main", "assets", "sokki", "words.bin")


class Table:
    def __init__(self, bloom):
        self.bloom = bloom

    def zipf(self, w):
        """Zipf of a folded word, in half steps (bucket middle); 0 if unlisted."""
        for b in range(MAX_BUCKET, 0, -1):
            if f"{b}:{w}" in self.bloom:
                return b / 2 + 0.25
        return 0.0

    def common_in(self, lang, w):
        return f"{lang}:{w}" in self.bloom

    NOUNS = None  # offline experiments: a set standing in for the "C:" keys

    def noun(self, w):
        """German text writes this folded word with a capital (90%+ of its uses mid-sentence)."""
        return w in self.NOUNS if self.NOUNS is not None else f"C:{w}" in self.bloom

    def candidate(self, w):
        """At least CAND_ZIPF common: may replace a typed word."""
        return f"w:{w}" in self.bloom


DENGLISH_ENDINGS = ("en", "n", "t", "st", "et", "te", "ten", "s")


def built_word(table, w):
    """A German compound of two common words, or an English stem with a German ending."""
    for suf in DENGLISH_ENDINGS:
        if STRICT_DENGLISH and len(suf) == 1:
            continue
        # a one-letter ending needs a 4-letter stem ("soo" + "s" is no word; "own" + "en" is)
        if w.endswith(suf) and len(w) - len(suf) >= (4 if len(suf) == 1 else 3) and table.common_in("e", w[: -len(suf)]):
            return True
    if w.startswith("ge") and (table.common_in("e", w[2:]) or (w.endswith("t") and table.common_in("e", w[2:-1]))):
        return True
    for i in range(3, len(w) - 2):
        a, b = w[:i], w[i:]
        if min(len(a), len(b)) < 4 and max(len(a), len(b)) < 5:
            continue  # a 3-letter part needs a long other part (vor+lagern; not han+hing)
        if table.common_in("d", b) and (table.common_in("d", a) or (a.endswith("s") and table.common_in("d", a[:-1]))):
            return True
    return False


def umlaut_twin(table, t, z):
    """A form of folded `t` with ä/ö/ü for a/o/u that is at least 10x more common (konnen -> können)."""
    spots = [i for i, c in enumerate(t) if c in "aou"][:4]
    for mask in range(1, 1 << len(spots)):
        w = list(t)
        for k, i in enumerate(spots):
            if mask >> k & 1:
                w[i] = {"a": "ä", "o": "ö", "u": "ü"}[w[i]]
        if table.zipf("".join(w)) >= z + 1:
            return True
    return False


def umlaut_form(table, t):
    """The common German word that is folded `t` with ä/ö/ü for a/o/u (fur -> für), or None."""
    spots = [i for i, c in enumerate(t) if c in "aou"][:4]
    best = None
    for mask in range(1, 1 << len(spots)):
        w = list(t)
        for k, i in enumerate(spots):
            if mask >> k & 1:
                w[i] = {"a": "ä", "o": "ö", "u": "ü"}[w[i]]
        w = "".join(w)
        if table.common_in("d", w) and (best is None or table.zipf(w) > table.zipf(best)):
            best = w
    return best


def known(table, personal, w):
    t = fold(w)
    if t in personal:
        return True
    z = table.zipf(t)
    if CURATED_KNOWN and umlaut_twin(table, t, z):  # konnen, uberlegt, personlich: web forms of umlaut words
        return False
    if z >= COMMON:
        return True
    return built_word(table, t) or (VERB_STEMS and verb_stem(table, t)) or (INFLECTIONS and inflected(table, t))


GERMAN_PREFIXES = ("un", "ab", "an", "auf", "aus", "be", "ein", "ent", "er", "ge", "mit", "nach", "um", "ver", "vor",
                   "weg", "zer", "zu", "rein", "raus", "rum", "ran", "runter", "hin", "her")
GERMAN_ENDINGS = ("ere", "eres", "erem", "eren", "erer", "est", "end", "ens", "em", "es", "er", "en", "e", "st", "t",
                  "te", "ten", "s")


def inflected(table, t):
    """A German word built from a common one by inflection or a prefix, as Hunspell's affix rules
    and TongueSwitcher do it: fokussier-st, einzigartig-ere, initial-em, generierung-s, un-qualifiziert,
    ab-zu-klopfen. The remainder must be a common German word (or its verb: fokussier -> fokussieren)."""
    def common(w):
        return len(w) >= 4 and (table.common_in("d", w) or table.common_in("d", w + "en") or table.common_in("d", w + "n")
                                or (table.zipf(w) >= 2.5 and not table.common_in("e", w)))
    stems = [t]
    for pre in GERMAN_PREFIXES:
        if t.startswith(pre) and len(t) - len(pre) >= 4:
            rest = t[len(pre):]
            stems.append(rest[2:] if rest.startswith("zu") and len(rest) >= 6 else rest)
            stems.append(rest)
    for w in stems:
        if w != t and common(w):
            return True
        for end in GERMAN_ENDINGS:
            if w.endswith(end) and common(w[: -len(end)]):
                return True
    return False


def compound_fix(table, t):
    """The German compound one slip from folded `t`, split where one part is a German word as typed
    and the other, a word no list has, is one slip from one (bchhaltungs|thema, stuer|ergebnisse),
    or None when none or two compete. The first part may end in the linking s."""
    def common(w, left):  # the linking s only ends a first part (unternehmen-s|profil)
        return len(w) >= 4 and (table.common_in("d", w) or (left and w.endswith("s") and len(w) >= 5
                                                             and table.common_in("d", w[:-1])))
    found = {}
    for i in range(4, len(t) - 3):
        a, b = t[:i], t[i:]
        for fixed, typed_part, left in ((b, a, True), (a, b, False)):
            # the slipped part is no listed word itself (farb|kodiert, prio|liste, wort|blob stay)
            if not common(fixed, not left) or table.candidate(typed_part):
                continue
            for c in one_slip(typed_part):
                if len(c) >= 4 and common(c, left):
                    w = c + b if left else a + c
                    s = channel(typed_part, c) + table.zipf(c.removesuffix("s") if not table.common_in("d", c) else c)
                    found[w] = max(found.get(w, -99.0), s)
    if not found:
        return None
    ranked = sorted(found.items(), key=lambda x: -x[1])
    if len(ranked) > 1 and ranked[0][1] - ranked[1][1] < math.log10(RUNNER_UP):
        return None
    return ranked[0][0]


def adjective(table, w):
    """An inflected German adjective before a noun: klein-es, neu-er, sinnvoll-en (its stem a German
    word, as Hunspell's affix rules check), German only, neither a noun (Ergebnissen) nor a verb
    (zieh-e: zieht; find-en: findet; soll-te: sollen; können). Not immer, trotzdem, wieder: their stems
    are no words, or names (Wied)."""
    a = fold(w)
    if lean(table, a) != "d" or table.noun(a) or infinitive(table, a):
        return False
    if a.endswith(("te", "ten")) and infinitive(table, a[: a.rindex("te")] + "en"):
        return False
    for end in ("em", "en", "er", "es", "e"):
        if a.endswith(end):
            stem = a[: -len(end)]
            return (len(stem) >= 3 and table.common_in("d", stem) and not table.noun(stem)
                    and not table.common_in("d", stem + "t") and not table.common_in("d", stem + "et"))
    return False


def infinitive(table, t):
    """A German verb infinitive: -en/-ern/-eln whose stem also conjugates (auswählen: auswählt; ändern:
    ändert); not an ordinal like "ersten"."""
    if not table.common_in("d", t):
        return False
    for end, stem_end in (("ern", "ert"), ("eln", "elt"), ("en", "t")):
        if t.endswith(end) and table.common_in("d", t[: -len(end)] + stem_end):
            return True
    return False


def verb_stem(table, t):
    """A German verb form without its ending, as in speech and imperatives: rechne(n), and stems
    that also conjugate: informier(en/t), leg(en/t); with a clitic s (machs = mach es). Not
    "zieg" (ziegen are goats: no "ziegt")."""
    for stem in (t, t[:-1] if t.endswith("s") else None):
        if not stem or len(stem) < 3:
            continue
        if stem.endswith("e") and table.common_in("d", stem + "n"):
            return True
        if stem[-2:] in ("le", "re") and table.common_in("d", stem[:-2] + "e" + stem[-2] + "n"):  # sammle: sammeln
            return True
        if table.common_in("d", stem + "en") and table.common_in("d", stem + "t"):
            return True
    return False


def sentence_lang(table, prev, nxt=None):
    """'de' or 'en' for the sentence so far: the last three words count 1, 1/2, 1/4 (German +1,
    English -1 for a word only common in that language), the next word 1/2. German wins ties: it
    is the user's first language."""
    ws = [prev] if isinstance(prev, str) or prev is None else list(prev)
    score, weight = 0.0, 1.0
    for w in reversed(ws[-3:]):
        lang = lean(table, w)
        score += weight * (1 if lang == "d" else -1 if lang == "e" else 0)
        weight /= 2
    lang = lean(table, nxt)
    score += 0.5 * (1 if lang == "d" else -1 if lang == "e" else 0)
    return "en" if score < 0 else "de"


def lean(table, w):
    """'d' / 'e' for a word only common in German / English, else None."""
    if not w or w == BOS:
        return None
    t = fold(w)
    d, e = table.common_in("d", t), table.common_in("e", t)
    return "d" if d and not e else "e" if e and not d else None


def space_fix(table, personal, prev, typed):
    """A letter that slipped across the space: ("th", "ecar") -> ("the", "car");
    or None. Only when the typed word is no word, both new words are common, the
    new first word is more common than the old one (not "a sking" -> "as king"),
    and neither word has capitals after its first letter (LLM, MCP)."""
    if not prev.isalpha() or not typed.isalpha() or len(typed) < 3 or known(table, personal, typed):
        return None
    if any(c.isupper() for c in prev[1:] + typed[1:]):
        return None
    a, b = prev + typed[0], typed[1:]
    za, zb = table.zipf(fold(a)), table.zipf(fold(b))
    if za >= COMMON and zb >= COMMON and za > table.zipf(fold(prev)):
        return a, b
    return None


# ---- The score (see the top). Swept in score_context.py: these keep correct
# words unchanged on every test set; a laxer prior (-12) or a channel weight of
# 0.75 fixes ~6 points more typos but changed real words (Ollama -> Llama).
OOV = -10.0  # log10 P of a typed non-word
TAU = 0.95  # share the best word needs
CAND_ZIPF = 2.0  # candidates are at least this common
BOS = "<s>"
BACKOFF = math.log10(0.4)
# log10 P of one slip per letter, typing37k (8.5M letters, 51k typos): a
# neighbouring key (one of ~4), another key (one of ~21), a letter missing, an
# extra letter (doubled / next to a neighbour / any), two letters swapped.
SLIP = {"near": math.log10(0.00308 / 4.2), "far": math.log10(0.00118 / 21), "missing": math.log10(0.00171),
        "double": math.log10(0.00017), "extra-near": math.log10(0.00032 / 8.4),
        "extra-far": math.log10(0.00068 / 24), "swap": math.log10(0.00033)}
FIRST = -0.3  # slips on the first letter are 2-3 times rarer
MATCH = math.log10(1 - 0.0075)
LETTERS = "abcdefghijklmnopqrstuvwxyzäöü"
BASE = {"ä": "a", "ö": "o", "ü": "u"}

# Switches robin_eval.py compares; the phone runs these values (2026-11-23 loop, measured on
# Robin's own writing: his real typos fixed 46.9% -> 63.4%, nouns capitalised 47% -> 63%).
SWAP_BOOST = 5.0  # times the swap rate: half of Robin's typos are two swapped letters
UMLAUT = -0.2  # log10 P of typing the plain vowel for an umlaut (a missed long-press); None: a far key
STRICT_DENGLISH = True  # Denglish endings need 2+ letters ("losts", "tought" are typos, not lost+s)
RARE_AS_UNKNOWN = True  # a listed word the dictionaries don't know competes like an unknown one
CURATED_KNOWN = True  # a common web form with a much more common umlaut twin isn't protected (konnen)
VERB_STEMS = True  # a German verb without its ending is a word (informier, rechne, leg, machs)
LANG_CAPS = True  # capitals by sentence language: in German a noun gets its capital even if English
                  # has the lower-case word (den Link); in English nothing is capitalised
INFLECTIONS = True  # German inflections and prefixes of common words are words (organisierst, unvorbereiteter)
# Off: on Robin's writing it cost 0.6 points of typos fixed for one false change less (2026-11-23).
FRONT_GUARD = False  # no letter added or dropped in front of a word (a tool name like "kstack" is not "stack")
UMLAUT_IN_GERMAN = True  # a word only English (or the user's history) has, with a common German umlaut twin,
                          # becomes the twin in a German sentence (fur -> für, uber -> über)
NOMINALISED = True  # after zum/beim/vom a German verb is a noun: zum Auswählen (not "zum ersten Mal")
SPLIT_DECISION = False  # detection and choice apart (Kukich 1992): a typed word that is surely a typo (its
                        # own share < 1 - TAU) gets the best fix when it leads the next one RUNNER_UP times
                        # (fot -> for over got). Off, not in the app: on Robin's writing (2026-11-23,
                        # x5) it fixed 13 more typos but changed about as many of his own words (names,
                        # tool names, Denglish verbs); word frequency doesn't tell them apart. Revisit once
                        # the keyboard learns his words.
RUNNER_UP = 5.0
NEXT_CONTEXT = False  # the word after counts too, log10 P(next | word) (correcting a word once the next is
                      # typed, as Gboard does). Off: the app decides before the next word exists.
COMPOUND_FIX = True  # a long non-word one slip from a German compound gets it (Rchnungsthema ->
                      # Rechnungsthema), as Hunspell's and LanguageTool's compound suggestions do
COMPOUND_MIN = 9
PERSONAL_TARGETS = False  # a word the keyboard learned from the user (a candidate with no language) one
                          # slip away competes like a common word (Zentrxi -> Zentrix, a learned name). Off, not in
                          # the app: on Robin's writing (2026-11-23) 2 more typos fixed, 1-4 of his words
                          # changed (Kundenprojekts -> Kundenprojekt).
PERSONAL_ZIPF = 2.0
EN_IN_GERMAN = True  # in a German sentence an English-only word one slip from a German word competes with it
                      # on the pair table (mir bite die -> mir bitte die), instead of being kept as valid; the
                      # English word pays a language switch, as in bilingual decoders
EN_IN_GERMAN_GAP = 1.0  # the German word must be this much more common (zipf)
SWITCH = -1.0  # log10 P(an English-only word in a German sentence)
TRANSLIT = True  # ae/oe/ue for ä/ö/ü (DIN 5007), when the typed word isn't German or English: wuerde ->
                  # würde, fuer -> für; the plain vowel stays (wurde), so nothing is guessed
SS_ESZETT = True  # ss right after ei/au/eu/äu/ie becomes ß when the German dictionary spells it so: weiss ->
                   # weiß, ausser -> außer (after a diphthong German writes ß; dass, muss stay)
CAP_SPELLING = True  # a capitalised word gets the dictionary's spelling too: Heisst -> Heißt
MULTI_UMLAUT = True  # a non-word missing two or more umlauts: Geschaftsfuhrer -> Geschäftsführer
SHARED_NOUNS = True  # an English word also gets a German capital after an inflected German adjective, as
                      # after an article: a German noun phrase is article, adjectives, noun (neuer Tag,
                      # sinnvollen Tests, kleines Update; not "ist easy", "können open source")
OOV_PER_LETTER = 0.25  # a longer typed non-word is less likely meant: OOV - this x (letters - 4)
TRUECASE = True  # in a German sentence, a word German text writes capitalised (words.bin "C:" keys,
                 # gen_case_counts.py) gets its capital, except right after a personal pronoun
PRONOUNS = {"ich", "du", "er", "sie", "es", "wir", "ihr", "man", "sich"}  # ich krieg, sich stelle: verbs
# German words a noun follows: articles, determiners, prepositions, colloquial articles (ne, nem).
# An English word only gets a German capital after one of these (den Link, ne Weile; not "ist easy").
DETERMINERS = set("""der die das den dem des ein eine einen einem einer eines kein keine keinen keinem keiner
mein meine meinen meinem meiner dein deine deinen deinem deiner sein seine seinen seinem seiner ihre ihren
ihrem ihrer unser unsere unseren unserem unserer euer eure euren eurem diese dieser dieses diesen diesem
jede jeder jedes jeden jedem alle allen viele vielen mehr mehrere mehreren welche welcher welches welchen
zum zur im am beim vom ins ans aufs für mit von zu bei nach aus über unter auf in an um durch ohne gegen
pro per via vor hinter neben zwischen ne nen nem ner""".split())


def bigram_bucket(lp):
    """log10 P(word | previous word) in half steps, 1..16."""
    return max(1, min(MAX_BUCKET, int(2 * (lp + 8))))


def channel(typed, word):
    """log10 P(typed | word), both folded: the likeliest slips (weighted Damerau-Levenshtein)."""
    n, m = len(word), len(typed)
    neg = -1e9
    d = [[neg] * (m + 1) for _ in range(n + 1)]
    d[0][0] = 0.0
    for i in range(n + 1):
        for j in range(m + 1):
            if i == j == 0:
                continue
            first = FIRST if i <= 1 else 0.0
            best = neg
            if i and j:
                if word[i - 1] == typed[j - 1]:
                    best = d[i - 1][j - 1] + MATCH
                elif UMLAUT is not None and BASE.get(word[i - 1]) == typed[j - 1]:
                    best = d[i - 1][j - 1] + UMLAUT
                else:
                    best = d[i - 1][j - 1] + SLIP["near" if neighbours(typed[j - 1], word[i - 1]) else "far"] + first
            if i:
                best = max(best, d[i - 1][j] + SLIP["missing"] + first)
            if j:
                ch, around = typed[j - 1], word[max(0, i - 1): i + 1]
                kind = "double" if ch in around else "extra-near" if any(neighbours(ch, c) for c in around) else "extra-far"
                best = max(best, d[i][j - 1] + SLIP[kind] + (FIRST if i == 0 else 0.0))
            if i > 1 and j > 1 and word[i - 1] == typed[j - 2] and word[i - 2] == typed[j - 1]:
                best = max(best, d[i - 2][j - 2] + SLIP["swap"] + math.log10(SWAP_BOOST) + (FIRST if i == 2 else 0.0))
            d[i][j] = best
    return d[n][m]


def one_slip(t):
    """Every string one slip from folded `t`: a letter extra, missing, changed, or two swapped."""
    out = set()
    for i in range(len(t) + 1):
        for ch in LETTERS:
            out.add(t[:i] + ch + t[i:])
    for i in range(len(t)):
        out.add(t[:i] + t[i + 1:])
        if i + 1 < len(t):
            out.add(t[:i] + t[i + 1] + t[i] + t[i + 2:])
        for ch in LETTERS:
            out.add(t[:i] + ch + t[i + 1:])
    out.discard(t)
    return sorted(out)


def lm_score(table, bigrams, w, zw, prev):
    """log10 P(w | prev): the pair's bucket middle, else 0.4 x P(w); P(w) from zipf `zw`."""
    if prev and bigrams is not None:
        for b in range(MAX_BUCKET, 0, -1):
            if f"{b}:{prev} {w}" in bigrams:
                return b / 2 - 8 + 0.25
        return BACKOFF + zw - 9
    return zw - 9


def correction(table, bigrams, personal, typed, candidates=(), typed_in_dictionary=False, prev=None, vocab=None,
               nxt=None):
    """The word autocorrect puts in place of `typed`, or None. `candidates`: the keyboard's
    suggestions, as words or (word, language) pairs (their spelling wins: Straße, Montag; a
    capital only comes from a German one). `typed_in_dictionary`: the keyboard knows the typed
    word as is: True, or the set of languages that know it ("de", "en", "user"). `prev`: the
    sentence's words before (nearest last; BOS at a sentence start; or just the one word), `nxt`:
    the word after if known. `vocab`: offline speed-up only, every word of words.bin."""
    if len(typed) < MIN_LEN or not typed.isalpha() or any(c.isupper() for c in typed[1:]):
        return None
    t = fold(typed)
    valid = {"de", "en", "user"} if typed_in_dictionary is True else set(typed_in_dictionary or ())
    if t in personal:
        valid.add("user")
    prevs = [prev] if isinstance(prev, str) or prev is None else list(prev)
    last = prevs[-1] if prevs else None
    cands = [(c, None) if isinstance(c, str) else c for c in candidates]
    # The same word spelled as the dictionary has it: a capital (montag -> Montag) or ß. A
    # capital only from German (allen stays, not "Allen" from an English name list).
    cap, cap_lang = next(((c, lang) for c, lang in cands if fold(c) == t and c != typed and not any(x.isupper() for x in c[1:])
                          and (lang in (None, "de") or c[:1] == typed[:1])
                          and (c[:1] == typed[:1] or (typed[0].islower() and c[:1].isupper()))), (None, None))
    if CAP_SPELLING and cap is None and typed[0].isupper():
        cap, cap_lang = next(((c[0].upper() + c[1:], lang) for c, lang in cands if fold(c) == t and c[:1].islower()
                              and c[0].upper() + c[1:] != typed and lang in (None, "de")), (None, None))
    if TRANSLIT and valid <= {"user"} and re.search("ae|oe|ue", t) and not any(
            # ue may be a swapped eu, the most common slip: huete may be heute, so it doesn't become hüte
            table.common_in("d", t[:i] + "e" + t[i] + t[i + 2:]) for i in (m.start() for m in re.finditer("ae|oe|ue", t))):
        spots = [m.start() for m in re.finditer("ae|oe|ue", t)][:4]
        best = None
        for mask in range(1, 1 << len(spots)):
            w = t
            for k in reversed(range(len(spots))):
                if mask >> k & 1:
                    i = spots[k]
                    w = w[:i] + {"a": "ä", "o": "ö", "u": "ü"}[w[i]] + w[i + 2:]
            # a common German word, else one German builds (zerdrücken, Steuerschlüssel, einschätzung)
            score = table.zipf(w) if table.common_in("d", w) else 0.0 if not table.common_in("e", w) and known(table, (), w) else None
            if score is not None and (best is None or score > best[0]):
                best = (score, w)
        if best:
            best = best[1]
            spelled = next((c for c, _ in cands if fold(c) == best), best)
            return spelled[0].upper() + spelled[1:] if typed[0].isupper() else spelled
    if SS_ESZETT and "ss" in t and (typed.islower() or last in (None, BOS)):
        esz = next((c for c, lang in cands if lang in (None, "de") and fold(c) == t and "ß" in c
                    and re.search("(ei|ai|au|eu|äu|ie)ß", c.lower())), None)
        if esz:
            return esz[0].upper() + esz[1:] if typed[0].isupper() else esz
    if valid == {"user"} and typed.islower() and cap and cap[:1].isupper() and cap_lang == "de":
        valid = set()  # the history learned a noun typed in lower case: the dictionary has its capital
    def truecase():
        """The capitalised form when German writes this word capitalised and the sentence is German."""
        if NOMINALISED and typed.islower() and last not in (None, BOS) and fold(last) in ("zum", "beim", "vom") \
                and infinitive(table, t):
            return typed[0].upper() + typed[1:]
        if not (TRUECASE and typed.islower() and last not in (None, BOS) and table.noun(t)):
            return None
        if fold(last) in PRONOUNS or sentence_lang(table, prevs, nxt) != "de":
            return None
        # an English word only after a German article or preposition, with no English word after
        # (den Link; not "image processing", "ist easy", "best practices")
        if table.common_in("e", t) and (not (fold(last) in DETERMINERS or (SHARED_NOUNS and adjective(table, last)))
                                         or lean(table, nxt) == "e"):
            return None
        if umlaut_twin(table, t, table.zipf(t)):  # hort is more likely hört than Hort
            return None
        return cap if cap and cap[:1].isupper() else typed[0].upper() + typed[1:]

    if (UMLAUT_IN_GERMAN and valid and valid <= {"en", "user"} and sentence_lang(table, prevs, nxt) == "de"
            and lean(table, typed) != "d"):
        twin = umlaut_form(table, t)
        if twin and table.zipf(twin) >= table.zipf(t) + 2:  # 100x more common: für vs fur, not regulär vs regular
            spelled = next((c for c, _ in cands if fold(c) == twin), twin)
            return spelled[0].upper() + spelled[1:] if typed[0].isupper() else spelled
    if (EN_IN_GERMAN and valid == {"en"} and lean(table, typed) == "e" and last not in (None, BOS)
            and sentence_lang(table, prevs, nxt) == "de"):
        p0 = fold(last)
        zt = table.zipf(t)
        scored = [(lm_score(table, bigrams, t, zt, p0) + len(t) * MATCH + SWITCH, t)]
        for c in one_slip(t):
            if table.common_in("d", c) and not table.common_in("e", c) and table.zipf(c) >= zt + EN_IN_GERMAN_GAP:
                scored.append((lm_score(table, bigrams, c, table.zipf(c), p0) + channel(t, c), c))
        top, word = max(scored)
        if word != t and 1.0 / sum(10 ** (s - top) for s, _ in scored) >= TAU:
            spelled = next((c for c, _ in cands if fold(c) == word), word)
            return spelled[0].upper() + spelled[1:] if typed[0].isupper() else spelled
    if valid:
        # Words the keyboard knows are never changed, only capitalised. With LANG_CAPS a German
        # noun typed in lower case that only English knows that way gets its capital in German.
        tc = truecase()
        if tc:
            return tc
        if (LANG_CAPS and cap and valid == {"en"} and cap[:1] != typed[:1] and sentence_lang(table, prevs, nxt) == "de"
                and last not in (None, BOS) and fold(last) in DETERMINERS and lean(table, nxt) != "e"):
            return cap
        return None
    if cap is None and known(table, personal, typed):
        return truecase()
    p = None if last is None else last if last == BOS else fold(last)
    zt = table.zipf(t)
    oov = OOV - OOV_PER_LETTER * max(0, len(t) - 4)
    lit = oov if zt == 0 or (RARE_AS_UNKNOWN and cap is None) else lm_score(table, bigrams, t, zt, p)
    n = fold(nxt) if NEXT_CONTEXT and nxt and nxt != BOS and nxt.isalpha() else None
    def right(w):  # log10 P(next | w) / P(next): what the word after says about w
        return 0.0 if n is None else lm_score(table, bigrams, n, table.zipf(n), w) - (table.zipf(n) - 9)
    if n is not None:
        lit += BACKOFF
    scored = [(lit + len(t) * MATCH, t)]
    # A capital in mid-sentence: a name or a noun; its first letter stays (Qwen is not "wen").
    keep_first = typed[0].isupper() and p != BOS
    if MULTI_UMLAUT:
        u = umlaut_form(table, t)
        if u and sum(a != b for a, b in zip(t, u)) >= 2 and table.candidate(u):
            scored.append((lm_score(table, bigrams, u, table.zipf(u), p) + channel(t, u), u))
    for c in one_slip(t):
        if (keep_first and BASE.get(c[0], c[0]) != t[0]) or (vocab is not None and c not in vocab) or not table.candidate(c):
            continue
        if FRONT_GUARD and len(c) != len(t) and BASE.get(c[0], c[0]) != t[0]:
            continue
        zc = table.zipf(c)
        if zc >= CAND_ZIPF:
            scored.append((lm_score(table, bigrams, c, zc, p) + channel(t, c) + right(c), c))
    if PERSONAL_TARGETS:
        slips = None
        for c, lang in cands:
            fc = fold(c)
            if lang is not None or fc == t or table.candidate(fc) or (keep_first and BASE.get(fc[0], fc[0]) != t[0]):
                continue
            slips = slips if slips is not None else set(one_slip(t))
            if fc in slips:
                scored.append((lm_score(table, bigrams, fc, PERSONAL_ZIPF, p) + channel(t, fc), fc))
    top, word = max(scored)
    total = sum(10 ** (s - top) for s, _ in scored)
    sure = 1.0 / total >= TAU
    if SPLIT_DECISION and not sure and word != t and 10 ** (scored[0][0] - top) / total < 1 - TAU:
        rest = sorted((s for s, c in scored[1:] if c != word), reverse=True)
        sure = not rest or top - rest[0] >= math.log10(RUNNER_UP)
    if word != t and sure:
        spelled = next((c for c, _ in cands if fold(c) == word), word)
        return spelled[0].upper() + spelled[1:] if typed[0].isupper() else spelled
    if COMPOUND_FIX and word == t and zt == 0 and len(t) >= COMPOUND_MIN and sentence_lang(table, prevs, nxt) != "en":
        fix = compound_fix(table, t)
        if fix:
            return fix[0].upper() + fix[1:] if typed[0].isupper() else fix
    tc = truecase()
    if tc:
        return tc
    if cap is None:
        return None
    if LANG_CAPS:
        if cap[:1] != typed[:1] and (sentence_lang(table, prevs, nxt) == "en" or
                                     (last not in (None, BOS) and fold(last) in PRONOUNS)):
            return None
        return cap
    return cap if word == t and sure else None


PAIRS = os.path.join(os.path.dirname(TABLE), "bigrams.bin")


def replay(text, vocab=None, personal=frozenset()):
    """Yields every change the keyboard would make to `text`, typed word by word:
    ("fix", typed, new) or ("space", "prev typed", "prev' typed'"). `vocab`:
    optional set of all words, only to make it faster."""
    table = Table(Bloom.load(open(TABLE, "rb").read()))
    bigrams = Bloom.load(open(PAIRS, "rb").read())
    seen = set()  # a fix undone once is learned: count each word once
    prev = BOS  # the committed word before; BOS at a sentence start, None after other punctuation
    for tok in re.findall(r"[A-Za-zÄÖÜäöüß]+|[^A-Za-zÄÖÜäöüß]+", text):
        if not tok[0].isalpha():
            if tok != " ":
                prev = BOS if re.search(r"[.!?\n:]", tok) else None
            continue
        if prev not in (None, BOS) and tok not in seen:
            fix = space_fix(table, personal, prev, tok)
            if fix:
                seen.add(tok)
                yield "space", prev + " " + tok, " ".join(fix)
                prev = fix[1]
                continue
        word = tok
        if fold(tok) not in seen:
            pick = correction(table, bigrams, personal, tok, prev=prev, vocab=vocab)
            if pick:
                seen.add(fold(tok))
                yield "fix", tok, pick
                word = pick
        prev = word


if __name__ == "__main__":
    for kind, a, b in replay(open(sys.argv[1]).read()):
        print(f"{kind:5} {a} -> {b}")
