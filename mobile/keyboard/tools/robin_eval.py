"""Sokki: autocorrect measured on Robin's own writing, as the phone runs it (typo_model.correction).

Data (~/sokki-typing-data, private): robin-texts-*.txt (his messages), robin-real-typos-labels.json
(every word the keyboard doesn't know, labelled typo -> meant or intended), phone/journal-*.jsonl (text
sent from the phone) with phone/journal-labels.json, and the keyboard's word lists in dicts/.

What the phone's dictionary tells autocorrect is emulated from those word lists: the typed word is
valid (decoder dictionaries of the active languages, or a word Robin typed before), and the
dictionary's spellings of nearby words (Montag, Straße).

Measured per test set:
  typos     his real typos: fixed right / fixed wrong (to another word) / missed
  false     correct words changed (to another word), and wrong capitals (allen -> Allen)
  nouns     words he typed lower case that only exist capitalised (woche -> Woche): share capitalised
  umlauts   his correct umlaut words with the umlauts left out (konnen): share restored
  public    typing37k / 136m samples: right / wrong / correct words changed
  latency   Python twin time per word (relative; the device bench gives real numbers)

    ~/.cache/sokki-eval-venv/bin/python mobile/keyboard/tools/robin_eval.py [variant ...]
Variants are feature switches in typo_model (VARIANTS below). Keep jobs small: one process, ~1 GB.
SPDX-License-Identifier: GPL-3.0-only
"""

import collections
import json
import os
import random
import re
import statistics
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
import typo_model as tm  # noqa: E402

D = os.path.expanduser("~/sokki-typing-data/")
WORD = re.compile(r"[A-Za-zÄÖÜäöüß]+")
NO_NEXT = os.environ.get("NO_NEXT") == "1"  # as the phone decides: before the next word is typed
UML = str.maketrans({"ä": "a", "ö": "o", "ü": "u", "Ä": "A", "Ö": "O", "Ü": "U"})


def wordlist(name, freqs=False):
    out = {} if freqs else set()
    for line in open(D + "dicts/" + name, encoding="utf-8"):
        if line.startswith(" word="):
            w = line[6:line.index(",")]
            if freqs:
                f = int(re.search(r",f=(\d+)", line).group(1))
                out[w] = max(out.get(w, 0), f)
            else:
                out.add(w)
    return out


class Lexicon:
    """The phone's dictionaries for the active languages, and the user's own words."""

    def __init__(self, langs, personal=(), protect=False):
        files = {"de": ["main_de__legacy.combined"], "en": ["main_en_US.combined"]}
        self.exact = set()
        for lang in langs:
            for f in files[lang]:
                self.exact |= wordlist(f)
        self.personal = set(personal)
        self.personal_by_fold = collections.defaultdict(set)
        for w in self.personal:
            self.personal_by_fold[tm.fold(w)].add(w)
        # the big word lists (Leipzig German, British/US English): valid words, not suggestions
        self.protect = {}  # word -> language
        if protect:
            for f, lang in (("main_en_GB__experimental.combined", "en"), ("main_en_US__experimental.combined", "en"),
                            ("main_de.combined", "de")):
                fr = wordlist(f, freqs=True)
                for w, c in fr.items():
                    if w.isupper() and len(w) > 1:
                        continue
                    # a lower-case form much rarer than the capitalised one is a stray (zugriff); an
                    # English word German has capitalised is an English quote (link, performance)
                    if lang == "de" and w[:1].islower() and w[0].upper() + w[1:] in fr and (
                            fr[w[0].upper() + w[1:]] > c + 30 or self.english(w)):
                        continue
                    self.protect[w] = lang
        self.forms = collections.defaultdict(set)  # folded -> (dictionary spelling, language)
        for lang in langs:
            for f in files[lang]:
                for w in wordlist(f):
                    self.forms[tm.fold(w)].add((w, lang))

    english = None  # set by main: is a word common in English (the word table's e: keys)

    def langs(self, typed):
        """The languages whose dictionaries know the typed word as is ("user": his history)."""
        low = typed[0].lower() + typed[1:]
        out = {lang for f, lang in self.forms.get(tm.fold(typed), ()) if f == typed or (typed[0].isupper() and f == low)}
        for w in (typed, low if typed[0].isupper() else None):
            if w and w in self.protect:
                out.add(self.protect[w])
        if typed in self.personal or (typed[0].isupper() and low in self.personal):
            out.add("user")
        return out

    def valid(self, typed, start):
        """The keyboard knows the typed word as is: its dictionaries, the protection list, or the
        user's history (a lower-case history word whose German dictionary form is capitalised
        doesn't count: the history learned a noun typed in lower case)."""
        low = typed[0].lower() + typed[1:]
        if typed in self.exact or (typed[0].isupper() and low in self.exact):
            return True
        if typed in self.protect or (typed[0].isupper() and low in self.protect):
            return True
        if typed in self.personal or (typed[0].isupper() and low in self.personal):
            return not (typed.islower() and any(f[:1].isupper() and lang == "de" for f, lang in self.forms.get(tm.fold(typed), ())))
        return False

    def candidates(self, typed):
        """Dictionary spellings of the typed word and of every word one slip away (the decoder's list),
        and the user's own words one slip away (the history dictionary's, with no language)."""
        t = tm.fold(typed)
        out = []
        for w in [t] + tm.one_slip(t):
            out += sorted(self.forms.get(w, ()))
            out += sorted((p, None) for p in self.personal_by_fold.get(w, ()) if w != t)
        return out


