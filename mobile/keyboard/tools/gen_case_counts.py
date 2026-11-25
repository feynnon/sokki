"""Sokki: how German text writes each word in mid-sentence, for truecasing (Lita et al. 2003;
the Moses truecaser): capitalised (Link, Spalte, Dinge) or lower case (schon, gut, super).

Streams German sentences (Leipzig's Wikipedia sample, CC BY-SA, and Tatoeba, CC BY 2.0 FR: the
same licence-clean text as the pair table), skips each sentence's first word, and counts per
folded word how often it starts with a capital. Writes {word: [capitalised, lower]} for words
seen at least MIN times.

    python gen_case_counts.py out.json sentences.txt [...]
SPDX-License-Identifier: GPL-3.0-only
"""

import bz2
import collections
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(__file__))
import typo_model as tm  # noqa: E402

MIN = 5
WORD = re.compile(r"[A-Za-zÄÖÜäöüß]+")


def sentences(path):
    opener = bz2.open if path.endswith(".bz2") else open
    with opener(path, "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if path.endswith(".bz2") and (len(parts) < 3 or parts[1] != "deu"):
                continue
            yield parts[-1]


def main(out, paths):
    up, low = collections.Counter(), collections.Counter()
    for path in paths:
        for s in sentences(path):
            # clause starts after . : ? ! inside the line are capitalised too: skip them
            for clause in re.split(r"[.!?:;\"„“»«()]\s*", s):
                ws = WORD.findall(clause)
                for w in ws[1:]:
                    if len(w) < 2 or w.isupper():
                        continue
                    (up if w[0].isupper() else low)[tm.fold(w)] += 1
    data = {w: [up[w], low[w]] for w in set(up) | set(low) if up[w] + low[w] >= MIN}
    json.dump(data, open(out, "w"), ensure_ascii=False)
    print(len(data), "words")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
