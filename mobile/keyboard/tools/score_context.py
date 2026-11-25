"""Sokki: scores autocorrect variants (the noisy channel) on typed/meant pairs with context.

Input: .jsonl pairs with "typed", "meant", "prev" (up to 8 typed words before,
reading order), "next" (public_pairs.py, synth_typos.py). Each source is
scored on its own: typos fixed right / wrong / missed, and correct words changed.

The noisy channel (Fowler et al. 2015; Ouyang et al. 2017): every word within
two slips of the typed one, and the typed word itself, gets
    log10 P_LM(word | context) + LAMBDA * log10 P(typed | word)
P_LM: ngram_lm.py's trigram model with stupid backoff (or its bigram/unigram).
P(typed | word): typo_model.channel (per-letter slip rates from typing37k).
The typed word as is, when it is no word: log10 P_OOV. The scores are turned
into shares (a posterior); the best word replaces the typed one only when it is
not the typed word and its share is at least TAU. Words autocorrect may not
change (known words: zipf >= 3, compounds) stay as typed in "strict" mode.
The keyboard runs one setting of this: typo_model.correction / Typos.kt
(bigram, weight 1, P_OOV -10, TAU 0.95, strict).

    python score_context.py lm.npz typing37k-ctx.jsonl 136m-ctx.jsonl robin-typos-ctx.jsonl robin-words-ctx.jsonl
SPDX-License-Identifier: GPL-3.0-only
"""

import collections
import json
import math
import os
import random
import sys
from concurrent.futures import ProcessPoolExecutor

from symspellpy import SymSpell, Verbosity
from wordfreq import get_frequency_dict

sys.path.insert(0, os.path.dirname(__file__))
import ngram_lm  # noqa: E402
import typo_model as tm  # noqa: E402

CAP = int(os.environ.get("CAP", 10_000))  # typos and correct words per source
CAND_ZIPF = 2.0  # candidates: words at least this common (233k words in German + English)

channel = tm.channel


FREQ = {}
for _lang in ("de", "en"):
    for _w, _f in get_frequency_dict(_lang, "large").items():
        if _f > FREQ.get(_w, 0):
            FREQ[_w] = _f
TABLE = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
WORDS = set(FREQ)
_state = {}


def _init(lm_path):
    sym = SymSpell(max_dictionary_edit_distance=2, prefix_length=7)
    for w, f in FREQ.items():
        if w.isalpha() and math.log10(f) + 9 >= CAND_ZIPF:
            sym.create_dictionary_entry(w, int(f * 1e9) + 1)
    _state["sym"] = sym
    _state["lm"] = ngram_lm.LM(lm_path, unigram=FREQ)


def context(prev):
    ws = [tm.fold(w) for w in prev]
    return (["<s>"] + ws if len(prev) < 8 else ws)[-2:]


def hypotheses(item):
    """Everything the variants need for one pair, computed once: per hypothesis
    (word, LM scores by order 1/2/3, right-context score, channel)."""
    typed, prev, nxt = item["typed"], item.get("prev", []), item.get("next", "")
    sym, lm = _state["sym"], _state["lm"]
    t = tm.fold(typed)
    ctx = context(prev)
    nf = tm.fold(nxt) if nxt.isalpha() else ""
    words = [t] + sorted({s.term for s in sym.lookup(t, Verbosity.ALL, max_edit_distance=2)} - {t})
    out = []
    for w in words:
        listed = w in FREQ or w in lm.ids
        lms = []
        for order in (1, 2, 3):
            lm.order = order
            lms.append(math.log10(lm.score(w, ctx)) if listed else None)
        lm.order = 3
        right = math.log10(lm.score(nf, ctx[-1:] + [w])) if nf else 0.0
        out.append((w, lms, right, channel(t, w) if w != t else len(t) * tm.MATCH))
    return {"typed": typed, "meant": item["meant"], "w": item.get("weight", 1), "prev": prev, "next": nxt, "known": tm.known(TABLE, set(), typed),
            "hyps": out}


def decide(h, order=3, lam=1.0, p_oov=-7.0, tau=0.9, right=False, strict=True, beta=1.0, min_len=3):
    typed = h["typed"]
    if len(typed) < min_len or not typed.isalpha() or any(c.isupper() for c in typed[1:]):
        return None
    if strict and h["known"]:
        return None
    scores = []
    for w, lms, r, ch in h["hyps"]:
        lmv = lms[order - 1]
        if w == h["hyps"][0][0]:  # the typed word itself
            s = p_oov if lmv is None else lmv + (beta if h["known"] else 0.0)
        elif lmv is None:
            continue
        else:
            s = lmv
        scores.append((s + lam * ch + (r if right else 0.0), w))
    top = max(scores)
    if top[1] == h["hyps"][0][0]:
        return None
    share = 1.0 / sum(10 ** (s - top[0]) for s, _ in scores)
    return top[1] if share >= tau else None


def tally(hs, pick):
    n = collections.Counter()
    ex = collections.defaultdict(list)
    for h in hs:
        p = pick(h)
        typed, meant, w = h["typed"], h["meant"], h["w"]
        if typed == meant:
            kind = "changed" if p and tm.fold(p) != tm.fold(typed) else "kept"
        elif p is None:
            kind = "missed"
        elif tm.fold(p) == tm.fold(meant):
            kind = "right"
        else:
            kind = "wrong"
        n[kind] += w
        if kind in ("changed", "wrong") and len(ex[kind]) < 6:
            ex[kind].append(f"{typed}->{p}" + ("" if typed == meant else f" ({meant})"))
    return n, ex


def line(name, n):
    typos = n["right"] + n["wrong"] + n["missed"]
    words = n["kept"] + n["changed"]
    pct = lambda k, tot: f"{100 * n[k] / tot:5.1f}%" if tot else "    -"
    per10k = f"{1e4 * n['changed'] / words:5.1f}" if words else "    -"
    return (f"  {name:34} right {pct('right', typos)} wrong {pct('wrong', typos)} missed {pct('missed', typos)}"
            f" | changed per 10k correct words {per10k}")


def load(path):
    items = [json.loads(x) for x in open(path)]
    items = [i for i in items if i["typed"].isalpha() and i["meant"].isalpha() and not i.get("partial")]
    random.seed(1)
    typos = [i for i in items if i["typed"] != i["meant"]]
    correct = [i for i in items if i["typed"] == i["meant"]]
    for group in (typos, correct):
        if len(group) > CAP:
            k = len(group) / CAP
            group[:] = random.sample(group, CAP)
            for i in group:
                i["weight"] = i.get("weight", 1) * k
    return typos + correct


VARIANTS = {
    "keyboard (bigram, strict)": lambda h: decide(h, order=2),
    "unigram, tau .9": lambda h: decide(h, order=1),
    "bigram, tau .9": lambda h: decide(h, order=2),
    "trigram, tau .9": lambda h: decide(h, order=3),
    "trigram + next word, tau .9": lambda h: decide(h, order=3, right=True),
}


def main(lm_path, paths):
    with ProcessPoolExecutor(10, initializer=_init, initargs=(lm_path,)) as pool:
        for path in paths:
            items = load(path)
            hs = list(pool.map(hypotheses, items, chunksize=200))
            json.dump(hs, open(path + ".hyps.json", "w"))
            print(f"\n{os.path.basename(path)}: {len(hs)} words")
            for name, pick in VARIANTS.items():
                n, ex = tally(hs, pick)
                print(line(name, n))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