def case_truth():
    """Lower-case forms any dictionary knows, and forms only known capitalised (nouns, names)."""
    lower, upper = set(), set()
    for f in ("main_de__legacy.combined", "main_de.combined", "main_en_US.combined"):
        for w in wordlist(f):
            (lower if w[:1].islower() else upper).add(w)
    return lower, upper


def tokens(text):
    """(word, previous word or BOS, next word) for every word, sentence by sentence. Words with an
    apostrophe (wasn't, geht's) are left out: the keyboard sees them whole."""
    out = []
    for sent in re.split(r"[.!?\n:;]+", text):
        ws = re.findall(r"[A-Za-zÄÖÜäöüß]+(?:'[A-Za-z]+)?", sent)
        for i, w in enumerate(ws):
            if "'" not in w:
                out.append((w, ws[i - 1] if i else tm.BOS, ws[i + 1] if i + 1 < len(ws) else "",
                            ([tm.BOS] + ws[:i])[-3:]))
    return out


def load_robin():
    text = open(D + "robin-texts-2026-11-19.txt").read()
    labels = {e["word"]: e for e in json.load(open(D + "robin-real-typos-labels.json"))}
    journal = []
    for f in sorted(os.listdir(D + "phone")):
        if f.startswith("journal-") and f.endswith(".jsonl"):
            journal += [json.loads(x).get("text", "") for x in open(D + "phone/" + f)]
    jl = D + "phone/journal-labels.json"
    jlabels = json.load(open(jl)) if os.path.exists(jl) else {}
    return tokens(text), labels, tokens("\n".join(journal)), jlabels


def personal_words(toks, labels):
    """Words Robin typed at least twice that aren't labelled typos: the phone's history has them."""
    n = collections.Counter(t[0] for t in toks)
    return {w for w, c in n.items() if c >= 2 and labels.get(w, {}).get("label") != "typo"}


def sample(path, n_typo, n_ok, seed=1):
    random.seed(seed)
    typo, ok, nt, no = [], [], 0, 0
    for line in open(path):
        x = json.loads(line)
        if not (x["typed"].isalpha() and x["meant"].isalpha()):
            continue
        group, k = (typo, n_typo) if x["typed"] != x["meant"] else (ok, n_ok)
        if x["typed"] != x["meant"]:
            nt += 1
            c = nt
        else:
            no += 1
            c = no
        if len(group) < k:
            group.append(x)
        elif random.random() < k / c:
            group[random.randrange(k)] = x
    return typo + ok


