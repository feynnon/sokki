"""Sokki: downloads the public eval sets into $SOKKI_EVALS (default ~/sokki-evals).

Every file comes from its original source (eval_sources.json lists source and SHA-256), so
nothing is re-hosted. Files already there are kept; --verify checks their checksums too.
Private sets can't be fetched: bring your own (docs/evals.md, "Test on your own data").

    python3 mobile/keyboard/tools/fetch_evals.py --list
    python3 mobile/keyboard/tools/fetch_evals.py de-clean-10k heliboard-dicts
    python3 mobile/keyboard/tools/fetch_evals.py --all          (about 4.5 GB)
SPDX-License-Identifier: GPL-3.0-only
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
import tarfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import evals  # noqa: E402

SOURCES = json.load(open(os.path.join(HERE, "eval_sources.json"), encoding="utf-8"))["files"]

# What to run once a set's files are there.
NEXT = {
    "typing37k": "python3 mobile/keyboard/tools/public_pairs.py typing37k {d} > {d}/typing37k-ctx.jsonl",
    "136m": "python3 mobile/keyboard/tools/public_pairs.py 136m {d} > {d}/136m-ctx.jsonl",
    "de-clean-10k": "python3 mobile/keyboard/tools/score_de_clean.py <sokki-typos binary>",
    "text-corpora": "python3 mobile/keyboard/tools/ngram_lm.py (see its docstring) to build the LMs",
}


def sha256(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def fetch(e, verify):
    d = os.path.join(evals.ROOT, e["set"], "data")
    p = os.path.join(d, e["file"])
    if os.path.exists(p) and not verify:
        print(f"  have   {e['file']}")
        return True
    if not os.path.exists(p):
        os.makedirs(os.path.dirname(p), exist_ok=True)
        print(f"  fetch  {e['file']}  <- {e['url']}", flush=True)
        req = urllib.request.Request(e["url"], headers={"User-Agent": "sokki-fetch-evals"})
        try:
            with urllib.request.urlopen(req) as r, open(p + ".part", "wb") as f:
                shutil.copyfileobj(r, f, 1 << 20)
        except Exception as x:  # noqa: BLE001
            print(f"  FAILED {e['file']}: {x}. Download it by hand from {e['url']} into {d}.")
            return False
        os.replace(p + ".part", p)
    want = e.get("sha256")
    if want and not e.get("live"):
        got = sha256(p)
        if got != want:
            print(f"  DIFFERS {e['file']}: sha256 {got}, Sokki's results used {want}. "
                  "The source changed; numbers may not match docs/research.")
    if e.get("note"):
        print(f"         note: {e['note']}")
    if e.get("unpack"):
        name = e["file"][: -len(".tar.gz")]
        if not os.path.isdir(os.path.join(d, name)):
            print(f"  unpack {e['file']}", flush=True)
            with tarfile.open(p) as t:
                t.extractall(d, filter="data")
    return True


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sets", nargs="*")
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--verify", action="store_true", help="check the checksums of files already there")
    a = ap.parse_args()
    names = list(dict.fromkeys(e["set"] for e in SOURCES))
    if a.list or not (a.sets or a.all):
        print(f"Public sets (into {evals.ROOT}):")
        for n in names:
            print(f"  {n:16} {sum(e['set'] == n for e in SOURCES)} files")
        return
    want = names if a.all else a.sets
    bad = [n for n in want if n not in names]
    if bad:
        sys.exit(f"not a public set: {', '.join(bad)}. Public: {', '.join(names)}. "
                 "Private sets can't be fetched; see docs/evals.md.")
    ok = True
    for n in want:
        print(n)
        ok &= all([fetch(e, a.verify) for e in SOURCES if e["set"] == n])
        if n in NEXT:
            print("  next:  " + NEXT[n].format(d=os.path.join(evals.ROOT, n, "data")))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
