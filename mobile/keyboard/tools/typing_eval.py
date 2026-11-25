"""Sokki: does letter-aware key targeting help? Offline, two ways.

  sim    <test.jsonl>          synthetic thumb taps (Gaussian scatter around key
                               centres) over the typing-test sentences, on the
                               keyboard layout recorded in the log
  replay <test.jsonl> [...]    the user's real taps from typing tests: their
                               typing pattern, and geometry vs. model on them
  daily  <taps-*.jsonl> [...]  everyday taps (TapLog): misses (a letter deleted
                               at once and retyped as a neighbour), how often a
                               smart-edge decision was deleted at once, what the
                               model would fix or break, and the typing pattern

Get logs from the phone:
  adb exec-out run-as dev.verbatim.mobile sh -c 'cat files/typing_test/test-*.jsonl' > test.jsonl
  adb exec-out run-as dev.verbatim.mobile sh -c 'cat files/typing_log/taps-*.jsonl' > daily.jsonl
SPDX-License-Identifier: GPL-3.0-only
"""

import json
import math
import os
import random
import re
import statistics as st
import sys

sys.path.insert(0, os.path.dirname(__file__))
import letter_model as M  # noqa: E402

HERE = os.path.dirname(__file__)
ASSETS = os.path.join(HERE, "..", "src", "main", "assets", "sokki")
KOTLIN = os.path.join(HERE, "..", "src", "main", "java", "dev", "verbatim", "sokki", "TypingTest.kt")
DELETE = -7


def table(lang):
    with open(os.path.join(ASSETS, f"letters_{lang}.bin"), "rb") as f:
        return f.read()


def label_of(code):
    if code is None:
        return None
    if code > 0:
        return chr(code).lower()
    return f"<{code}>"


def read_log(paths):
    events = []
    for p in paths:
        with open(p) as f:
            events += [json.loads(line) for line in f if line.strip()]
    return events


def keys_of(layout):
    return [{"label": label_of(k["c"]), "x": k["x"], "y": k["y"], "w": k["w"], "h": k["h"]}
            for k in layout["keys"] if k["w"] > 0 and k["h"] > 0]


def sentences():
    src = open(KOTLIN).read()
    block = src[src.index("val SENTENCES"):]
    return re.findall(r'"([^"]+)"', block[:block.index(")\n")])


def centre(k):
    return k["x"] + k["w"] / 2, k["y"] + k["h"] / 2


# ---------------------------------------------------------------- simulation

def simulate(keys, tab, text, sx, sy, rng, **params):
    by = {k["label"]: k for k in keys}
    out = {"letters": 0, "geo": 0, "model": 0, "fixed": 0, "broken": 0}
    for s in text:
        typed = ""
        for ch in s.lower():
            k = by.get(ch)
            if k is None:
                typed += ch
                continue
            cx, cy = centre(k)
            x, y = rng.gauss(cx, sx * k["w"]), rng.gauss(cy, sy * k["h"])
            geo = M.decide(None, keys, x, y, typed)
            mod = M.decide(tab, keys, x, y, typed, **params)
            if ch in M.LETTERS:
                out["letters"] += 1
                out["geo"] += geo != ch
                out["model"] += mod != ch
                out["fixed"] += geo != ch and mod == ch
                out["broken"] += geo == ch and mod != ch
            typed += mod if len(mod) == 1 else ch
    return out


def cmd_sim(paths):
    events = read_log(paths)
    layout = next(e for e in events if e["t"] == "layout" and e["element"].startswith("ALPHABET"))
    keys = keys_of(layout)
    tab = table(layout["lang"].split("-")[0])
    text = sentences()
    print(f"layout {layout['element']} {layout['lang']}, {len(text)} sentences, model sigma={M.SIGMA_X} weight={M.WEIGHT} core={M.CORE}")
    print("scatter (key widths) | letter taps | errors geometry | errors model | fixed | broken | change")
    for s in (0.15, 0.20, 0.25, 0.30, 0.35):
        tot = {"letters": 0, "geo": 0, "model": 0, "fixed": 0, "broken": 0}
        for seed in range(20):
            r = simulate(keys, tab, text, s, s, random.Random(seed))
            for k in tot:
                tot[k] += r[k]
        g, m = tot["geo"] / tot["letters"], tot["model"] / tot["letters"]
        print(f"  {s:.2f} | {tot['letters']} | {g:6.2%} | {m:6.2%} | {tot['fixed']} | {tot['broken']} | {(m - g) / g if g else 0:+.0%}")


# ---------------------------------------------------------------- replay

def taps_with_intent(events):
    """Yields dicts for every tap that typed a letter: touch, keys, context,
    intended letter (from the target sentence at the cursor position), geo."""
    keys, target, buf = None, None, []
    for e in events:
        t = e["t"]
        if t == "layout":
            keys = keys_of(e) if e["element"].startswith("ALPHABET") else None
        elif t == "sentence_start":
            target, buf = e["target"], []
        elif t == "sentence_end":
            target = None
        elif t == "tap" and target is not None:
            out = e["out"]
            if out == DELETE:
                if buf:
                    buf.pop()
                continue
            if out is None or out <= 0:
                continue
            ch = chr(out)
            pos = len(buf)
            before = "".join(buf)
            buf.append(ch)
            if keys is None or pos >= len(target):
                continue
            want = target[pos].lower()
            if want not in M.LETTERS:
                continue
            yield {"x": e["x"], "y": e["y"], "keys": keys, "before": before, "want": want,
                   "geo": label_of(e["geo"]), "out": label_of(out), "sentence": target}


