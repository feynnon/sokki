"""de-clean-10k: 10,000 edited German sentences for counting wrong autocorrect edits.

Edited text has almost no typos, so nearly every change autocorrect makes to it is wrong.
A third each from the Leipzig corpora deu_news_2023_1M, deu-de_web_2021_1M and
deu_wikipedia_2021_1M (eval set text-corpora; fetch_evals.py text-corpora). Within news and web: one sentence per
source website before any site gets a second (variety); Wikipedia has no source list, so random.
Kept: 6-35 words, ends in . ! or ?, at least 85% letters and spaces, no URL, no bullet.
Seed 1. Run: python3 make_de_clean.py > de-clean-10k.jsonl
The published copy: https://huggingface.co/datasets/Shannonigan/de-clean-10k (docs/evals.md).
SPDX-License-Identifier: GPL-3.0-only
"""
import collections, json, os, random, re, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import evals  # noqa: E402

C = evals.path("text-corpora") + "/"
CORPORA = [("news", "deu_news_2023_1M"), ("web", "deu-de_web_2021_1M"), ("wikipedia", "deu_wikipedia_2021_1M")]
N = [3334, 3333, 3333]


def ok(s):
    w = s.split()
    if not (6 <= len(w) <= 35) or s[-1] not in ".!?" or "http" in s or "www." in s or "•" in s:
        return False
    letters = sum(ch.isalpha() or ch == " " for ch in s)
    return letters / len(s) >= 0.85


def domain(url):
    m = re.match(r"https?://(?:www\.)?([^/]+)", url)
    return m.group(1) if m else url


random.seed(1)
for (name, c), n in zip(CORPORA, N):
    sents = {}
    for line in open(C + f"{c}/{c}-sentences.txt", encoding="utf-8"):
        i, s = line.rstrip("\n").split("\t", 1)
        if ok(s):
            sents[i] = s
    src = C + f"{c}/{c}-sources.txt"
    if os.path.exists(src):
        dom = {}
        for line in open(src, encoding="utf-8"):
            p = line.rstrip("\n").split("\t")
            dom[p[0]] = domain(p[1])
        by = collections.defaultdict(list)
        for line in open(C + f"{c}/{c}-inv_so.txt"):
            si, so = line.split()
            if si in sents:
                by[dom.get(so, so)].append(si)
        for v in by.values():
            random.shuffle(v)
        sites = list(by)
        random.shuffle(sites)
        picked, depth = [], 0
        while len(picked) < n:
            for d in sites:
                if depth < len(by[d]) and len(picked) < n:
                    picked.append((by[d][depth], d))
            depth += 1
    else:
        ids = random.sample(sorted(sents), n)
        picked = [(i, None) for i in ids]
    for i, d in picked:
        print(json.dumps({"corpus": c, "id": int(i), "site": d, "text": sents[i]}, ensure_ascii=False))
