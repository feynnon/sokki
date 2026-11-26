#!/usr/bin/env python3
"""Evaluates LLM correction of meeting transcripts on real calls.

Data: the private eval set real-calls, $SOKKI_EVALS/real-calls/data/<call>/ (default ~/sokki-evals;
docs/evals.md):
  raw.json     uncorrected transcript (crates/meeting-e2e/examples/raw_transcript.rs)
  labels.json  {"fixes": [{"line": 12, "wrong": "Cloud Code", "right": "Claude Code"}],
                "keep":  [{"line": 30, "text": "Patird"}]}
               fixes: misrecognitions a good correction must repair;
               keep:  correct but unusual words it must leave alone
                      (customer names, Denglisch).

Scores per variant: fixed / total fixes, broken keeps, and every other change
(printed for review; not automatically right or wrong).

  python3 -I scripts/correction_eval.py --model qwen3.5:9b --mode rewrite --window 8
"""

import argparse
import difflib
import json
import os
import re
import sys
import time
import urllib.request
from pathlib import Path

REJECTED = []
REAL = Path(os.environ.get("SOKKI_EVALS", "~/sokki-evals")).expanduser() / "real-calls" / "data"
if not REAL.is_dir():
    sys.exit(f"eval set 'real-calls' is missing: {REAL} not found.\n"
             "  How to get it: docs/evals.md, section 'real-calls'. Sets elsewhere? Set SOKKI_EVALS.")
OLLAMA = "http://127.0.0.1:11434/api/chat"


