"""Sokki: typo pairs from public typing datasets, for score_context.py.

typing37k  Palin et al. 2019, phones, 37k people (userinterfaces.aalto.fi/typing37k):
           processed_typing_data.zip + sentences_content.sql. Only sentences typed
           without swipe, prediction or autocorrect, so the typos are raw.
136m       Dhakal et al. 2018, physical keyboards, 168k people
           (userinterfaces.aalto.fi/136Mkeystrokes; free for research and
           non-commercial use with attribution): Keystrokes.zip.

Each typed sentence is lined up word by word with the sentence people were
asked to type. Every typo pair is kept, and one in SAMPLE correctly typed words
(to count real words that autocorrect would change). Each pair carries its context as
typed: "prev", up to 8 words before it in the sentence (in reading order), and
"next", the word after it ("" at the end).

    python3 mobile/keyboard/tools/public_pairs.py typing37k <dir> > typing37k-pairs.jsonl
    python3 mobile/keyboard/tools/public_pairs.py 136m <dir> > 136m-pairs.jsonl
SPDX-License-Identifier: GPL-3.0-only
"""

import difflib
import io
import json
import os
import random
import re
import subprocess
import sys
import zipfile

SAMPLE = 50
random.seed(1)


def sql_rows(line):
    """The value tuples of one MySQL INSERT line, as lists of str/None."""
    i = line.index(" VALUES ") + 8
    row, rows, n = [], [], len(line)
    while i < n:
        c = line[i]
        if c == "(":
            row = []
            i += 1
        elif c == "'":
            j, buf = i + 1, []
            while True:
                k = line[j]
                if k == "\\":
                    buf.append(line[j + 1])
                    j += 2
                elif k == "'":
                    if j + 1 < n and line[j + 1] == "'":
                        buf.append("'")
                        j += 2
                    else:
                        break
                else:
                    buf.append(k)
                    j += 1
            row.append("".join(buf))
            i = j + 1
        elif c in ",; \n":
            i += 1
        elif c == ")":
            rows.append(row)
            i += 1
        else:
            j = i
            while line[j] not in ",)":
                j += 1
            v = line[i:j]
            row.append(None if v == "NULL" else v)
            i = j
    return rows


def word_pairs(typed, meant):
    """(index in typed, typed word, meant word) for words lined up one to one."""
    t, m = typed.split(), meant.split()
    for op, i1, i2, j1, j2 in difflib.SequenceMatcher(a=[w.lower() for w in t], b=[w.lower() for w in m],
                                                     autojunk=False).get_opcodes():
        if op == "equal" or (op == "replace" and i2 - i1 == j2 - j1):
            yield from zip(range(i1, i2), t[i1:i2], m[j1:j2])


PUNCT = ".,!?;:\"'()"


def context(words, i):
    """Up to 8 typed words before words[i] in its sentence, and the word after it."""
    prev = []
    for w in reversed(words[max(0, i - 8):i]):
        if w[-1:] in ".!?":
            break
        prev.insert(0, w.strip(PUNCT))
    nxt = words[i + 1].strip(PUNCT) if i + 1 < len(words) and words[i][-1:] not in ".!?" else ""
    return [w for w in prev if w], nxt


def emit(typed_sentence, meant_sentence, source):
    words = typed_sentence.split()
    for i, t, m in word_pairs(typed_sentence, meant_sentence):
        t, m = t.strip(PUNCT), m.strip(PUNCT)
        if not t.isalpha() or not m.isalpha():
            continue
        if t.lower() == m.lower():
            t = m  # only the case differs (shift): not a typo autocorrect is about
        if t == m and random.randrange(SAMPLE):
            continue
        prev, nxt = context(words, i)
        print(json.dumps({"source": source, "typed": t, "meant": m, "weight": SAMPLE if t == m else 1,
                          "prev": prev, "next": nxt}, ensure_ascii=False))


def typing37k(d):
    sentences = {}
    for line in open(os.path.join(d, "typing37k_sentences.sql"), encoding="utf-8", errors="replace"):
        if line.startswith("\t("):  # one sentence per line after a lone VALUES
            for r in sql_rows("x VALUES " + line.strip()):
                sentences[r[0]] = r[1]
    # this zip uses a compression Python's zipfile can't read: stream it through unzip
    proc = subprocess.Popen(["unzip", "-p", os.path.join(d, "typing37k_processed.zip")], stdout=subprocess.PIPE)
    with io.TextIOWrapper(proc.stdout, encoding="latin-1") as f:
            for line in f:
                if not line.startswith("INSERT INTO `open_test_sections`"):
                    continue
                for r in sql_rows(line):
                    n_swype, n_pred, n_auto = r[-7], r[-6], r[-5]
                    if r[3] and r[1] in sentences and n_swype == n_pred == n_auto == "0":
                        emit(r[3], sentences[r[1]], "typing37k")


def m136(d):
    with zipfile.ZipFile(os.path.join(d, "136m_keystrokes.zip")) as z:
        for name in z.namelist():
            if not name.endswith("_keystrokes.txt"):
                continue
            with io.TextIOWrapper(z.open(name), encoding="utf-8", errors="replace") as f:
                header = f.readline().rstrip("\n").split("\t")
                try:
                    si, ui, ts = header.index("SENTENCE"), header.index("USER_INPUT"), header.index("TEST_SECTION_ID")
                except ValueError:
                    continue
                last = None
                for line in f:
                    v = line.rstrip("\n").split("\t")
                    if len(v) > max(si, ui) and v[ts] != last:
                        last = v[ts]
                        emit(v[ui], v[si], "136m")


if __name__ == "__main__":
    {"typing37k": typing37k, "136m": m136}[sys.argv[1]](sys.argv[2])
