"""Sokki: a word trigram model for context-aware autocorrect experiments.

Counts bigrams and trigrams of folded words (lower case, ß -> ss) in plain
sentences, keeps those seen at least MIN_COUNT times, and scores a word in its
context with stupid backoff (Brants et al. 2007): the trigram's share of its
context's count, else 0.4 x the bigram's, else 0.4 x the unigram. Unigrams come
from wordfreq (the larger of German and English), so colloquial words keep
their weight. One model holds both languages: the context picks the language.

    python ngram_lm.py build lm.npz corpus.txt ...   # "id<TAB>sentence" (Leipzig) or "id<TAB>lang<TAB>sentence" (Tatoeba)
    python ngram_lm.py size lm.npz                   # n-gram counts and bytes in a phone-sized table

Corpora used for the 2026-11 experiments (eval set text-corpora, docs/evals.md): Leipzig
deu-de_web_2021_1M, deu_news_2023_1M, eng-com_web-public_2018_1M,
eng_news_2023_1M, deu_wikipedia_2021_1M, eng_wikipedia_2016_1M, and Tatoeba's
German and English sentences (CC BY 2.0 FR). The shipped pair table uses only
Wikipedia and Tatoeba (gen_bigrams.py says why). Building from 4M sentences
takes about 1 minute and a few GB: keep it to one job at a time.
SPDX-License-Identifier: GPL-3.0-only
"""

import array
import bz2
import collections
import re
import sys

import numpy as np

V = 1 << 21  # ids fit 21 bits: a trigram key fits an int64
BOS, UNK = 0, 1
MIN_COUNT = 2
BACKOFF = 0.4
TOKEN = re.compile(r"[a-zäöüéèêàâçñ]+(?:'[a-z]+)?")


def fold(w):
    return w.lower().replace("ß", "ss")


def words(sentence):
    return TOKEN.findall(fold(sentence))


def sentences(path):
    opener = bz2.open if path.endswith(".bz2") else open
    with opener(path, "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            yield line.rstrip("\n").split("\t")[-1]


def build(out, paths):
    counts = collections.Counter()
    for path in paths:
        for s in sentences(path):
            counts.update(words(s))
    vocab = [w for w, c in counts.most_common() if c >= MIN_COUNT][: V - 2]
    ids = {w: i + 2 for i, w in enumerate(vocab)}
    seq = array.array("i")
    for path in paths:
        for s in sentences(path):
            ws = words(s)
            if ws:
                seq.append(BOS)
                seq.extend(ids.get(w, UNK) for w in ws)
    a = np.frombuffer(seq, dtype=np.int32).astype(np.int64)
    uni = np.bincount(a, minlength=len(vocab) + 2)
    # n-grams inside one sentence: no BOS after the first position
    bi = a[:-1] * V + a[1:]
    bi = bi[a[1:] != BOS]
    tri = (a[:-2] * V + a[1:-1]) * V + a[2:]
    tri = tri[(a[1:-1] != BOS) & (a[2:] != BOS)]
    arrays = {"vocab": np.array(vocab, dtype=object), "uni": uni}
    for name, keys in (("bi", bi), ("tri", tri)):
        k, c = np.unique(keys, return_counts=True)
        keep = c >= MIN_COUNT
        arrays[name + "_keys"], arrays[name + "_counts"] = k[keep], c[keep].astype(np.int32)
    np.savez(out, **arrays)


class LM:
    def __init__(self, path, unigram=None, order=3):
        z = np.load(path, allow_pickle=True)
        self.ids = {w: i + 2 for i, w in enumerate(z["vocab"])}
        self.uni = z["uni"]
        self.bi_k, self.bi_c = z["bi_keys"], z["bi_counts"]
        self.tri_k, self.tri_c = z["tri_keys"], z["tri_counts"]
        self.order = order
        self.unigram = unigram  # word -> probability (wordfreq); None: corpus counts
        self.total = int(self.uni[2:].sum())

    @staticmethod
    def _count(keys, counts, key):
        i = np.searchsorted(keys, key)
        return int(counts[i]) if i < len(keys) and keys[i] == key else 0

    def p_unigram(self, w):
        if self.unigram is not None:
            p = self.unigram.get(w, 0.0)
            if p:
                return p
        i = self.ids.get(w)
        return (self.uni[i] / self.total if i else 0.0) or 1e-9

    def score(self, w, prev=()):
        """Stupid-backoff score of folded word `w` after folded words `prev` (reading
        order; "<s>" for a sentence start)."""
        wid = self.ids.get(w)
        ctx = [BOS if p == "<s>" else self.ids.get(p) for p in prev[-(self.order - 1):]] if self.order > 1 else []
        weight = 1.0
        if wid is not None:
            if len(ctx) == 2 and ctx[0] is not None and ctx[1] is not None:
                c = self._count(self.tri_k, self.tri_c, (ctx[0] * V + ctx[1]) * V + wid)
                if c:
                    return c / self._count(self.bi_k, self.bi_c, ctx[0] * V + ctx[1])
                weight = BACKOFF
            if ctx and ctx[-1] is not None:
                c = self._count(self.bi_k, self.bi_c, ctx[-1] * V + wid)
                if c:
                    return weight * c / self.uni[ctx[-1]]
                weight *= BACKOFF
        return weight * self.p_unigram(w)


def size(path):
    z = np.load(path, allow_pickle=True)
    nv, nb, nt = len(z["vocab"]), len(z["bi_keys"]), len(z["tri_keys"])
    print(f"vocabulary {nv:,}, bigrams {nb:,}, trigrams {nt:,}")
    for m in (2, 3, 5, 10):
        b, t = int((z["bi_counts"] >= m).sum()), int((z["tri_counts"] >= m).sum())
        # 4-byte fingerprint + 1-byte quantised score per n-gram
        print(f"  count >= {m}: {b:,} bigrams, {t:,} trigrams, ~{(b + t) * 5 / 1e6:.0f} MB as 5-byte entries")


if __name__ == "__main__":
    if sys.argv[1] == "build":
        build(sys.argv[2], sys.argv[3:])
    else:
        size(sys.argv[2])