def chat(model, system, user, temperature, seed, think, timeout=600):
    body = {
        "model": model,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
        "stream": False,
        "think": think,
        "options": {"temperature": temperature, "seed": seed, "num_ctx": 4096},
        "keep_alive": "5m",
    }
    req = urllib.request.Request(OLLAMA, json.dumps(body).encode(), {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        msg = json.load(r)["message"]
    return msg.get("content", "")


def extract_json(text):
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.S)
    start = text.find("{")
    if start < 0:
        raise ValueError(f"no JSON in reply: {text[:200]!r}")
    obj, _ = json.JSONDecoder().raw_decode(text[start:])  # ignores anything after the first object
    return obj


def render(segments, a, b, numbered=True):
    out = []
    for i in range(a, b):
        s = segments[i]
        prefix = f"[{i}] " if numbered else ""
        out.append(f"{prefix}{s['speaker']}: {s['text']}")
    return "\n".join(out)


GLOSSARY_LINE = (
    "Terms and names that occur in this user's meetings (correct spelling): {terms}."
)


def system_prompt(mode, glossary, names):
    terms = ", ".join(glossary)
    known = ""
    if names:
        known = f"\nNames of people and companies that are correct as written, never change them: {', '.join(names)}."
    base = (
        "You correct speech-recognition errors in a German business meeting transcript "
        "(German with many English business terms). The recogniser sometimes writes a "
        "similar-sounding wrong word, splits or merges words, or misspells names and jargon.\n"
        + GLOSSARY_LINE.format(terms=terms)
        + known
        + "\nRules:\n"
        "- Fix only clear recognition errors: a word that makes no sense here and a similar-sounding word that does.\n"
        "- Keep everything else exactly as spoken: wording, grammar, filler words, Denglisch verbs "
        "(e.g. 'raisen', 'waiven', 'droppen'), colloquial speech. Do not rephrase, do not translate.\n"
        "- Names you don't know are not errors. Only change a name if it is clearly a misheard term from the list above.\n"
        "- If you cannot tell what was said, leave the words unchanged.\n"
    )
    if mode in ("edits", "verify"):
        return base + (
            "- Most lines have no errors. An empty list is the expected answer for most chunks.\n"
            "- When uncertain, preserve the original wording.\n"
            "Answer with JSON only: {\"corrections\": [{\"line\": <number>, \"from\": \"<exact words in that line>\", "
            "\"to\": \"<replacement>\", \"reason\": \"<why the original cannot be right>\"}]}. "
            "Return {\"corrections\": []} if nothing needs fixing."
        )
    if mode == "edits_old":
        return base + (
            "Answer with JSON only: {\"corrections\": [{\"line\": <number>, \"from\": \"<exact words in that line>\", "
            "\"to\": \"<replacement>\"}]}. Return {\"corrections\": []} if nothing needs fixing."
        )
    return base + (
        "Answer with JSON only: {\"lines\": {\"<number>\": \"<corrected full text of that line>\"}} containing "
        "ONLY the lines you changed (without the speaker name). Return {\"lines\": {}} if nothing needs fixing."
    )


def user_prompt(segments, a, b, context):
    parts = []
    c = max(0, a - context)
    if c < a:
        parts.append("Context (already corrected, do not change):\n" + render(segments, c, a))
    parts.append("Correct these lines:\n" + render(segments, a, b))
    return "\n\n".join(parts)


def replace_word(text, wrong, right):
    """Replaces every whole-word occurrence (spaces or hyphens between words may differ)."""
    return pattern(wrong).sub(lambda m: right, text)



def verify_system(glossary, names):
    return (
        "You check proposed corrections to a speech-recognition transcript of a German business meeting "
        "(German with English business terms, colloquial speech, Denglisch verbs like 'raisen', 'droppen').\n"
        f"Known terms: {', '.join(glossary)}.\n"
        + (f"Known names (correct as written): {', '.join(names)}.\n" if names else "")
        + "For each proposal decide: is the ORIGINAL wording plausibly what the speaker really said "
        "(a real word that fits, a person's or company's name, an English word, Denglisch, colloquial speech)? "
        "Then keep the original. Accept the correction only if the original makes no sense in this sentence "
        "AND the correction is clearly what was said (it sounds similar).\n"
        "Answer with JSON only: {\"decisions\": [{\"id\": <number>, \"accept\": true|false, \"reason\": \"<short>\"}]}"
    )


def verify(segments, proposals, args, glossary, names):
    """Second pass: returns the proposals the model accepts."""
    if not proposals:
        return []
    items = []
    for k, (i, frm, to) in enumerate(proposals):
        items.append(f"{k}. line: \"{segments[i]['text']}\"\n   change \"{frm}\" -> \"{to}\"")
    reply = chat(args.model, verify_system(glossary, names), "\n".join(items), 0.0, args.seed, False)
    data = extract_json(reply)
    ok = set()
    for d in data.get("decisions", []):
        try:
            if d.get("accept") is True:
                ok.add(int(d.get("id")))
        except (TypeError, ValueError):
            pass
    return [p for k, p in enumerate(proposals) if k in ok]


def propose(out, a, b, args, system):
    """One sampling of edit proposals for lines a..b: list of (line, from, to)."""
    user = user_prompt(out, a, b, args.context)
    reply = chat(args.model, system, user, args.temperature, args.seed, args.think)
    data = extract_json(reply)
    res = []
    for c in data.get("corrections", []):
        try:
            i = int(c.get("line"))
        except (TypeError, ValueError):
            continue
        frm, to = str(c.get("from") or "").strip(), str(c.get("to") or "").strip()
        if a <= i < b and frm and to and frm != to and pattern(frm).search(out[i]["text"]):
            res.append((i, frm, to))
    return res


def koelner(word):
    """Kölner Phonetik: German sound code (similar-sounding words get similar codes)."""
    w = word.lower().replace("ä", "a").replace("ö", "o").replace("ü", "u").replace("ß", "s")
    w = re.sub(r"[^a-z]", "", w)
    codes = []
    for i, ch in enumerate(w):
        prev = w[i - 1] if i > 0 else ""
        nxt = w[i + 1] if i + 1 < len(w) else ""
        if ch in "aeijouy":
            c = "0"
        elif ch == "h":
            c = ""
        elif ch == "b":
            c = "1"
        elif ch == "p":
            c = "3" if nxt == "h" else "1"
        elif ch in "dt":
            c = "8" if nxt in ("c", "s", "z") and nxt else "2"
        elif ch in "fvw":
            c = "3"
        elif ch in "gkq":
            c = "4"
        elif ch == "c":
            if i == 0:
                c = "4" if nxt and nxt in "ahkloqrux" else "8"
            else:
                c = "8" if prev in ("s", "z") or not (nxt and nxt in "ahkoqux") else "4"
        elif ch == "x":
            c = "8" if prev in ("c", "k", "q") else "48"
        elif ch == "l":
            c = "5"
        elif ch in "mn":
            c = "6"
        elif ch == "r":
            c = "7"
        elif ch in "sz":
            c = "8"
        else:
            c = ""
        codes.append(c)
    out = ""
    for c in "".join(codes):
        if not out or out[-1] != c:
            out += c
    return out[:1] + out[1:].replace("0", "") if out else ""


def lev(a, b):
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


def compact(s):
    return re.sub(r"[\W_]", "", s.lower())


def stem(w):
    for e in sorted(ENDINGS, key=len, reverse=True):
        if e and w.endswith(e) and len(w) - len(e) >= 4:
            return w[: -len(e)]
    return w


def guard(heard, corr):
    """None if the change may be applied, else the reason it is rejected."""
    h, c = compact(heard), compact(corr)
    if not h or not c:
        return "empty"
    if h == c:
        return None  # spacing or hyphens only
    if min(len(h), len(c)) >= 4 and (h in c or c in h):
        return "one contains the other (longer/shorter form, not a mishearing)"
    k = 0
    while k < min(len(h), len(c)) and h[k] == c[k]:
        k += 1
    if k >= 5 and h[k:] in ENDINGS and c[k:] in ENDINGS:
        return "only the ending differs"
    if stem(h) == stem(c):
        return "only the ending differs"
    kh, kc = koelner(heard), koelner(corr)
    if kh and kc and lev(kh, kc) / max(len(kh), len(kc)) > args_sound:
        return f"does not sound alike ({kh} vs {kc})"
    return None


args_sound = 0.35
ENDINGS = {'', 'e', 'n', 'en', 'er', 'ern', 'es', 's', 't', 'te', 'ten', 'st', 'ing', 'ed', 'ers', 'ung', 'ier', 'eer', 'ng'}


def terms_system(dictionary):
    return (
        "You find places in a German business meeting transcript where the speech recogniser misheard a word "
        "from the user's dictionary. Dictionary (correct spellings): " + ", ".join(dictionary) + ".\n"
        "Report a span only if BOTH hold: it sounds similar to a dictionary entry (also misspellings like "
        "'OPDA' for 'OVDA', split or merged words), AND that entry makes sense in the sentence.\n"
        "Do not report anything else: other errors, grammar, English words, Denglisch, or names that are not "
        "in the dictionary. Most lines contain no such span.\n"
        "Answer with JSON only: {\"matches\": [{\"line\": <number>, \"heard\": \"<exact words in the line>\", "
        "\"correct\": \"<the words as they should be written, using the dictionary entry>\"}]}"
    )


def terms_verify_system(dictionary):
    return (
        "You check suspected speech-recognition errors in a German business meeting transcript. "
        "User's dictionary: " + ", ".join(dictionary) + ".\n"
        "For each item decide whether the speaker most likely said the suggested words instead of the "
        "recognised ones, judging by sound AND meaning in the sentence. If the recognised words are a real "
        "word that fits, an English or Denglisch word, or a name, answer false.\n"
        "Answer with JSON only: {\"decisions\": [{\"id\": <number>, \"accept\": true|false}]}"
    )


def contains_entry(text, dictionary):
    return any(pattern(e).search(text) for e in dictionary)


def terms_pass(out, a, b, args, dictionary):
    user = user_prompt(out, a, b, args.context)
    reply = chat(args.model, terms_system(dictionary), user, args.temperature, args.seed, args.think)
    data = extract_json(reply)
    res = []
    for m in data.get("matches", []):
        try:
            i = int(m.get("line"))
        except (TypeError, ValueError):
            continue
        heard, corr = str(m.get("heard") or "").strip(), str(m.get("correct") or "").strip()
        if not (a <= i < b and heard and corr and heard != corr):
            continue
        if not pattern(heard).search(out[i]["text"]):
            continue  # not in the line
        if not any(compact(e) in compact(corr) for e in dictionary):
            continue  # the fix must use a dictionary entry (also inflected: "USPs")
        if contains_entry(heard, dictionary):
            continue  # the words already are a dictionary entry (e.g. a known name)
        if len(heard.split()) > 4:
            continue
        why = guard(heard, corr) if args.guards else None
        if why:
            REJECTED.append((i, heard, corr, why))
            continue
        res.append((i, heard, corr))
    return res


def terms_verify(out, props, args, dictionary):
    if not props:
        return []
    items = [f"{k}. sentence: \"{out[i]['text']}\"\n   recognised \"{h}\" -> suggested \"{c}\""
             for k, (i, h, c) in enumerate(props)]
    data = extract_json(chat(args.model, terms_verify_system(dictionary), "\n".join(items), 0.0, args.seed, False))
    ok = {int(d["id"]) for d in data.get("decisions", []) if d.get("accept") is True and str(d.get("id", "")).isdigit()}
    return [p for k, p in enumerate(props) if k in ok]


def correct(segments, args, glossary, names):
    out = [dict(s) for s in segments]
    system = system_prompt(args.mode, glossary, names)
    n = len(out)
    calls = 0
    errors = []
    windows = [(a, min(n, a + args.window)) for a in range(0, n, args.window)]
    if args.shifted and args.mode in ("terms", "terms_verify"):
        h = args.window // 2
        windows += [(0, min(n, h))] + [(a, min(n, a + args.window)) for a in range(h, n, args.window)]
    for a, b in windows:
        if args.mode in ("terms", "terms_verify"):
            dictionary = glossary + names
            try:
                props = terms_pass(out, a, b, args, dictionary)
                calls += 1
                if args.mode == "terms_verify" and props:
                    props = terms_verify(out, props, args, dictionary)
                    calls += 1
            except Exception as e:  # noqa: BLE001
                errors.append(f"lines {a}-{b - 1}: {e}")
                continue
            for i, h, c in props:
                out[i]["text"] = replace_word(out[i]["text"], h, c)
            continue
        if args.mode in ("edits", "verify"):
            votes = {}
            for k in range(args.samples):
                try:
                    one = propose(out, a, b, argparse.Namespace(**{**vars(args), "seed": args.seed + k}), system)
                    calls += 1
                except Exception as e:  # noqa: BLE001 - report every failure
                    errors.append(f"lines {a}-{b - 1}: {e}")
                    continue
                for p in set(one):
                    votes[p] = votes.get(p, 0) + 1
            props = [p for p, v in votes.items() if v >= args.min_votes]
            if args.mode == "verify" and props:
                try:
                    props = verify(out, props, args, glossary, names)
                    calls += 1
                except Exception as e:  # noqa: BLE001
                    errors.append(f"verify lines {a}-{b - 1}: {e}")
                    props = []
            for i, frm, to in props:
                out[i]["text"] = replace_word(out[i]["text"], frm, to)
            continue
        user = user_prompt(out, a, b, args.context)
        try:
            reply = chat(args.model, system, user, args.temperature, args.seed, args.think)
            calls += 1
            data = extract_json(reply)
        except Exception as e:  # noqa: BLE001 - report every failure
            errors.append(f"lines {a}-{b - 1}: {e}")
            continue
        if args.mode == "edits_old":
            for c in data.get("corrections", []):
                try:
                    i = int(c.get("line"))
                except (TypeError, ValueError):
                    continue
                if a <= i < b and c.get("from") and c.get("to") is not None:
                    new = replace_word(out[i]["text"], str(c["from"]).strip(), str(c["to"]).strip())
                    out[i]["text"] = new
        else:
            for k, v in (data.get("lines") or {}).items():
                try:
                    i = int(str(k).strip("[] "))
                except ValueError:
                    continue
                if a <= i < b and isinstance(v, str) and v.strip():
                    v = re.sub(r"^\s*[^:]{1,40}:\s+", "", v) if v.startswith(out[i]["speaker"] + ":") else v
                    out[i]["text"] = v.strip()
    return out, calls, errors


def word_changes(old, new):
    a, b = old.split(), new.split()
    sm = difflib.SequenceMatcher(a=a, b=b, autojunk=False)
    for op, i1, i2, j1, j2 in sm.get_opcodes():
        if op != "equal":
            yield " ".join(a[i1:i2]), " ".join(b[j1:j2])


def norm(s):
    return re.sub(r"[^\w\s-]", "", s).lower()


def pattern(phrase):
    """Whole-word, case-insensitive; words may be joined by spaces or hyphens."""
    words = re.findall(r"\w+", phrase)
    return re.compile(r"(?<!\w)" + r"[\s-]*".join(map(re.escape, words)) + r"(?!\w)", re.I)


def align(raw, out):
    """`out` re-indexed to the lines of `raw` by start time and track (for
    transcripts from another speech model with the same diarization). Lines
    missing in `out` become empty."""
    if len(raw) == len(out) and all(abs(r["start_s"] - o["start_s"]) < 0.01 for r, o in zip(raw, out)):
        return out
    by_key = {(o.get("track"), round(o["start_s"], 2)): o for o in out}
    res = []
    for r in raw:
        o = by_key.get((r.get("track"), round(r["start_s"], 2)))
        res.append(o if o else {**r, "text": ""})
    return res


def score(raw, out, labels):
    out = align(raw, out)
    fixes = labels.get("fixes", [])
    keeps = labels.get("keep", [])
    fixed, missed = [], []
    for f in fixes:
        line = out[f["line"]]["text"]
        if pattern(f["right"]).search(line) and not pattern(f["wrong"]).search(line):
            fixed.append(f)
        else:
            missed.append(f)
    broken = [k for k in keeps if not pattern(k["text"]).search(out[k["line"]]["text"])]
    explained = [(f["line"], norm(f["wrong"]), norm(f["right"])) for f in fixes]
    other = []
    for i, (r, o) in enumerate(zip(raw, out)):
        for frm, to in word_changes(r["text"], o["text"]):
            if norm(frm).replace("-", " ").split() == norm(to).replace("-", " ").split():
                continue  # punctuation / case / hyphen only
            if any(i == l and (w in norm(frm) or norm(frm) in w or rt in norm(to)) for (l, w, rt) in explained):
                continue
            other.append((i, frm, to))
    return fixed, missed, broken, other


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="qwen3.5:9b")
    ap.add_argument("--mode", choices=["edits", "verify", "rewrite", "terms", "terms_verify"], default="verify")
    ap.add_argument("--samples", type=int, default=1)
    ap.add_argument("--shifted", action="store_true", help="second pass with windows shifted by half")
    ap.add_argument("--no-guards", dest="guards", action="store_false")
    ap.add_argument("--min-votes", type=int, default=1)
    ap.add_argument("--window", type=int, default=8)
    ap.add_argument("--context", type=int, default=4)
    ap.add_argument("--temperature", type=float, default=0.0)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--think", action="store_true")
    ap.add_argument("--no-names", action="store_true", help="leave out the known-names line")
    ap.add_argument("--calls", nargs="*", default=None)
    ap.add_argument("--quiet", action="store_true")
    ap.add_argument("--asr", action="store_true", help="with --score-only: the file is another speech model's raw transcript")
    ap.add_argument("--score-only", metavar="FILE", help="score an existing output file (e.g. out_rust.json) in each call folder")
    args = ap.parse_args()

    cfg = json.loads((REAL / "config.json").read_text())
    glossary = cfg["glossary"]
    names = [] if args.no_names else cfg.get("names", [])
    tot_fix = tot_fixed = tot_broken = tot_other = 0
    t0 = time.time()
    for d in sorted(REAL.iterdir()):
        if not d.is_dir() or (args.calls and d.name not in args.calls):
            continue
        raw = json.loads((d / "raw.json").read_text())["segments"]
        labels = json.loads((d / "labels.json").read_text())
        if args.score_only:
            f = d / args.score_only
            if not f.exists():
                print(f"== {d.name}: {f.name} missing")
                continue
            out, calls, errors = json.loads(f.read_text())["segments"], 0, []
            if args.asr:
                out_al = align(raw, out)
                missing = sum(1 for o in out_al if not o["text"])
                fixed, missed, broken, _ = score(raw, out, labels)
                tot_fix += len(labels.get("fixes", [])); tot_fixed += len(fixed); tot_broken += len(broken)
                print(f"== {d.name}: right already {len(fixed)}/{len(labels.get('fixes', []))}, keeps lost {len(broken)}, lines empty/unaligned {missing}/{len(raw)}")
                if not args.quiet:
                    for f_ in fixed:
                        print(f"   right   [{f_['line']}] {out_al[f_['line']]['text'][:110]!r}")
                    for k in broken:
                        print(f"   LOST    [{k['line']}] {k['text']!r}: {out_al[k['line']]['text'][:110]!r}")
                continue
        else:
            out, calls, errors = correct(raw, args, glossary, names)
        fixed, missed, broken, other = score(raw, out, labels)
        tot_fix += len(labels.get("fixes", []))
        tot_fixed += len(fixed)
        tot_broken += len(broken)
        tot_other += len(other)
        print(f"== {d.name}: fixed {len(fixed)}/{len(labels.get('fixes', []))}, broken {len(broken)}, other changes {len(other)}, {calls} calls")
        for e in errors:
            print("   ERROR", e)
        if not args.quiet:
            for (i, h, c, why) in REJECTED:
                print(f"   guard   [{i}] {h!r} -> {c!r}: {why}")
            REJECTED.clear()
            for f in missed:
                print(f"   missed  [{f['line']}] {f['wrong']!r} -> {f['right']!r}: now {out[f['line']]['text'][:110]!r}")
            for k in broken:
                print(f"   BROKEN  [{k['line']}] {k['text']!r}: now {out[k['line']]['text'][:110]!r}")
            for i, frm, to in other:
                print(f"   other   [{i}] {frm!r} -> {to!r}")
        (d / f"out_{args.model.replace(':', '_')}_{args.mode}_w{args.window}_t{args.temperature}_n{args.samples}{'_shift' if args.shifted else ''}.json").write_text(
            json.dumps({"segments": out}, ensure_ascii=False, indent=1))
    print(f"TOTAL {args.model} {args.mode} window={args.window} ctx={args.context} temp={args.temperature} "
          f"seed={args.seed} samples={args.samples}/{args.min_votes} names={not args.no_names} guards={args.guards} shifted={args.shifted}: fixed {tot_fixed}/{tot_fix}, broken {tot_broken}, "
          f"other {tot_other}, {time.time() - t0:.0f}s")


if __name__ == "__main__":
    sys.exit(main())
