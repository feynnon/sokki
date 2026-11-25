"""Sokki: letter-aware key edges, the model shared by the generator, the
offline evaluation and (ported 1:1) dev.verbatim.sokki.KeyTargeting.

A tap goes to the letter key that maximises
    log N(touch | key centre, sigma) + weight * log P(letter | two letters before)
but only when the tap is outside the inner core of the key it landed on
("anchored" targets, Gunawardana et al. 2010): the middle of a key always
gives that key, so every key can still be typed on purpose.

Letter table: P(letter | c1 c2) for 30 letters (a-z ä ö ü ß) after any two
symbols (the 30 letters or "^" = start of word), interpolated trigram /
bigram / unigram, stored as one byte per entry: round(-ln p * 16).
SPDX-License-Identifier: GPL-3.0-only
"""

import math

LETTERS = "abcdefghijklmnopqrstuvwxyzäöüß"
SYMBOLS = "^" + LETTERS  # index 0 = start of word
N = len(SYMBOLS)  # 31
SCALE = 16.0  # bytes are -ln(p) * SCALE
LAMBDAS = (0.75, 0.2, 0.05)  # trigram, bigram, unigram

# Decision parameters (Kotlin: KeyTargeting). Distances are in key widths / heights.
SIGMA_X = 0.30
SIGMA_Y = 0.30
WEIGHT = 1.0
CORE = 0.5  # inner core: |dx| < CORE/2 and |dy| < CORE/2 of the key's size
REACH = 1.0  # candidates: letter keys whose centre is within REACH key widths


def sym(c):
    """Index of a context character: a letter, or 0 for anything else."""
    i = SYMBOLS.find(c.lower()) if c else -1
    return i if i > 0 else 0


def build_table(word_freqs):
    """word_freqs: iterable of (word, frequency). Returns bytes(N*N*N)."""
    tri = [[[0.0] * N for _ in range(N)] for _ in range(N)]
    bi = [[0.0] * N for _ in range(N)]
    uni = [0.0] * N
    for word, f in word_freqs:
        w = word.lower()
        if not w or any(c not in LETTERS for c in w):
            continue
        s = [0, 0] + [SYMBOLS.index(c) for c in w]
        for i in range(2, len(s)):
            a, b, c = s[i - 2], s[i - 1], s[i]
            tri[a][b][c] += f
            bi[b][c] += f
            uni[c] += f
    tu = sum(uni)
    out = bytearray(N * N * N)
    for a in range(N):
        for b in range(N):
            tsum = sum(tri[a][b])
            bsum = sum(bi[b])
            for c in range(1, N):
                pu = uni[c] / tu
                pb = bi[b][c] / bsum if bsum else pu
                pt = tri[a][b][c] / tsum if tsum else pb
                p = LAMBDAS[0] * pt + LAMBDAS[1] * pb + LAMBDAS[2] * pu
                p = max(p, 1e-12)
                out[(a * N + b) * N + c] = min(255, round(-math.log(p) * SCALE))
    return bytes(out)


def logp(table, before, letter):
    """ln P(letter | the two characters before). before: text before the cursor."""
    c2 = sym(before[-1]) if len(before) >= 1 else 0
    c1 = sym(before[-2]) if len(before) >= 2 and c2 != 0 else 0
    c = SYMBOLS.find(letter)
    if c <= 0:
        return None
    return -table[(c1 * N + c2) * N + c] / SCALE


def decide(table, keys, x, y, before, sigma_x=SIGMA_X, sigma_y=SIGMA_Y,
           weight=WEIGHT, core=CORE, reach=REACH):
    """keys: list of dicts {label, x, y, w, h} (visible key boxes, letters only
    get re-weighed). Returns the label the tap counts as.
    Mirrors KeyTargeting.decide in Kotlin."""
    hit = None
    for k in keys:
        if k["x"] <= x < k["x"] + k["w"] and k["y"] <= y < k["y"] + k["h"]:
            hit = k
            break
    if hit is None:  # between keys: nearest key by edge distance (as HeliBoard)
        hit = min(keys, key=lambda k: _edge_dist2(k, x, y))
    if table is None or len(hit["label"]) != 1 or hit["label"] not in LETTERS:
        return hit["label"]
    cx, cy = hit["x"] + hit["w"] / 2, hit["y"] + hit["h"] / 2
    if abs(x - cx) < core * hit["w"] / 2 and abs(y - cy) < core * hit["h"] / 2:
        return hit["label"]
    best, best_score = hit["label"], None
    for k in keys:
        lab = k["label"]
        if len(lab) != 1 or lab not in LETTERS:
            continue
        kx, ky = k["x"] + k["w"] / 2, k["y"] + k["h"] / 2
        dx, dy = (x - kx) / k["w"], (y - ky) / k["h"]
        if dx * dx + dy * dy > reach * reach * 2.25:  # 1.5 * reach, generous
            continue
        lp = logp(table, before, lab)
        if lp is None:
            continue
        score = -0.5 * ((dx / sigma_x) ** 2 + (dy / sigma_y) ** 2) + weight * lp
        if best_score is None or score > best_score:
            best, best_score = lab, score
    return best


def _edge_dist2(k, x, y):
    dx = max(k["x"] - x, 0, x - (k["x"] + k["w"]))
    dy = max(k["y"] - y, 0, y - (k["y"] + k["h"]))
    return dx * dx + dy * dy