def run(lex, table, bigrams, toks, labels, umlaut_words, lower, upper, timings, truth=None):
    """Score one test set of Robin's writing. `truth`: per word, should it be capitalised
    (caps_truth.py; None outside German sentences)."""
    n = collections.Counter()
    ex = collections.defaultdict(list)
    for k, (w, prev, nxt, prevs) in enumerate(toks):
        lab = labels.get(w)
        t0 = time.perf_counter()
        p = tm.correction(table, bigrams, set(), w, lex.candidates(w) if len(w) >= tm.MIN_LEN else (),
                          lex.langs(w), prevs, nxt=None if NO_NEXT else nxt)
        timings.append(time.perf_counter() - t0)
        out = p or w
        if lab and lab["label"] == "typo" and lab.get("kind") != "space":
            m = lab["meant"]
            n["typos"] += 1
            if tm.fold(out) == tm.fold(m) and out != w:
                n["typo right"] += 1
            elif tm.fold(out) != tm.fold(w):
                n["typo wrong"] += 1
                ex["typo wrong"].append(f"{prev} [{w}->{out}] ({m})")
            else:
                n["typo missed"] += 1
                ex["typo missed"].append(f"{prev} [{w}] ({m})")
            continue
        if lab and lab["label"] in ("unsure",) or (lab and lab.get("kind") == "space"):
            continue
        n["words"] += 1
        if tm.fold(out) != tm.fold(w):
            n["false"] += 1
            ex["false"].append(f"{prev} [{w}->{out}] {nxt}")
        cap = truth[k] if truth and k < len(truth) else None
        if cap is not None and w.islower() and prev != tm.BOS:
            if cap:
                n["nouns"] += 1
                if out[:1].isupper():
                    n["nouns capitalised"] += 1
                else:
                    ex["noun lower"].append(f"{prev} [{w}] {nxt}")
            elif out[:1].isupper():
                n["caps wrong"] += 1
                ex["caps wrong"].append(f"{prev} [{w}->{out}] {nxt}")
    for w, prev, nxt, prevs in umlaut_words:
        s = w.translate(UML)
        p = tm.correction(table, bigrams, set(), s, lex.candidates(s), lex.langs(s), prevs, nxt=None if NO_NEXT else nxt)
        n["umlauts"] += 1
        if p == w or (p and p.lower() == w.lower()):
            n["umlauts restored"] += 1
        elif p:
            n["umlauts wrong"] += 1
            ex["umlaut wrong"].append(f"[{s}->{p}] ({w})")
        else:
            ex["umlaut missed"].append(f"{prev} [{s}] ({w})")
    return n, ex


def public(table, bigrams, lex, name, path):
    n = collections.Counter()
    for x in sample(path, 1500, 2500):
        typed, meant = x["typed"], x["meant"]
        prev = x["prev"][-1] if x["prev"] else tm.BOS
        prevs = ([tm.BOS] + x["prev"])[-3:] if len(x["prev"]) < 8 else x["prev"][-3:]
        p = tm.correction(table, bigrams, set(), typed, lex.candidates(typed) if len(typed) >= tm.MIN_LEN else (),
                          lex.langs(typed), prevs, nxt=None if NO_NEXT else x.get("next", ""))
        out = p or typed
        if typed == meant:
            n["ok"] += 1
            n["changed"] += tm.fold(out) != tm.fold(typed)
        else:
            n["typos"] += 1
            n["right" if tm.fold(out) == tm.fold(meant) else "wrong" if p and tm.fold(out) != tm.fold(typed) else
              "missed"] += 1
    return n


OLD = {"SWAP_BOOST": 1.0, "UMLAUT": None, "STRICT_DENGLISH": False, "RARE_AS_UNKNOWN": False, "CURATED_KNOWN": False,
       "VERB_STEMS": False, "LANG_CAPS": False, "TRUECASE": False, "INFLECTIONS": False, "OOV_PER_LETTER": 0.0,
       "UMLAUT_IN_GERMAN": False, "NOMINALISED": False, "COMPOUND_FIX": False,
       "SHARED_NOUNS": False, "EN_IN_GERMAN": False,
       "TRANSLIT": False, "SS_ESZETT": False, "CAP_SPELLING": False, "MULTI_UMLAUT": False}
