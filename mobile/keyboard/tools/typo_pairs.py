"""Sokki: real typos from the keyboard's tap log (typing_log/taps-<date>.jsonl).

Replays the taps of each app as text. Wherever the user deleted letters and
typed again, the word as it was before the deletion ("typed") and the word as
it ended up ("meant") form a pair. Autocorrect fixes and undos in the log
(TapLog.logFix/logUndo) become pairs too: an undone fix is a fix that was wrong.

    python3 mobile/keyboard/tools/typo_pairs.py taps-*.jsonl > phone-pairs.jsonl

Each line: {"source": "self"|"fix"|"undo", "typed", "meant", "partial", "app", "ms"}.
"partial": the user stopped mid-word to correct, so "typed" is the start of a
word; compare it with as many letters of "meant".
Text the keyboard didn't type (dictation, paste, cursor moves) is not in the
taps, so a few pairs can be off; pairs are a sample, not a transcript.
SPDX-License-Identifier: GPL-3.0-only
"""

import json
import re
import sys

DELETE = -7
GAP_MS = 30_000  # a pause this long, or another app: a new piece of text
WORD = re.compile(r"[^\W\d_]+$")


def distance(a, b):
    """Damerau-Levenshtein distance (adjacent swaps count once)."""
    d = [[i + j if i * j == 0 else 0 for j in range(len(b) + 1)] for i in range(len(a) + 1)]
    for i in range(1, len(a) + 1):
        for j in range(1, len(b) + 1):
            d[i][j] = min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] != b[j - 1]))
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                d[i][j] = min(d[i][j], d[i - 2][j - 2] + 1)
    return d[-1][-1]


def word_at_end(text):
    m = WORD.search(text)
    return m.group(0) if m else ""


def pairs(events):
    text, app, last_ms = "", None, 0
    before = None  # text just before a run of deletions
    deleted_to = None  # its length after the run
    for e in events:
        t = e.get("t")
        if t in ("fix", "undo"):
            yield {"source": t, "typed": e["typed"], "meant": e["to"] if t == "fix" else e["typed"],
                   "partial": False, "app": e.get("app", ""), "ms": e["ms"]}
            continue
        if t != "tap":
            continue
        if e["app"] != app or e["ms"] - last_ms > GAP_MS:
            text, before = "", None
        app, last_ms = e["app"], e["ms"]
        code = e["out"]
        if code == DELETE:
            if before is None:
                before = text
            text = text[:-1]
            deleted_to = len(text)
            continue
        if code == 10:
            ch = "\n"  # enter ends a word too
        elif code < 32:
            continue
        else:
            ch = chr(code)
        # a capital right after a small letter: a new message began (sent with the app's button)
        new_text = ch.isupper() and text[-1:].islower()
        if before is not None and (not ch.isalpha() or new_text):
            # the corrected word is finished: compare what was there with what is there now
            start = deleted_to - len(word_at_end(text[:deleted_to]))
            m = re.match(r"[^\W\d_]+", before[start:])
            typed = m.group(0) if m else ""
            meant = word_at_end(text)
            if (len(typed) >= 2 and meant and typed.lower() != meant.lower()
                    and not meant.lower().startswith(typed.lower())):
                # stopped mid-word: the deleted text ended in a letter of the same word
                partial = start + len(typed) == len(before)
                compare = meant[: len(typed)] if partial else meant
                if distance(typed.lower(), compare.lower()) <= max(1, len(compare) // 3):
                        yield {"source": "self", "typed": typed, "meant": meant, "partial": partial,
                           "app": app, "ms": e["ms"]}
            before = None
        if new_text:
            text = ""
        text += ch


def main(paths):
    for path in paths:
        with open(path) as f:
            events = [json.loads(line) for line in f if line.strip()]
        for p in pairs(events):
            print(json.dumps(p, ensure_ascii=False))


if __name__ == "__main__":
    main(sys.argv[1:])
