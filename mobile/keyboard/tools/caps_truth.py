"""Sokki: which words of Robin's writing should be capitalised, judged by an independent tagger.

A word German only has capitalised (Variante) is a noun, one it only has in lower case (allen) is
not; the German dictionaries decide those. Words German has both ways (Reise/reise,
Krieg/krieg) are decided by spaCy's German tagger (de_core_news_sm, MIT) in the sentence: noun or
name -> capital. The tagger alone is unreliable on Robin's style (lower-case nouns, typos,
Denglish), so it only judges the ambiguous words.
English and mixed sentences are left out (their nouns stay lower case in English). The result
lines up with robin_eval.tokens(), which reads it to score capitalisation.

    uv venv -p 3.12 ~/.cache/sokki-spacy-venv && uv pip install --python ... spacy de_core_news_sm
    ~/.cache/sokki-spacy-venv/bin/python mobile/keyboard/tools/caps_truth.py
SPDX-License-Identifier: GPL-3.0-only
"""

import json
import os
import sys

import spacy

sys.path.insert(0, os.path.dirname(__file__))
import robin_eval as ev  # noqa: E402
import typo_model as tm  # noqa: E402


def sentence_lang(table, words):
    d = sum(tm.lean(table, w) == "d" for w in words)
    e = sum(tm.lean(table, w) == "e" for w in words)
    return "de" if d >= 2 and d >= 3 * e else "en" if e >= 2 and e >= 3 * d else "mixed"


def main():
    table = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
    lower, upper = set(), set()
    for f in ("main_de__legacy.combined", "main_de.combined"):
        for w in ev.wordlist(f):
            (lower if w[:1].islower() else upper).add(w)
    nlp = spacy.load("de_core_news_sm", disable=["ner", "lemmatizer", "parser"])
    toks, _, jtoks, _ = ev.load_robin()
    out = {}
    for name, ts in (("text", toks), ("journal", jtoks)):
        # tokens() yields words sentence by sentence: a sentence starts where prev is BOS
        sents, cur = [], []
        for w, prev, *_ in ts:
            if prev == tm.BOS and cur:
                sents.append(cur)
                cur = []
            cur.append(w)
        if cur:
            sents.append(cur)
        tags = []
        for words, doc in zip(sents, nlp.pipe((" ".join(s) for s in sents), batch_size=256)):
            pos = [t.pos_ for t in doc]
            lang = sentence_lang(table, words)
            if len(pos) != len(words):
                tags += [None] * len(words)
                continue
            for w, p in zip(words, pos):
                if lang != "de":
                    tags.append(None)
                    continue
                lo, up = w[0].lower() + w[1:], w[0].upper() + w[1:]
                if up in upper and lo not in lower:
                    tags.append(True)
                elif lo in lower and up not in upper:
                    tags.append(False)
                else:
                    tags.append(p in ("NOUN", "PROPN") if (up in upper or lo in lower) else None)
        out[name] = tags
        print(name, len(ts), "words,", sum(t is not None for t in tags), "in German sentences,",
              sum(bool(t) for t in tags), "nouns/names")
    json.dump(out, open(ev.D + "caps-truth.json", "w"))


if __name__ == "__main__":
    main()