VARIANTS = {"before the loop (German+English)": OLD, "before, phone in English only": dict(OLD, langs=("en",)),
            "now": {}, "+ split decision x2": {"SPLIT_DECISION": True, "RUNNER_UP": 2.0},
            "+ split decision x5": {"SPLIT_DECISION": True, "RUNNER_UP": 5.0},
            "now, no compound fixes": {"COMPOUND_FIX": False}, "+ learned words as targets": {"PERSONAL_TARGETS": True},
            "now, no capitals after adjectives": {"SHARED_NOUNS": False}, "+ next word in the score": {"NEXT_CONTEXT": True},
            "now, English words always kept": {"EN_IN_GERMAN": False},
            "now, no umlaut/ß helpers": {"TRANSLIT": False, "SS_ESZETT": False, "CAP_SPELLING": False, "MULTI_UMLAUT": False},
            "+ next word in the score, split x5": {"NEXT_CONTEXT": True, "SPLIT_DECISION": True}, "now, no truecasing": {"TRUECASE": False}, "now, no rare=unknown": {"RARE_AS_UNKNOWN": False},
            "longer non-words less likely 0.25": {"OOV_PER_LETTER": 0.25},
            "longer non-words less likely 0.5": {"OOV_PER_LETTER": 0.5},
            "longer non-words less likely 0.75": {"OOV_PER_LETTER": 0.75},
            "+ inflections": {"INFLECTIONS": True},
            "+ inflections, longer 0.25": {"INFLECTIONS": True, "OOV_PER_LETTER": 0.25},
            "+ inflections, longer 0.5": {"INFLECTIONS": True, "OOV_PER_LETTER": 0.5},
            "+ inflections, longer 0.25, front guard": {"INFLECTIONS": True, "OOV_PER_LETTER": 0.25, "FRONT_GUARD": True},
            "+ inflections, longer 0.35, front guard": {"INFLECTIONS": True, "OOV_PER_LETTER": 0.35, "FRONT_GUARD": True},
            "+ umlaut twins in German": {"UMLAUT_IN_GERMAN": True}}


def main(names):
    table = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
    bigrams = tm.Bloom.load(open(tm.PAIRS, "rb").read())
    toks, labels, jtoks, jlabels = load_robin()
    lower, upper = case_truth()
    personal = personal_words(toks + jtoks, labels)
    random.seed(2)
    umlaut_words = [x for x in toks + jtoks if re.search("[äöüÄÖÜ]", x[0]) and len(x[0]) >= 3]
    umlaut_words = random.sample(umlaut_words, min(1500, len(umlaut_words)))
    Lexicon.english = staticmethod(lambda w: table.common_in("e", tm.fold(w)))
    for name in names:
        flags = VARIANTS[name]
        saved = {k: getattr(tm, k) for k in flags if k not in ("langs", "protect")}
        for k, v in flags.items():
            if k not in ("langs", "protect"):
                setattr(tm, k, v)
        lex = Lexicon(flags.get("langs", ("de", "en")), personal, flags.get("protect", False))
        timings = []
        truth = json.load(open(D + "caps-truth.json")) if os.path.exists(D + "caps-truth.json") else {}
        n, ex = run(lex, table, bigrams, toks, labels, umlaut_words, lower, upper, timings, truth.get("text"))
        nj, exj = run(lex, table, bigrams, jtoks, jlabels, [], lower, upper, timings, truth.get("journal"))
        pub = {k: public(table, bigrams, Lexicon(("de", "en")), k, D + f"public/{k}-ctx.jsonl")
               for k in ("typing37k", "136m")}
        for k, v in saved.items():
            setattr(tm, k, v)
        pct = lambda a, b: f"{100 * a / b:5.1f}%" if b else "  -  "
        ts = sorted(timings)
        line = (f"{name:28} typos {pct(n['typo right'] + nj['typo right'], n['typos'] + nj['typos'])} right "
                f"{pct(n['typo wrong'] + nj['typo wrong'], n['typos'] + nj['typos'])} wrong | false "
                f"{n['false'] + nj['false']} | nouns capitalised "
                f"{pct(n['nouns capitalised'] + nj['nouns capitalised'], n['nouns'] + nj['nouns'])}, wrong caps "
                f"{n['caps wrong'] + nj['caps wrong']} | umlauts "
                f"{pct(n['umlauts restored'], n['umlauts'])} ({n['umlauts wrong']} wrong) | "
                + " ".join(f"{k} {pct(v['right'], v['typos'])}/{pct(v['wrong'], v['typos'])}/{v['changed']}ch"
                           for k, v in pub.items())
                + f" | {1000 * statistics.median(ts):.2f}/{1000 * ts[int(.95 * len(ts))]:.2f} ms")
        line = ("[no next word] " if NO_NEXT else "") + line
        print(line, flush=True)
        with open(D + "loop-log.md", "a") as f:
            f.write(f"- {time.strftime('%m-%d %H:%M')} {line}\n")
        json.dump({k: v[:60] for k, v in list(ex.items()) + [("journal " + k, v) for k, v in exj.items()]},
                  open(D + f"eval-examples-{name}.json", "w"), ensure_ascii=False, indent=0)


if __name__ == "__main__":
    main(sys.argv[1:] or list(VARIANTS))
