"""Sokki: writes assets/sokki/letters_<lang>.bin for KeyTargeting.

Word frequencies from wordfreq (data CC-BY-SA 4.0, see LICENSE-CC-BY-SA-4.0):
    python3 -m venv /tmp/wf && /tmp/wf/bin/pip install wordfreq
    /tmp/wf/bin/python mobile/keyboard/tools/gen_letter_tables.py
SPDX-License-Identifier: GPL-3.0-only
"""

import os
import sys

from wordfreq import top_n_list, word_frequency

sys.path.insert(0, os.path.dirname(__file__))
import letter_model  # noqa: E402

OUT = os.path.join(os.path.dirname(__file__), "..", "src", "main", "assets", "sokki")
WORDS = 200_000

os.makedirs(OUT, exist_ok=True)
for lang in ("de", "en"):
    words = top_n_list(lang, WORDS)
    table = letter_model.build_table((w, word_frequency(w, lang)) for w in words)
    path = os.path.join(OUT, f"letters_{lang}.bin")
    with open(path, "wb") as f:
        f.write(table)
    print(path, len(table), "bytes")
