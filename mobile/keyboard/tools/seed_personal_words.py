"""Sokki: a head start for the personal word list (Typos: <app files>/sokki_words.txt).

Reads the user's own writing (plain text, or the keyboard's journal .jsonl) and
prints the words autocorrect would change although the user wrote them at least
twice: their own terms (product, tool and project names). Words that are only two swapped
letters of a common word stay out (teh, nciht): those are typos even when
repeated. On Robin's 47k words this protected 6 terms and no typo.

    ~/.cache/sokki-eval-venv/bin/python mobile/keyboard/tools/seed_personal_words.py messages.txt journal-*.jsonl > sokki_words.txt
    adb shell run-as dev.verbatim.mobile sh -c 'cat >> files/sokki_words.txt' < sokki_words.txt
SPDX-License-Identifier: GPL-3.0-only
"""

import collections
import json
import os
import re
import sys

from wordfreq import get_frequency_dict

sys.path.insert(0, os.path.dirname(__file__))
import typo_model as tm  # noqa: E402

TIMES = 2


def texts(path):
    with open(path) as f:
        if path.endswith(".jsonl"):
            for line in f:
                try:
                    yield json.loads(line).get("text", "")
                except ValueError:
                    pass
        else:
            yield f.read()


words = set(get_frequency_dict("de", "large")) | set(get_frequency_dict("en", "large"))
table = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
bigrams = tm.Bloom.load(open(tm.PAIRS, "rb").read())


def swapped(a, b):
    """b is a with two neighbouring letters swapped."""
    d = [i for i in range(len(a)) if len(a) == len(b) and a[i] != b[i]]
    return len(d) == 2 and d[1] == d[0] + 1 and a[d[0]] == b[d[1]] and a[d[1]] == b[d[0]]


# The highest count in any one source: the same message can be in two (a phone
# message to Claude is in the journal and in Claude's history).
counts = collections.Counter()
for path in sys.argv[1:]:
    here = collections.Counter()
    for text in texts(path):
        here.update(tm.fold(w) for w in re.findall(r"[A-Za-zÄÖÜäöüß]+", text))
    counts |= here
for t, n in sorted(counts.items()):
    if n < TIMES:
        continue
    pick = tm.correction(table, bigrams, set(), t, vocab=words)  # without context: the word alone
    if pick and not swapped(t, tm.fold(pick)):
        print(t)
