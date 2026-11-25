"""Sokki: score and time the keyboard's own decoder on a device (SokkiBench.kt, debug builds).

    python bench_device.py prepare N out-dir pairs-ctx.jsonl ...   # N typos + N correct words per file
    adb push out-dir/sokki_bench.jsonl /data/local/tmp/ && adb shell run-as dev.verbatim.mobile \
        cp /data/local/tmp/sokki_bench.jsonl files/   # then open the keyboard anywhere
    adb exec-out run-as dev.verbatim.mobile cat files/sokki_bench-out.jsonl > out-dir/out.jsonl
    python bench_device.py score out-dir/sokki_bench-meta.jsonl out-dir/out.jsonl

Scores per source: what autocorrect does ("auto": Typos), what HeliBoard alone
would do ("native"), and the time per word (us: the whole suggestion step,
usDecode: HeliBoard's decoder, usTypos: Typos).
SPDX-License-Identifier: GPL-3.0-only
"""

import collections
import json
import os
import random
import statistics
import sys

sys.path.insert(0, os.path.dirname(__file__))
import typo_model as tm  # noqa: E402


def prepare(n, out_dir, paths):
    os.makedirs(out_dir, exist_ok=True)
    random.seed(1)
    with open(os.path.join(out_dir, "sokki_bench.jsonl"), "w") as fb, \
            open(os.path.join(out_dir, "sokki_bench-meta.jsonl"), "w") as fm:
        for path in paths:
            items = [json.loads(x) for x in open(path)]
            items = [i for i in items if i["typed"].isalpha() and i["meant"].isalpha()]
            for group in ([i for i in items if i["typed"] != i["meant"]], [i for i in items if i["typed"] == i["meant"]]):
                k = max(1.0, len(group) / n)
                for i in random.sample(group, min(n, len(group))):
                    fb.write(json.dumps({"typed": i["typed"], "prev": i["prev"], "start": len(i["prev"]) < 8},
                                        ensure_ascii=False) + "\n")
                    fm.write(json.dumps({"source": os.path.basename(path), "typed": i["typed"], "meant": i["meant"],
                                         "weight": i.get("weight", 1) * k}, ensure_ascii=False) + "\n")


def score(meta_path, out_path):
    meta = [json.loads(x) for x in open(meta_path)]
    out = [json.loads(x) for x in open(out_path)]
    by = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    times = collections.defaultdict(list)
    for m, o in zip(meta, out):
        assert m["typed"] == o["typed"]
        for how in ("auto", "native"):
            if how not in o:
                continue
            p = o[how]
            if m["typed"] == m["meant"]:
                kind = "changed" if p and tm.fold(p) != tm.fold(m["typed"]) else "kept"
            elif not p or tm.fold(p) == tm.fold(m["typed"]):
                kind = "missed"
            else:
                kind = "right" if tm.fold(p) == tm.fold(m["meant"]) else "wrong"
            by[m["source"]][how][kind] += m["weight"]
        for k in ("us", "usDecode", "usTypos"):
            if k in o:
                times[k].append(o[k])
    for source, hows in by.items():
        print(source)
        for how, n in hows.items():
            typos = n["right"] + n["wrong"] + n["missed"] or 1
            words = n["kept"] + n["changed"] or 1
            print(f"  {how:7} right {100 * n['right'] / typos:5.1f}% wrong {100 * n['wrong'] / typos:5.1f}% "
                  f"missed {100 * n['missed'] / typos:5.1f}% | changed per 10k correct words "
                  f"{1e4 * n['changed'] / words:5.1f}")
    for k, v in times.items():
        v = sorted(v)
        print(f"{k:9} median {statistics.median(v) / 1000:6.2f} ms  p95 {v[int(.95 * len(v))] / 1000:6.2f} ms  "
              f"p99 {v[int(.99 * len(v))] / 1000:6.2f} ms  max {v[-1] / 1000:6.2f} ms")


if __name__ == "__main__":
    if sys.argv[1] == "prepare":
        prepare(int(sys.argv[2]), sys.argv[3], sys.argv[4:])
    else:
        score(sys.argv[2], sys.argv[3])
