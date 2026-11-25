"""Sokki: plain sentences from a Wikipedia dump, for ngram_lm.py.

Reads a pages-articles XML dump (.bz2) from dumps.wikimedia.org, keeps
articles (namespace 0, no redirects), strips the wiki markup, splits the
prose into sentences and writes a seeded random sample of them as
"id<TAB>sentence", the format ngram_lm.py reads. Wikipedia text is CC BY-SA
4.0, so the word-pair table built from it is too (keyboard/LICENSE-CC-BY-SA-4.0).

    python wiki_sentences.py dewiki-...-pages-articles1.xml-p1p297012.bz2 de-wiki-1M.txt 1000000

Only the standard library. The markup stripping is rough on purpose: a line
that still holds markup after it is dropped, and only whole prose sentences
are kept, so what reaches the counts is plain running text.
SPDX-License-Identifier: GPL-3.0-only
"""

import bz2
import random
import re
import sys
import xml.etree.ElementTree as ET

SEED = 20261124
COMMENT = re.compile(r"<!--.*?-->", re.S)
REF = re.compile(r"<ref[^>/]*/>|<ref[^>]*>.*?</ref>", re.S | re.I)
BLOCK = re.compile(r"<(math|code|pre|syntaxhighlight|source|gallery|timeline|score|chem|hiero|poem|table|div)[^>]*>.*?</\1>", re.S | re.I)
TAG = re.compile(r"</?[a-zA-Z][^>]*>")
FILE_LINK = re.compile(r"\[\[(?:Datei|Bild|File|Image|Kategorie|Category|Media):[^\[\]]*(?:\[\[[^\[\]]*\]\][^\[\]]*)*\]\]", re.I)
LINK = re.compile(r"\[\[(?:[^\[\]|]*\|)?([^\[\]|]*)\]\]")
EXT_LINK = re.compile(r"\[https?://[^\s\]]+\s*([^\]]*)\]")
QUOTES = re.compile(r"'{2,}")
ENTITY = {"&nbsp;": " ", "&amp;": "&", "&lt;": "<", "&gt;": ">", "&quot;": '"', "&ndash;": "–", "&mdash;": "—"}
SPLIT = re.compile(r"(?<=[.!?])\s+(?=[A-ZÄÖÜ\"„“])")
LEFTOVER = re.compile(r"[{}\[\]|<>=_\\]|https?:|www\.")
# abbreviations and German ordinals ("im 20. Jahrhundert") end with a dot inside
# a sentence: a split after them is undone
ABBREV = re.compile(r"(?:\b\d+|\b[A-Za-zÄÖÜäöü]|\b(?:z\.B|d\.h|u\.a|bzw|ca|vgl|Nr|St|Dr|Prof|etc|usw|Jh|Mr|Mrs|Ms|Jr|Sr|vs|No|Inc|Ltd|Co|Bd|Abb|ggf|evtl|sog|geb|gest))\.$")


def strip_nested(text, open_, close):
    """Removes {{...}} templates or {|...|} tables, nested ones included."""
    out, depth, i, n = [], 0, 0, len(text)
    lo, lc = len(open_), len(close)
    while i < n:
        if text.startswith(open_, i):
            depth += 1
            i += lo
        elif depth and text.startswith(close, i):
            depth -= 1
            i += lc
        else:
            if not depth:
                out.append(text[i])
            i += 1
    return "".join(out)


def prose(wikitext):
    t = COMMENT.sub("", wikitext)
    t = REF.sub("", t)
    t = BLOCK.sub("", t)
    t = strip_nested(t, "{{", "}}")
    t = strip_nested(t, "{|", "|}")
    t = FILE_LINK.sub("", t)
    for _ in range(2):  # links inside link labels
        t = LINK.sub(r"\1", t)
    t = EXT_LINK.sub(r"\1", t)
    t = TAG.sub("", t)
    t = QUOTES.sub("", t)
    for k, v in ENTITY.items():
        t = t.replace(k, v)
    for line in t.split("\n"):
        line = line.strip()
        # headings, lists, indents, tables and leftovers aren't running prose
        if not line or line[0] in "=*#:;|!{" or line.startswith("__"):
            continue
        yield line


def sentences_of(line):
    parts = SPLIT.split(line)
    merged = []
    for p in parts:
        if merged and ABBREV.search(merged[-1]):
            merged[-1] += " " + p
        else:
            merged.append(p)
    for s in merged:
        s = re.sub(r"\s+", " ", s).strip()
        s = re.sub(r"\(\s*[,;]?\s*\)", "", s).replace(" ,", ",").replace(" .", ".")
        if not 25 <= len(s) <= 400 or s[-1] not in ".!?" or LEFTOVER.search(s):
            continue
        if len(s.split()) < 4:
            continue
        yield s


def pages(path):
    with bz2.open(path, "rb") as f:
        ns_text = title = None
        for event, el in ET.iterparse(f, events=("end",)):
            tag = el.tag.rsplit("}", 1)[-1]
            if tag == "ns":
                ns_text = el.text
            elif tag == "redirect":
                title = "#redirect"
            elif tag == "text":
                if ns_text == "0" and title != "#redirect" and el.text and not el.text.lstrip().lower().startswith(("#redirect", "#weiterleitung")):
                    yield el.text
            elif tag == "page":
                ns_text = title = None
                el.clear()


def main(dump, out, want):
    want = int(want)
    rng = random.Random(SEED)
    sample, seen = [], 0
    for text in pages(dump):
        for line in prose(text):
            for s in sentences_of(line):
                seen += 1
                # reservoir sampling: every sentence of the dump equally likely
                if len(sample) < want:
                    sample.append(s)
                else:
                    j = rng.randrange(seen)
                    if j < want:
                        sample[j] = s
    with open(out, "w", encoding="utf-8") as f:
        for i, s in enumerate(sample, 1):
            f.write(f"{i}\t{s}\n")
    print(out, len(sample), "of", seen, "sentences")


if __name__ == "__main__":
    main(*sys.argv[1:4])
