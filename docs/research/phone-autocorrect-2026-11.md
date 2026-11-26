# Phone autocorrect: keys on screen, British spellings, two slips (26 Nov 2026)

**Task** (Robin, 26 Nov 2026, after the Mac fixes in `mac-autocorrect-2026-11.md`): give the phone keyboard the same fixes. Read the keyboard layout in use instead of assuming one, and try judging the text's language from the words typed. Measured with `mobile/keyboard/tools/robin_eval.py` on the Python twin (`typo_model.py`): Robin's labelled typos (382: 375 from his messages, 7 from the phone journal), his correct words and capital truth (`~/sokki-typing-data`), and 4,000-word samples of typing37k and 136m. Numbers only.

## What the phone did

- Neighbouring keys came from fixed QWERTZ rows, with ü, ö and ä as keys. Robin's phone (German, `qwertz+`, read from his tap log) has no umlaut keys, and its rows sit differently: y is under s and x under d, a full key further right than the rows assumed.
- British -ise words (optimised, prioritise) are rare in wordfreq's English list (zipf 2.25–2.75), so they were not protected, and a word one slip away could replace them.
- A word with no candidate one slip away got none at all.
- The language of a sentence comes from its last three words (and the next one, if typed).

## Changes

- **Keys on screen:** `Typos.useKeyboard` reads the letter keys of the layout on screen (centre, width, height) whenever it changes. A key is a neighbour when it sits beside it in the same row, or overlaps it sideways in the row above or below. It works for any layout HeliBoard draws (QWERTY, QWERTZ, AZERTY, with or without umlaut keys). The y/z swap habit stays only when z is in the top row. Log line: `keyboard keys set (26 letters, …)`.
- **British spellings:** as on the Mac, an -ise/-yse word is a word when English has its -ize/-yze form.
- **Two slips:** as on the Mac, when no word is one slip away, common words (zipf ≥ 3) two slips away compete, and the same 95 % share decides. The phone's word table is a Bloom filter that can't list words, so the 60,564 candidates ship as `assets/sokki/common.txt` (555 KB), written by `gen_known_words.py`.

## Measured

Before → after (all three on). The phone decides before the next word is typed. With the next word known, the numbers are the same except wrong capitals (104 instead of 109).

| Set | What | Before | After |
|---|---|---|---|
| Robin, 382 typos | fixed right | 74.1 % (283) | 74.9 % (286) |
| Robin, 382 typos | fixed to a wrong word | 0.5 % | 0.5 % |
| Robin | correct words changed | 25 | 24 |
| Robin | lower-case nouns capitalised / wrong capitals | 66.3 % / 109 | 66.3 % / 109 |
| typing37k, 1,500 typos | fixed right / fixed wrong | 37.1 % / 1.1 % | 37.6 % / 1.1 % |
| 136m, 1,500 typos | fixed right / fixed wrong | 39.1 % / 1.0 % | 40.0 % / 0.9 % |
| both, 2,500 correct words each | changed | 0 / 1 | 0 / 1 |
| Python twin, per word | median / 95th percentile | 0.25 / 0.67 ms | 0.25 / 0.68 ms |

Per change (each alone, against "before"):

| Change | Robin's typos right | Correct words changed | typing37k right | 136m right / wrong |
|---|---|---|---|---|
| Keys on screen | 74.1 → 74.9 % | 25 | 37.1 → 37.3 % | 39.1 % / 1.0 → 0.9 % |
| British spellings | 74.1 % | 25 → 24 | 37.1 % | 39.1 % / 1.0 % |
| Two slips | 74.1 % | 25 | 37.1 → 37.4 % | 39.1 → 40.0 % / 1.0 % |

In the umlaut test (Robin's umlaut words typed without umlauts), two slips changed 6 more words: 22 instead of 16. Five of those words already had a typo in Robin's text, and two slips fixed both the umlaut and the typo. The sixth is a verb stem that became the verb (made-up example of the kind: "Präsentier" → "Präsentiert").

## Language from the whole text: tried, left off

The text's last 10, 20 or 40 words before the sentence counted for its language too, weighted by their share German minus English (×0.5, ×1, ×2). Measured with the next word known, before the other changes:

| Variant | Robin's typos right | Nouns capitalised | Wrong capitals |
|---|---|---|---|
| Sentence only (shipped) | 74.1 % | 66.2 % | 104 |
| + last 10, 20 or 40 words | 73.8 % | 66.2–66.3 % | 104 |
| + last 20 words ×0.5 | 73.8 % | 66.1 % | 104 |
| + last 20 words ×2 | 73.8 % | 66.5 % | 106 |

The sentence's own words already decide its language. The wrong capitals come from English words inside German sentences ("das easy"-type phrases), where the sentence really is German. The code stays in, switched off (`LANG_CONTEXT = 0` in `Typos.kt` and `typo_model.py`).

## Checks

- `tools/typos_parity.py` compares Typos.kt with the Python twin on the JVM (`TyposTest`), on Robin's words and both public samples, on QWERTZ rows and on his phone's keys: 0 of 95,984 decisions differ (2.2 s on the Mac's JVM for all of them).
- Not yet run on a phone: the keyboard has to be built and installed, and `bench_device.py` timed on the Pixel.

## Sources

- typing37k: Aalto 37k typing test (phone keyboards). 136m: Dhakal et al. 2018, 136 million keystrokes (hardware keyboards).
- wordfreq (Speer, CC-BY-SA 4.0) for the word lists.
- HeliBoard layouts: `mobile/keyboard/src/main/assets/layouts/main/qwertz.txt`.
