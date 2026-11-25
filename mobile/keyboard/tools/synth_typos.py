"""Sokki: test pairs with context from a user's own text, for score_context.py.

There is no public German typing dataset, so German typos are made up: each
typo copies a real one from typing37k (public_pairs.py output). The same kind
of slip (neighbouring key, letter missing, extra letter, far key, two letters
swapped, or several of these) lands at the same relative place in the word,
with neighbours taken on the QWERTZ layout. Every word of the text is also
written as a correct pair (typed == meant): the user's own words, to count the
real words a rule would change. The text's own typos count as correct there;
read changes before calling them wrong.

    python synth_typos.py robin-texts.txt typing37k-ctx.jsonl typos.jsonl words.jsonl
SPDX-License-Identifier: GPL-3.0-only
"""

import json
import random
import re
import sys

QWERTZ = ["qwertzuiopü", "asdfghjklöä", "yxcvbnm"]
QWERTY = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
TYPO_RATE = 0.025  # share of words typed wrong in typing37k: typos in the context words
random.seed(1)


def layout_neighbours(rows):
    pos = {c: (r, i + 0.5 * r) for r, row in enumerate(rows) for i, c in enumerate(row)}
    return {a: [b for b in pos if b != a and abs(pos[a][0] - pos[b][0]) <= 1 and abs(pos[a][1] - pos[b][1]) <= 1.0]
            for a in pos}


NEAR_DE, NEAR_EN = layout_neighbours(QWERTZ), layout_neighbours(QWERTY)
LETTERS = "abcdefghijklmnopqrstuvwxyz"


def edits(typed, meant):
    """The slips that turn `meant` into `typed`, from a Damerau-Levenshtein
    alignment: (op, position in meant, typed letter) with op in sub/ins/del/swap."""
    n, m = len(meant), len(typed)
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(n + 1):
        d[i][0] = i
    for j in range(m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (meant[i - 1] != typed[j - 1]))
            if i > 1 and j > 1 and meant[i - 1] == typed[j - 2] and meant[i - 2] == typed[j - 1]:
                d[i][j] = min(d[i][j], d[i - 2][j - 2] + 1)
    ops, i, j = [], n, m
    while i or j:
        if i and j and d[i][j] == d[i - 1][j - 1] + (meant[i - 1] != typed[j - 1]):
            if meant[i - 1] != typed[j - 1]:
                ops.append(("sub", i - 1, typed[j - 1]))
            i, j = i - 1, j - 1
        elif i > 1 and j > 1 and meant[i - 1] == typed[j - 2] and meant[i - 2] == typed[j - 1] \
                and d[i][j] == d[i - 2][j - 2] + 1:
            ops.append(("swap", i - 2, ""))
            i, j = i - 2, j - 2
        elif i and d[i][j] == d[i - 1][j] + 1:
            ops.append(("del", i - 1, ""))
            i -= 1
        else:
            ops.append(("ins", i, typed[j - 1]))
            j -= 1
    return ops[::-1]


def pattern(typed, meant):
    """A typo as slips at relative places, and how each typed letter relates to
    the meant ones (neighbour / doubled / other)."""
    out = []
    for op, i, ch in edits(typed, meant):
        rel = i / max(1, len(meant) - 1)
        if op == "sub":
            how = "near" if ch in NEAR_EN.get(meant[i], ()) else "far"
        elif op == "ins":
            around = meant[max(0, i - 1): i + 1]
            how = "double" if ch in around else "near" if any(ch in NEAR_EN.get(c, ()) for c in around) else "far"
        else:
            how = ""
        out.append((op, rel, i == 0, i >= len(meant) - 1, how))
    return out


def apply(word, pat):
    w = list(word.lower())
    for op, rel, first, last, how in sorted(pat, key=lambda p: -p[1]):  # back to front: places stay valid
        n = len(w)
        i = 0 if first else n - 1 if last else min(n - 1, max(1, round(rel * (n - 1))))
        if op == "sub" and i < n:
            near = NEAR_DE.get(w[i], [])
            w[i] = random.choice(near) if how == "near" and near else random.choice(LETTERS)
        elif op == "ins":
            i = min(i, n)
            around = w[max(0, i - 1): i + 1] or ["e"]
            c = random.choice(around)
            w.insert(i, c if how == "double" else random.choice(NEAR_DE.get(c, list(LETTERS))) if how == "near"
                     else random.choice(LETTERS))
        elif op == "del" and n > 1 and i < n:
            del w[i]
        elif op == "swap" and i + 1 < n:
            w[i], w[i + 1] = w[i + 1], w[i]
    typed = "".join(w)
    return typed[0].upper() + typed[1:] if word[0].isupper() and typed else typed


def main(text_path, pairs_path, typos_out, words_out):
    pats = []
    for line in open(pairs_path):
        p = json.loads(line)
        t, m = p["typed"].lower(), p["meant"].lower()
        if t != m and len(m) >= 2:
            pats.append(pattern(t, m))
    text = open(text_path).read()
    with open(typos_out, "w") as ft, open(words_out, "w") as fw:
        for sentence in re.split(r"[.!?\n:;]+", text):
            ws = re.findall(r"[A-Za-zÄÖÜäöüß]+", sentence)
            # the context as typed: some of its words carry typos too
            typed_ws = [apply(w, random.choice(pats)) if len(w) >= 2 and random.random() < TYPO_RATE else w
                        for w in ws]
            for i, w in enumerate(ws):
                prev, nxt = typed_ws[max(0, i - 8):i], typed_ws[i + 1] if i + 1 < len(ws) else ""
                fw.write(json.dumps({"source": "robin", "typed": w, "meant": w, "prev": prev, "next": nxt},
                                    ensure_ascii=False) + "\n")
                if len(w) < 2:
                    continue
                for _ in range(10):
                    typed = apply(w, random.choice(pats))
                    if typed.lower() != w.lower():
                        ft.write(json.dumps({"source": "robin-synth", "typed": typed, "meant": w, "prev": prev,
                                             "next": nxt}, ensure_ascii=False) + "\n")
                        break


if __name__ == "__main__":
    main(*sys.argv[1:5])
