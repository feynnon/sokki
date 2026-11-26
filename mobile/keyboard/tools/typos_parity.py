"""Sokki: does Typos.kt decide as typo_model.py does? Runs on the JVM, no phone needed.

    ~/.cache/sokki-eval-venv/bin/python mobile/keyboard/tools/typos_parity.py write   # build/typos-parity/in.tsv
    (cd mobile/src-tauri/gen/android && ./gradlew :keyboard:testDebugUnitTest --tests '*TyposTest*')
    ~/.cache/sokki-eval-venv/bin/python mobile/keyboard/tools/typos_parity.py check   # compares out.tsv

The cases: Robin's own words and the typing37k / 136m samples, as robin_eval.py feeds them, once on
the QWERTZ rows and once on the phone's own keys (tap log). They stay in mobile/keyboard/build
(ignored by git): Robin's typing never goes into the repo.
SPDX-License-Identifier: GPL-3.0-only
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import robin_eval as ev  # noqa: E402
import typo_model as tm  # noqa: E402

DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "build", "typos-parity")


def cases():
    toks, labels, jtoks, _ = ev.load_robin()
    personal = ev.personal_words(toks + jtoks, labels)
    lex = ev.Lexicon(("de", "en"), personal)
    for w, prev, nxt, prevs, ctx in toks + jtoks:
        if len(w) >= tm.MIN_LEN:
            yield w, prevs, nxt, lex.langs(w), lex.candidates(w), ctx
    plain = ev.Lexicon(("de", "en"))
    for name in ("typing37k", "136m"):
        for x in ev.sample(ev.evals.path(name, f"{name}-ctx.jsonl"), 1500, 2500):
            typed = x["typed"]
            if len(typed) >= tm.MIN_LEN:
                prevs = ([tm.BOS] + x["prev"])[-3:] if len(x["prev"]) < 8 else x["prev"][-3:]
                yield typed, prevs, x.get("next", ""), plain.langs(typed), plain.candidates(typed), x["prev"]


def write():
    os.makedirs(DIR, exist_ok=True)
    table = tm.Table(tm.Bloom.load(open(tm.TABLE, "rb").read()))
    bigrams = tm.Bloom.load(open(tm.PAIRS, "rb").read())
    ev.Lexicon.english = staticmethod(lambda w: table.common_in("e", tm.fold(w)))
    all_cases = [c for c in cases() if "\t" not in "".join(map(str, c))]
    with open(os.path.join(DIR, "in.tsv"), "w") as f, open(os.path.join(DIR, "expected.tsv"), "w") as e:
        for keys in (None, ev.phone_keys()):
            tm.set_keys(keys)
            spec = "none" if keys is None else ";".join(f"{c}:{','.join(map(str, v))}" for c, v in sorted(keys.items()))
            f.write(f"#keys\t{spec}\n")
            e.write(f"#keys\t{spec}\n")
            for typed, prevs, nxt, langs, cands, ctx in all_cases:
                ctx = ev.WORD.findall(" ".join(ctx))  # as the phone reads the text before the cursor
                p = tm.correction(table, bigrams, set(), typed, cands, langs, prevs, nxt=nxt or None, context=ctx)
                cand = " ".join(f"{c}/{lang or '-'}" for c, lang in cands)
                f.write("\t".join([typed, " ".join(prevs), nxt or "", ",".join(sorted(langs)), cand, " ".join(ctx)]) + "\n")
                e.write(f"{typed}\t{p or ''}\n")
        tm.set_keys(None)
    print(len(all_cases), "cases x 2 keyboards written to", DIR)


def check():
    exp = open(os.path.join(DIR, "expected.tsv")).read().splitlines()
    out = open(os.path.join(DIR, "out.tsv")).read().splitlines()
    assert len(exp) == len(out), (len(exp), len(out))
    diff = [(a, b) for a, b in zip(exp, out) if a != b]
    print(f"{len(diff)} of {len(exp)} decisions differ")
    for a, b in diff[:20]:
        print("  python", a.split("\t"), "kotlin", b.split("\t")[1:])
    return not diff


if __name__ == "__main__":
    {"write": write, "check": lambda: sys.exit(0 if check() else 1)}[sys.argv[1]]()
