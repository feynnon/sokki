"""Sokki: JamSpell (github.com/bakwc/JamSpell, MIT), an open-source trigram spell
corrector, on the keyboard's typo test sets, scored exactly as score_context.py
scores its variants (same CAP-weighted sample, same tally and result line).

JamSpell's pick for a typed word is the first of GetCandidates(words, i), where
words are the up to 8 typed words before it (cut at a sentence end, as JamSpell's
own tokenizer cuts), the typed word, and the word after it; JamSpell itself only
looks at two words either side. Rows: "jamspell" (as is) and "jamspell, known
words kept" (never changes a word typo_model.known() accepts: strict mode).

    python score_jamspell.py text DIR              # DIR/train.txt (the corpora ngram_lm.py uses) + DIR/alphabet.txt
    python score_jamspell.py train DIR             # DIR/de-en.bin (+ .spell cache), with the training time
    python score_jamspell.py score MODEL set.jsonl ...

JamSpell's Python module is imported from JAMSPELL_PATH (default
~/sokki-typing-data/jamspell/pymod): the 0.0.12 sdist, swig 4 wrapper, compiled with
    clang++ -std=c++11 -O2 -fPIC -bundle -undefined dynamic_lookup -I. -Ijamspell -I<python include>
        jamspell_wrap.cpp jamspell/*.cpp contrib/cityhash/city.cc contrib/phf/phf.cc
(the setup.py build, with Python's own flags, crashes in TrainLangModel on arm64).
SPDX-License-Identifier: GPL-3.0-only
"""

import bz2
import os
import statistics
import sys
import time

sys.path.insert(0, os.path.dirname(__file__))
sys.path.insert(0, os.environ.get("JAMSPELL_PATH", os.path.expanduser("~/sokki-typing-data/jamspell/pymod")))
import jamspell  # noqa: E402

CORPORA = os.path.expanduser("~/sokki-typing-data/corpora")
SOURCES = ["deu-de_web_2021_1M", "deu_news_2023_1M", "eng-com_web-public_2018_1M", "eng_news_2023_1M"]
# ngram_lm.py's letters (a-z, äöüß, éèêàâçñ, the apostrophe inside "don't"), both cases
ALPHABET = "abcdefghijklmnopqrstuvwxyzäöüßéèêàâçñ'"
ENDS = ".?!"  # where JamSpell's tokenizer ends a sentence


def text(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "alphabet.txt"), "w", encoding="utf-8") as f:
        f.write(ALPHABET + "".join(c.upper() for c in ALPHABET if c != "ß") + "\n")
    paths = [os.path.join(CORPORA, s, f"{s}-sentences.txt") for s in SOURCES]
    paths += [os.path.join(CORPORA, f"tatoeba_{lang}.tsv.bz2") for lang in ("deu", "eng")]
    n = 0
    with open(os.path.join(out_dir, "train.txt"), "w", encoding="utf-8") as out:
        for path in paths:
            opener = bz2.open if path.endswith(".bz2") else open
            with opener(path, "rt", encoding="utf-8", errors="replace") as f:
                for line in f:
                    # JamSpell ends sentences only at . ? ! (not at a line end): close every one
                    out.write(line.rstrip("\n").split("\t")[-1] + " .\n")
                    n += 1
    print(f"{n:,} sentences")


def train(out_dir):
    c = jamspell.TSpellCorrector()
    model = os.path.join(out_dir, "de-en.bin")
    t0 = time.time()
    ok = c.TrainLangModel(os.path.join(out_dir, "train.txt"), os.path.join(out_dir, "alphabet.txt"), model)
    dt = time.time() - t0
    size = os.path.getsize(model) + os.path.getsize(model + ".spell")
    print(f"trained {ok} in {dt / 60:.1f} min; {model}: {os.path.getsize(model) / 1e6:.0f} MB"
          f" + .spell {os.path.getsize(model + '.spell') / 1e6:.0f} MB = {size / 1e6:.0f} MB")


def word_chars(corr, chars):
    """The characters the model's tokenizer keeps inside words (its alphabet), by probing:
    a nonsense word stays one word only when the character belongs to it."""
    pad = "qxzqxzqx"
    return {ch for ch in chars if " " not in corr.FixFragmentNormalized(pad + ch + pad)}


def tail(words, ok):
    """JamSpell's tokens of the words before, from the last sentence end on."""
    toks, cur = [], ""
    for w in words:
        for ch in w.lower() + " ":
            if ch in ok:
                cur += ch
                continue
            if cur:
                toks.append(cur)
                cur = ""
            if ch in ENDS:
                toks = []
    return toks


def head(word, ok):
    """JamSpell's first token of the word after, unless a sentence ends first."""
    cur = ""
    for ch in word.lower():
        if ch in ok:
            cur += ch
        elif cur or ch in ENDS:
            break
    return cur


def score(model, paths):
    import score_context as sc  # loads wordfreq and the keyboard's word table: after the module check
    tm = sc.tm
    corr = jamspell.TSpellCorrector()
    t0 = time.time()
    assert corr.LoadLangModel(model), model
    print(f"{model}: loaded in {time.time() - t0:.1f} s")
    sets = [(p, sc.load(p)) for p in paths]
    chars = {ch for _, items in sets for i in items for w in i.get("prev", []) + [i.get("next", "")] for ch in w.lower()}
    ok = word_chars(corr, chars)
    print(f"word characters besides letters: {''.join(sorted(c for c in ok if not c.isalpha())) or 'none'}")
    times = []
    for path, items in sets:
        own = []
        for i in items:
            typed = i["typed"]
            prev = tail(i.get("prev", []), ok)
            nxt = head(i.get("next", ""), ok)
            words = prev + [typed.lower()] + ([nxt] if nxt else [])
            s = time.perf_counter()
            cands = corr.GetCandidates(words, len(prev))
            own.append(time.perf_counter() - s)
            pick = cands[0] if cands else None
            i["js"] = None if pick is None or tm.fold(pick) == tm.fold(typed) else pick
            i["known"] = tm.known(sc.TABLE, set(), typed)
        times += own
        hs = [{"typed": i["typed"], "meant": i["meant"], "w": i.get("weight", 1), "js": i["js"], "known": i["known"]} for i in items]
        print(f"\n{os.path.basename(path)}: {len(hs)} words")
        for name, pick in (("jamspell", lambda h: h["js"]),
                           ("jamspell, known words kept", lambda h: None if h["known"] else h["js"])):
            n, ex = sc.tally(hs, pick)
            print(sc.line(name, n))
            for kind in ("changed", "wrong"):
                if ex[kind]:
                    print(f"      {kind}: {', '.join(ex[kind])}")
        own.sort()
        print(f"      GetCandidates: median {1e3 * statistics.median(own):.2f} ms, p95 {1e3 * own[int(0.95 * len(own))]:.2f} ms")
    times.sort()
    print(f"\nGetCandidates over all {len(times)} calls: median {1e3 * statistics.median(times):.2f} ms,"
          f" p95 {1e3 * times[int(0.95 * len(times))]:.2f} ms, max {1e3 * times[-1]:.1f} ms")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "text":
        text(sys.argv[2])
    elif cmd == "train":
        train(sys.argv[2])
    else:
        score(sys.argv[2], sys.argv[3:])
