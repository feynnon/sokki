# Research: Mac autocorrect in other apps (Chrome, Electron)

**Date:** 25–26 Nov 2026.

**Task:** make Sokki's Mac autocorrect (`crates/sokki-autocorrect`) fix typos in Chrome and in Electron apps such as Claude and Slack, not only in Apple's own text views. Measure it on a real Mac with a lab app, and measure whether it costs Slack or Claude noticeable CPU or memory.

## Result

**What macOS does by itself:** its own autocorrect replaces words only in Apple's text views (Apple apps). In Chrome and Electron apps it only underlines.

**How Sokki gets into Chrome's text fields (Accessibility):**
- Chrome shows its web fields to Accessibility only after `AXEnhancedUserInterface` is set on the app (screen-reader mode). The call returns error -25208, yet it works after about 2 s. Until then the focused element is missing (-25212).
- Electron apps take `AXManualAccessibility` (Claude: true). Chrome answers -25205 (unsupported) to it.
- Writing: Chrome reports success for `AXSelectedText` but changes nothing. What works: select the word plus its ending, then post the fix as unicode key events to Chrome's process (at most 20 UTF-16 units per event). Chrome takes them only while its window is the active one.
- Chrome's value lags a few ms behind each key, so Sokki waits until the value matches before it acts. Rich editors (contenteditable) show a trailing space as a no-break space (U+00A0) until the next character.

**Lab test, 26 Nov 2026 06:43, Chrome window in front** (made-up sentence with three typos; input, textarea and rich editor; 150 and 400 ms pause after each space; 9 runs):

