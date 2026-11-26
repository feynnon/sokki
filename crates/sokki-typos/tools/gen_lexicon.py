"""Sokki: the word lists the Mac's typo model checks a typed word against.

On the phone, the keyboard's dictionaries tell autocorrect whether a typed word is a word and
how the dictionary spells it (Montag, Straße). The Mac has no keyboard dictionaries, so these
lists stand in for them: every word of the same dictionaries robin_eval.py emulates the phone
with (HeliBoard's main_de legacy and main_en_US word lists), one per line, as spelled there.

    python3 crates/sokki-typos/tools/gen_lexicon.py ~/sokki-typing-data/dicts crates/sokki-typos/data

`english-only.txt`: lower-case English words the legacy German list took in (and, is, these,
meeting) that the newer main_de dictionary doesn't have. Only the sentence's language guess uses
it: in the legacy list they look German, so an English sentence never leaned English (Robin,
26.11.2026: "in these cases" became "in These cases").
SPDX-License-Identifier: GPL-3.0-only
"""
import os
import sys

FILES = {"de": "main_de__legacy.combined", "en": "main_en_US.combined"}


def words(path):
    out = set()
    for line in open(path, encoding="utf-8"):
        if line.startswith(" word="):
            out.add(line[6:line.index(",")])
    return out


src, dst = sys.argv[1], sys.argv[2]
for lang, name in FILES.items():
    ws = sorted(words(os.path.join(src, name)))
    with open(os.path.join(dst, f"lexicon-{lang}.txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(ws) + "\n")
    print(lang, len(ws))


leg = words(os.path.join(src, FILES["de"]))
new = words(os.path.join(src, "main_de.combined"))
en = words(os.path.join(src, FILES["en"]))
eo = sorted(w for w in en if w in leg and w not in new and w.islower())
with open(os.path.join(dst, "english-only.txt"), "w", encoding="utf-8") as f:
    f.write("\n".join(eo) + "\n")
print("english-only", len(eo))
