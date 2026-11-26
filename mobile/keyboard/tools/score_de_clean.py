"""Counts the edits Sokki's Mac typo model makes to de-clean-10k (edited German: nearly every
edit is wrong), and compares two builds.

    cd crates/sokki-typos && cargo build --release --features cli
    python3 mobile/keyboard/tools/score_de_clean.py <sokki-typos binary A> [<binary B>]

The model's tables and word lists come from this checkout; the sentences from the eval set
de-clean-10k (fetch_evals.py de-clean-10k). Prints edits per kind
(capital added/removed, word changed; "capital" also counts answers that equal the typed word)
and, with two builds, the edits only one of them makes, with a few examples. Public text only.
SPDX-License-Identifier: GPL-3.0-only
"""
import bisect, collections, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import evals  # noqa: E402

repo = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
bins = sys.argv[1:]
if not bins:
    sys.exit(__doc__)
rows = [json.loads(l) for l in open(evals.path("de-clean-10k", "de-clean-10k.jsonl"), encoding="utf-8")]
text = "\n".join(r["text"] for r in rows) + "\n"
starts, pos = [], 0
for r in rows:
    starts.append(pos)
    pos += len(r["text"]) + 1


def run(b):
    out = subprocess.run([b, os.path.join(repo, "mobile/keyboard/src/main/assets/sokki"),
                          os.path.join(repo, "crates/sokki-typos/data"), "fixes"],
                         input=text, capture_output=True, text=True).stdout
    edits = set()
    for line in out.splitlines():
        s, typed, fix = line.split("\t")
        edits.add((bisect.bisect_right(starts, int(s)) - 1, int(s), typed, fix))
    return edits


def kind(t, f):
    if t == f:
        return "same word back"
    if t.lower() == f.lower():
        return "capital added" if f[:1].isupper() else "capital removed"
    return "word changed"


res = []
for b in bins:
    e = run(b)
    res.append(e)
    c = collections.Counter(kind(t, f) for _, _, t, f in e)
    print(f"{b}: {len(e)} edits in {len({i for i, *_ in e})} of {len(rows)} sentences; {dict(c)}")
if len(res) == 2:
    for tag, s in (("only B", res[1] - res[0]), ("only A", res[0] - res[1])):
        print(f"{tag}: {len(s)} {dict(collections.Counter(kind(t, f) for *_, t, f in s))}")
        for i, p, t, f in sorted(s)[:40]:
            o = p - starts[i]
            print(f"   {t} -> {f} | ...{rows[i]['text'][max(0, o - 40):o + len(t) + 20]}")