| | Result |
|---|---|
| Fixes exact | 18 of 18 ("dign"→"ding", "wroks"→"works" in every run) |
| Typo typed ~4 s after a cold Chrome start | missed in every run (Chrome's 2 s wake). The model does fix it in that sentence. With an 8 s start Sokki found it. |
| Chrome window not active | the fix doesn't arrive. Sokki leaves the word as typed and selects nothing. |

A run at 01:00 that night measured nothing: a macOS permission prompt held the front, so Chrome never stayed active. Check `lsappinfo front` before a night run.

**CPU and memory, autocorrect off → on** (each app's own processes, 25 Nov 2026):

| App | Off | On |
|---|---|---|
| Slack | 0.3 % CPU, 343 MB | 0.1 % CPU, 380 MB |
| Claude | 18.9 % CPU, 2,562 MB | 42.1 % CPU, 2,794 MB |

Slack: no noticeable change. Claude: inconclusive, because the Claude app also runs all coding sessions, so its load swings on its own. Chrome's screen-reader mode itself costs some CPU and can slow window animations (`KNOWN_ISSUES.md` M1).

## Wrong German capitals in English sentences (26 Nov 2026)

**Task:** Robin typed English and Sokki changed "option" to "Option". Find out why, fix it, and measure the fix on real typing.

**Cause:** the model decides a sentence's language from its last three words, and a tie counts as German. The phone's word table marks most common English words (the, you, have, is, there) as common in German too, since German text quotes English. So a plain English sentence never leaned English. After "an", which is in the German determiner list ("an" = at), an English noun took the German noun capital: "pick an option" became "pick an Option".

**Fix:**
1. "an" right before a vowel is the English article; German puts an article in between ("an der Option"). After it, no German capital.
2. A word the table has as common in both languages but only one dictionary has (`crates/sokki-typos/data`) counts for that language at weight 0.25 (`DICT_LEAN`).

**Measured** (before → after, same typos-only binary; Robin's typing scored against its capital truth file, numbers only):

| Set | What | Result |
|---|---|---|
| Robin's typing, 47,494 words | capitals changed | 108 better (102 wrong capitals gone in non-German sentences, 3 in German, 3 nouns newly capitalised), 16 worse (14 German nouns lost their capital, 2 wrong capitals added) |
| Rule 1 alone | | 11 better, 0 worse |
| Dictionary weight 1.0 instead of 0.25 | | 113 better, 31 worse |
| typing37k, 4,000 English words | wrong changes | 72 → 59; right fixes 524 → 524 |
| 136m, 4,000 English words | wrong changes | 73 → 59; right fixes 543 → 543 |

The Mac model now answers about 125 of Robin's 47,494 words differently from the phone's Python twin (`crates/sokki-typos/tools/parity.py`); the phone keyboard is unchanged.

### Follow-up the same day: "in these cases", "Justt"

**Task:** Robin typed "…is ok in these cases" and Sokki changed "these" to "These"; "Justt" at the start of a sentence was left. Find why, fix, measure.

**Causes:**
- "These" is a German noun (thesis), and the legacy German word list Sokki uses also took in common English words ("and", "is", "these", "meeting"). So "is ok in" had no English evidence, the sentence counted as German, and "these" after "in" took the noun capital.
- "justt" counted as a known word: "just" plus the German verb ending "-t". Known words are never changed.

**Fix:**
1. `english-only.txt`: 3,252 lower-case English words the legacy German list has but the newer `main_de` dictionary doesn't. They count as English for the sentence's language, at the same 0.25 weight. Weights 0.5 and 1 only lost more German nouns (421 and 429 left lower, against 417).
2. A stem ending in "t" never takes "-t", "-te" or "-ten" in German (arbeit-et, not arbeitt), so such a word no longer counts as an inflected German word.

**Measured** against the installed 0.2.15 model, with both fixes of the day (Robin's typing, 47,494 words, plus his 349 labelled typos; public sets 8,000 words):

| What | 0.2.15 | Now |
|---|---|---|
| Wrong capitals in non-German sentences (Robin) | 250 | 132 |
| Wrong capitals in German sentences (Robin) | 109 | 106 |
| German nouns left lower case (Robin) | 399 | 417 |
| Labelled typos fixed right / wrong / missed (Robin) | 290 / 3 / 108 | 289 / 3 / 109 |
| Other words wrongly changed (Robin) | 48 | 48 |
| Public sets: right fixes / wrong changes | 1,067 / 145 | 1,067 / 111 |

The one lost typo is an English compound ("touchpoints" with a doubled letter): compound repair runs only in sentences that don't count as English, and the sentence now counts as English. Running compound repair in English sentences too won it back (+1 typo, +1 public fix) but cost 5 more wrong public changes and 1 more wrong change in Robin's text, so it stays off.

### Wrong edits on edited German text (26 Nov 2026)

**Task:** Robin asked whether the day's fixes hold for people other than him, and for a backup check of what Sokki edits wrongly. Edited text has almost no typos, so nearly every change is wrong.

**Set:** `de-clean-10k`: 10,000 sentences, a third each from the Leipzig corpora German news 2023, German web 2021 and German Wikipedia 2021 (156,983 words). News and web spread over 6,651 websites (one sentence per site before any site gets a second); seed 1. Lives outside the repo: `~/sokki-typing-data/eval-samples/de-clean-10k.jsonl`, with `make_de_clean.py`, which rebuilds it from `~/sokki-typing-data/corpora`.

**Result** (typos model, `fixes` mode):

| | 0.2.15 | 0.2.16 |
|---|---|---|
| Sentences with an edit | 919 | 918 |
| Edits that return the typed word unchanged (heißt → heißt) | 597 | 595 |
| Words changed | 404 | 404 |

- The day's fixes added no wrong edit on German text and removed 2.
- 6 in 10 of the model's answers on this text are the typed word itself (a ß or capital rule answering with the same spelling). Autocorrect selected and retyped such a word. It now leaves it alone (`crates/sokki-autocorrect`, test `a_word_the_model_gives_back_unchanged_is_left_alone`).
- The 404 changed words are 0.26 % of words, about one wrong edit per 390 correctly typed German words. Many are long correct compounds that compound repair changes by one letter ("Gasturbinenantriebe" → "Gastuabinenantriebe"), others are names and Swiss spellings.

**Option, switched off (`COMPOUND_PART_CHECK`):** compound repair leaves a part alone that is itself two known words (gas|turbinen); a 3-letter part counts only when both parts are common words. Wrong word changes 404 → 364 (42 gone, 2 new). On Robin's 349 typos: 1 fewer fixed ("landignpages"), 1 changed differently ("Abstraktionelevel" → "Abstraktionlevel" instead of left; meant "Abstraktionslevel"). A looser version (any 3-letter part) removed 67 but cost 3 typo fixes. Waiting for Robin's choice.

There is no language setting for autocorrect: it handles German and English sentence by sentence. Robin's `spoken_languages` (en, de) is the dictation setting.

## Open questions

- Not yet tested in the installed app by a person (first release with these fixes: 0.2.15).
- The fix path is measured in Chrome only, not in the Claude or Slack app.
- Terminals give no editable text field, so Sokki does nothing there.

## Sources

1. Own lab: `scripts/autocorrect-lab.sh` and `crates/sokki-autocorrect/examples/` (a signed lab app with its own Accessibility grant and no keyboard hook). Branch `claude-r/mac-typos`, commits e7291573 to 0ad72b85.
2. Electron, Accessibility tutorial (`AXManualAccessibility`): https://www.electronjs.org/docs/latest/tutorial/accessibility
3. Chromium, accessibility overview: https://chromium.googlesource.com/chromium/src/+/main/docs/accessibility/overview.md
4. `KNOWN_ISSUES.md` M1 and M5.