def evaluate(taps, tab, **params):
    r = {"n": 0, "geo": 0, "model": 0, "fixed": 0, "broken": 0}
    for tp in taps:
        mod = M.decide(tab, tp["keys"], tp["x"], tp["y"], tp["before"], **params)
        r["n"] += 1
        r["geo"] += tp["geo"] != tp["want"]
        r["model"] += mod != tp["want"]
        r["fixed"] += tp["geo"] != tp["want"] and mod == tp["want"]
        r["broken"] += tp["geo"] == tp["want"] and mod != tp["want"]
    return r


def cmd_replay(paths):
    events = read_log(paths)
    lang = next((e["lang"] for e in events if e["t"] == "layout"), "de").split("-")[0]
    tab = table(lang)
    all_taps = list(taps_with_intent(events))
    # Spatial taps only: the touch lies within 1.5 keys of the intended key
    # (an omitted or doubled letter shifts the rest of the sentence; those
    # aren't aiming errors and no key model can fix them).
    taps, skipped = [], 0
    for tp in all_taps:
        k = next((k for k in tp["keys"] if k["label"] == tp["want"]), None)
        if k is None:
            skipped += 1
            continue
        cx, cy = centre(k)
        dx, dy = (tp["x"] - cx) / k["w"], (tp["y"] - cy) / k["h"]
        if dx * dx + dy * dy > 2.25:
            skipped += 1
            continue
        tp["dx"], tp["dy"] = dx, dy
        taps.append(tp)
    print(f"{len(all_taps)} letter taps, {len(taps)} aimed at the intended key ({skipped} skipped: missing/extra letters)")
    if not taps:
        return

    # Your pattern
    dxs, dys = [t["dx"] for t in taps], [t["dy"] for t in taps]
    print("\nYour typing pattern (in key widths / heights; + = right / down):")
    print(f"  average offset x {st.mean(dxs):+.2f}, y {st.mean(dys):+.2f}; scatter x {st.pstdev(dxs):.2f}, y {st.pstdev(dys):.2f}")
    rows = {}
    for t in taps:
        k = next(k for k in t["keys"] if k["label"] == t["want"])
        rows.setdefault(round(centre(k)[1]), []).append(t)
    for i, (y, ts) in enumerate(sorted(rows.items())):
        print(f"  row {i + 1}: offset x {st.mean(t['dx'] for t in ts):+.2f}, y {st.mean(t['dy'] for t in ts):+.2f}  ({len(ts)} taps)")
    per = {}
    for t in taps:
        per.setdefault(t["want"], []).append(t)
    missed = sorted(((sum(t["geo"] != t["want"] for t in ts) / len(ts), k, len(ts)) for k, ts in per.items() if len(ts) >= 5), reverse=True)[:6]
    print("  most missed keys: " + ", ".join(f"{k} {r:.0%} of {n}" for r, k, n in missed if r > 0))
    confusions = {}
    for t in taps:
        if t["geo"] != t["want"]:
            confusions[(t["want"], t["geo"])] = confusions.get((t["want"], t["geo"]), 0) + 1
    print("  misses (meant → hit): " + ", ".join(f"{a}→{b} {n}×" for (a, b), n in sorted(confusions.items(), key=lambda x: -x[1])[:10]))

    # Geometry vs. model, current parameters
    r = evaluate(taps, tab)
    print(f"\nWith the shipped parameters (sigma {M.SIGMA_X}, weight {M.WEIGHT}, core {M.CORE}):")
    print(f"  errors geometry {r['geo']}/{r['n']} ({r['geo'] / r['n']:.2%}), model {r['model']} ({r['model'] / r['n']:.2%}); fixed {r['fixed']}, broken {r['broken']}")

    # Tuned on half the sentences, checked on the other half
    sents = sorted({t["sentence"] for t in taps})
    a = [t for t in taps if sents.index(t["sentence"]) % 2 == 0]
    b = [t for t in taps if sents.index(t["sentence"]) % 2 == 1]
    grid = [dict(sigma_x=s, sigma_y=s, weight=w, core=c)
            for s in (0.2, 0.25, 0.3, 0.4) for w in (0.5, 1.0, 1.5) for c in (0.3, 0.5, 0.7)]
    for train, test, name in ((a, b, "A→B"), (b, a, "B→A")):
        if not train or not test:
            continue
        best = min(grid, key=lambda p: evaluate(train, tab, **p)["model"])
        r = evaluate(test, tab, **best)
        print(f"  tuned on half {name[0]}, tested on {name[-1]}: {best} → errors geometry {r['geo']}/{r['n']}, model {r['model']}; fixed {r['fixed']}, broken {r['broken']}")


