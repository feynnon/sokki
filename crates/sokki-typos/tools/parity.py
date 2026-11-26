"""Sokki: check the Mac's Rust typo model gives the same answer as the phone's Python twin.

Runs both on every word of Robin's own writing (~/sokki-typing-data, private: nothing from it is
written outside a temp dir) and on samples of the public typing sets, each word with the three
words before it in its sentence, the next word unknown (as when a word is committed).

    cargo build --release --features cli   (in crates/sokki-typos)
    ~/.cache/sokki-eval-venv/bin/python crates/sokki-typos/tools/parity.py <sokki-typos binary>
Prints how many answers differ, and the first few that do (to the terminal only).

Since 26 Nov 2026 the Rust model differs on purpose: the fixes for German capitals in English
sentences ("an option", "in these cases"), "-t" after a t-stem ("justt") and the Bloom filter
cross-check in compound repair went into Rust only (docs/research/mac-autocorrect-2026-11.md).
Measured then: about 150 of 47,494 words of Robin's text, 17 of 4,000 in typing37k and in 136m.
Also Rust only since the evening of 26 Nov 2026: British -ise spellings count as English words, and
a word with no candidate one slip away gets the dictionary's common words two slips away. With the
phone's QWERTZ (the default; the Mac sets its own keys with `set_keyboard`): 152 of 47,494, 22 and
31 of 4,000. Until the Python twin gets them too, read a difference here against that list.
SPDX-License-Identifier: GPL-3.0-only
"""
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", ".."))
TOOLS = os.path.join(ROOT, "mobile", "keyboard", "tools")
sys.path.insert(0, TOOLS)
import typo_model as tm  # noqa: E402
import robin_eval as re_  # noqa: E402

D = re_.D


def samples():
    toks, labels, jtoks, _ = re_.load_robin()
    for w, _, _, prevs in toks + jtoks:
        yield "robin", w, prevs
    for name in ("typing37k", "136m"):
        for x in re_.sample(D + f"public/{name}-ctx.jsonl", 1500, 2500):
            prevs = ([tm.BOS] + x["prev"])[-3:] if len(x["prev"]) < 8 else x["prev"][-3:]
            yield name, x["typed"], prevs


def main(binary):
    table = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
    bigrams = tm.Bloom.load(open(tm.PAIRS, "rb").read())
    re_.Lexicon.english = staticmethod(lambda w: table.common_in("e", tm.fold(w)))
    lex = re_.Lexicon(("de", "en"), (), False)
    rows = [s for s in samples() if s[1].isalpha() and all(" " not in p and "\t" not in p for p in s[2])]
    want = []
    for _, w, prevs in rows:
        p = tm.correction(table, bigrams, set(), w, lex.candidates(w) if len(w) >= tm.MIN_LEN else (),
                          lex.langs(w), prevs, nxt=None)
        want.append(p or "")
    with tempfile.TemporaryDirectory() as tmp:
        inp = os.path.join(tmp, "in.txt")
        with open(inp, "w", encoding="utf-8") as f:
            for _, w, prevs in rows:
                f.write(" ".join(prevs) + "\t" + w + "\n")
        tables = os.path.join(ROOT, "mobile", "keyboard", "src", "main", "assets", "sokki")
        lexdir = os.path.join(ROOT, "crates", "sokki-typos", "data")
        r = subprocess.run([binary, tables, lexdir, "words"], stdin=open(inp, encoding="utf-8"),
                           capture_output=True, text=True)
    sys.stderr.write(r.stderr)
    got = r.stdout.split("\n")[: len(rows)]
    per = {}
    bad = []
    for (name, w, prevs), a, b in zip(rows, want, got):
        n = per.setdefault(name, [0, 0, 0])
        n[0] += 1
        n[1] += bool(a)
        if a != b:
            n[2] += 1
            bad.append((name, prevs, w, a, b))
    for name, (n, fixes, diff) in per.items():
        print(f"{name:10} {n:6} words, {fixes:5} changed by the twin, {diff} answers differ")
    for x in bad[:15]:
        print("  differs:", json.dumps(x, ensure_ascii=False), file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
