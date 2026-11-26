"""Sokki: writes assets/sokki/words.bin, the word table for Typos (typos-only autocorrect).

Also writes assets/sokki/common.txt, the common words a typo may be two slips
from. Every word of wordfreq's large German and English lists (about 820,000,
colloquial ones like "geil" included) with its frequency in half-zipf steps,
which language has it commonly, whether it may be an autocorrect candidate, and
whether German writes it capitalised, stored as a Bloom filter (1 in 100,000 false
hits). See typo_model.py.

Word frequencies from wordfreq (data CC-BY-SA 4.0, see LICENSE-CC-BY-SA-4.0):
    python3 -m venv /tmp/wf && /tmp/wf/bin/pip install wordfreq
    /tmp/wf/bin/python mobile/keyboard/tools/gen_known_words.py
SPDX-License-Identifier: GPL-3.0-only
"""

import json
import math
import os
import sys

from wordfreq import get_frequency_dict

sys.path.insert(0, os.path.dirname(__file__))
import typo_model as tm  # noqa: E402

OUT = os.path.join(os.path.dirname(__file__), "..", "src", "main", "assets", "sokki", "words.bin")
FALSE_HITS = 1e-5


def zipf(f):
    return math.log10(f) + 9


freq = {lang: get_frequency_dict(lang, wordlist="large") for lang in ("de", "en")}
keys = []
for w in set(freq["de"]) | set(freq["en"]):
    if " " in w:
        continue
    z = zipf(max(freq["de"].get(w, 0), freq["en"].get(w, 0)))
    keys.append(f"{tm.bucket(z)}:{w}")
for lang, tag in (("de", "d"), ("en", "e")):
    keys += [f"{tag}:{w}" for w, f in freq[lang].items() if " " not in w and zipf(f) >= tm.COMMON]
# "w:<word>": words common enough to be an autocorrect candidate. Typos tries ~500
# one-slip strings per word: one probe each, and a false hit must also pass the
# zipf probes (one Bloom alone let "serveraqnce" through).
keys += [f"w:{w}" for w in set(freq["de"]) | set(freq["en"])
         if " " not in w and zipf(max(freq["de"].get(w, 0), freq["en"].get(w, 0))) >= tm.CAND_ZIPF]

# "C:<word>": German text writes it with a capital, 90%+ of its 10+ uses mid-sentence (truecasing;
# counts from gen_case_counts.py over Wikipedia + Tatoeba German, path in CASE_COUNTS).
cc = os.environ.get("CASE_COUNTS", os.path.expanduser("~/sokki-typing-data/corpora/case-de.json"))
keys += [f"C:{w}" for w, (up, low) in json.load(open(cc)).items() if up + low >= 10 and up >= 0.9 * (up + low)]

# common.txt: the words a typed word may be two slips from (typo_model.near_words), one per line,
# sorted: zipf 3+ in German or English, letters only.
common = sorted(w for w in set(freq["de"]) | set(freq["en"])
                if w.isalpha() and zipf(max(freq["de"].get(w, 0), freq["en"].get(w, 0))) >= tm.COMMON)
with open(tm.COMMON_WORDS, "w", encoding="utf-8") as f:
    f.write("\n".join(common) + "\n")
print(tm.COMMON_WORDS, len(common), "words")

bloom = tm.Bloom(len(keys), FALSE_HITS)
for k in keys:
    bloom.add(k)
data = bloom.dump()
with open(OUT, "wb") as f:
    f.write(data)
print(OUT, len(keys), "keys,", len(data), "bytes, k =", bloom.k)
