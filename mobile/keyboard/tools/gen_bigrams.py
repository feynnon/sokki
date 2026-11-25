"""Sokki: writes assets/sokki/bigrams.bin, the word-pair table for Typos' score.

Every pair of folded words seen at least MIN_COUNT times in an ngram_lm.py
model ("<s>" marks a sentence start), with log10 P(word | previous word) in
half steps, stored like words.bin as a Bloom filter: key "<b>:<prev> <word>",
b = floor(2 * (log10 P + 8)) (1..16). Pairs seen fewer times lose almost
nothing (score_context.py, 2026-11-20).

The shipped table (2026-11-24) comes from Wikipedia itself and Tatoeba:
1M sentences each sampled from one part of the German and the English
pages-articles dump of 2026-11-01 (dumps.wikimedia.org; wiki_sentences.py,
CC BY-SA 4.0) and Tatoeba's German and English sentences (CC BY 2.0 FR):
1.17M pairs, 4.2 MB. On typing37k and 136m it fixes as many typos as the
earlier table from Leipzig's Wikipedia samples (14.9% vs 14.7%, 13.1% vs
13.0%, same wrong fixes, no correct word changed; score_context.py, CAP 3000).
Leipzig's news and web corpora scored about 1.3 points better, but their terms
are unclear (CC BY in one place, non-commercial in another).

    python wiki_sentences.py dewiki-20261101-pages-articles1.xml-p1p297012.bz2 de-wiki-1M.txt 1000000
    python wiki_sentences.py enwiki-20261101-pages-articles1.xml-p1p41242.bz2 en-wiki-1M.txt 1000000
    python ngram_lm.py build lm-wikidump-tatoeba.npz de-wiki-1M.txt en-wiki-1M.txt \
        tatoeba_deu.tsv.bz2 tatoeba_eng.tsv.bz2
    python gen_bigrams.py lm-wikidump-tatoeba.npz [out.bin]
SPDX-License-Identifier: GPL-3.0-only
"""

import math
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
import ngram_lm  # noqa: E402
import typo_model as tm  # noqa: E402

OUT = os.path.join(os.path.dirname(__file__), "..", "src", "main", "assets", "sokki", "bigrams.bin")
MIN_COUNT = 5
FALSE_HITS = 1e-6  # 16 bucket probes per pair: 1e-4 let 0.14% of non-pairs through ("tay id")


def main(lm_path, out=OUT):
    z = np.load(lm_path, allow_pickle=True)
    words = ["<s>", "<unk>"] + list(z["vocab"])
    uni = z["uni"]
    keys, counts = z["bi_keys"], z["bi_counts"]
    keep = counts >= MIN_COUNT
    keys, counts = keys[keep], counts[keep]
    a, b = keys // ngram_lm.V, keys % ngram_lm.V
    ok = (a != ngram_lm.UNK) & (b != ngram_lm.UNK)
    a, b, counts = a[ok], b[ok], counts[ok]
    lp = np.log10(counts / uni[a])
    bloom = tm.Bloom(len(a), FALSE_HITS)
    for i, j, p in zip(a.tolist(), b.tolist(), lp.tolist()):
        bloom.add(f"{tm.bigram_bucket(p)}:{words[i]} {words[j]}")
    data = bloom.dump()
    with open(out, "wb") as f:
        f.write(data)
    print(out, len(a), "pairs,", len(data), "bytes, k =", bloom.k)


if __name__ == "__main__":
    main(*sys.argv[1:3])