# ---------------------------------------------------------------- everyday taps

def cmd_daily(paths):
    events = read_log(paths)
    keys, lang = None, "de"
    taps = []  # letter/space/delete taps with context, in order
    prev_ms, prev_app, typed = 0, None, ""
    for e in events:
        if e["t"] == "layout":
            keys = keys_of(e) if e["element"].startswith("ALPHABET") else None
            lang = e["lang"].split("-")[0]
            continue
        if e["t"] != "tap":
            continue
        if e["app"] != prev_app or e["ms"] - prev_ms > 30_000:
            typed = ""  # new field or long pause: no context
        prev_app, prev_ms = e["app"], e["ms"]
        out = e["out"]
        taps.append({**e, "keys": keys, "lang": lang, "before": typed[-2:],
                     "geo_l": label_of(e["geo"]), "out_l": label_of(out)})
        if out == DELETE:
            typed = typed[:-1]
        elif out and out > 0:
            typed += chr(out)
    tables = {}
    def tab(lang):
        if lang not in tables:
            try:
                tables[lang] = table(lang)
            except FileNotFoundError:
                tables[lang] = None
        return tables[lang]

    letters = [i for i, t in enumerate(taps) if t["keys"] and t["out_l"] in M.LETTERS]
    def undone(i):
        return i + 1 < len(taps) and taps[i + 1]["out"] == DELETE and taps[i + 1]["app"] == taps[i]["app"] \
            and taps[i + 1]["ms"] - taps[i]["ms"] < 5000
    print(f"{len(letters)} letter taps in {len({t['app'] for t in taps})} apps, "
          f"{sum(t['kt'] for t in taps)} with smart key edges on")

    # Smart-edge decisions in real use
    over = [i for i in letters if taps[i]["kt"] and taps[i]["geo_l"] != taps[i]["out_l"]]
    if over:
        u = sum(undone(i) for i in over)
        print(f"\nSmart key edges changed {len(over)} taps ({len(over) / len(letters):.1%}); "
              f"{u} of them were deleted right away (likely wrong), {len(over) - u} kept")

    # Misses: letter, delete, then a neighbouring letter
    misses, kept = [], []
    for i in letters:
        t = taps[i]
        if undone(i) and i + 2 < len(taps) and taps[i + 2]["out_l"] in M.LETTERS and taps[i + 2]["app"] == t["app"]:
            want = taps[i + 2]["out_l"]
            k = next((k for k in t["keys"] if k["label"] == want), None)
            if k and want != t["out_l"]:
                cx, cy = centre(k)
                if ((t["x"] - cx) / k["w"]) ** 2 + ((t["y"] - cy) / k["h"]) ** 2 <= 2.25:
                    misses.append((i, want))
        elif not undone(i):
            kept.append(i)
    print(f"\nMisses (deleted at once, retyped as a neighbour): {len(misses)} of {len(letters)} letter taps "
          f"({len(misses) / max(1, len(letters)):.1%})")
    conf = {}
    for i, want in misses:
        conf[(want, taps[i]["out_l"])] = conf.get((want, taps[i]["out_l"]), 0) + 1
    if conf:
        print("  meant → hit: " + ", ".join(f"{a}→{b} {n}×" for (a, b), n in sorted(conf.items(), key=lambda x: -x[1])[:10]))

    # What the model would do on all of them (geometry as the baseline)
    fixed = sum(M.decide(tab(taps[i]["lang"]), taps[i]["keys"], taps[i]["x"], taps[i]["y"], taps[i]["before"]) == want
                for i, want in misses if tab(taps[i]["lang"]))
    broken = sum(M.decide(tab(taps[i]["lang"]), taps[i]["keys"], taps[i]["x"], taps[i]["y"], taps[i]["before"]) != taps[i]["geo_l"]
                 for i in kept if tab(taps[i]["lang"]) and taps[i]["geo_l"] == taps[i]["out_l"])
    print(f"Model replayed on these taps: would fix {fixed} of {len(misses)} misses, "
          f"would change {broken} of {len(kept)} kept taps (each a likely new mistake)")

    # Pattern: kept taps relative to the key they gave
    dxs, dys = [], []
    for i in kept:
        t = taps[i]
        k = next((k for k in t["keys"] if k["label"] == t["out_l"]), None)
        if k:
            cx, cy = centre(k)
            dxs.append((t["x"] - cx) / k["w"]); dys.append((t["y"] - cy) / k["h"])
    if dxs:
        print(f"\nYour pattern ({len(dxs)} kept taps; key widths / heights, + = right / down): "
              f"offset x {st.mean(dxs):+.2f}, y {st.mean(dys):+.2f}; scatter x {st.pstdev(dxs):.2f}, y {st.pstdev(dys):.2f}")


if __name__ == "__main__":
    cmds = {"sim": cmd_sim, "replay": cmd_replay, "daily": cmd_daily}
    if len(sys.argv) < 3 or sys.argv[1] not in cmds:
        print(__doc__)
        sys.exit(1)
    cmds[sys.argv[1]](sys.argv[2:])
