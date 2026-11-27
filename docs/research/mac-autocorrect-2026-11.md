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

**Result** (typos model, `fixes` mode; `~/sokki-typing-data/eval-samples/score_de_clean.py`):

| | 0.2.15 | 0.2.16 |
|---|---|---|
| Sentences with an edit | 919 | 918 |
| Answers that are the typed word unchanged (heißt → heißt) | 486 | 486 |
| Capitals added | 111 | 109 |
| Words changed | 404 | 404 |

- The day's fixes added no wrong edit on German text and removed 2.
- About half of the model's answers on this text (486 of 1,001) are the typed word itself (a ß or capital rule answering with the same spelling). Autocorrect selected and retyped such a word. It now leaves it alone (`crates/sokki-autocorrect`, test `a_word_the_model_gives_back_unchanged_is_left_alone`).
- The 404 changed words are 0.26 % of words, about one wrong edit per 390 correctly typed German words. Many are long correct compounds that compound repair changes by one letter ("Gasturbinenantriebe" → "Gastuabinenantriebe"), others are names and Swiss spellings.

**Cause of the mangled compounds, and fix (same day):** the word table is a Bloom filter. Compound repair tries the hundreds of one-letter variants of a long part and takes one the table calls a common word; among that many tries, a false "common" is likely ("urbinenantriebe" → "uabinenantriebe", "schreiboffensive" → "schreiboffeneive"). A candidate now also needs a frequency above 0, a second, independent lookup in the same table. Wrong word changes 404 → 385 (19 gone, none new); Robin's typos and both public sets unchanged.

**Option, switched off (`COMPOUND_PART_CHECK`):** first measured against 404, now on top of the cross-check: 385 → 355 (1 new). Same cost on Robin's typos as below. Originally: compound repair leaves a part alone that is itself two known words (gas|turbinen); a 3-letter part counts only when both parts are common words. Wrong word changes 404 → 364 (42 gone, 2 new). On Robin's 349 typos: 1 fewer fixed ("landignpages"), 1 changed differently ("Abstraktionelevel" → "Abstraktionlevel" instead of left; meant "Abstraktionslevel"). A looser version (any 3-letter part) removed 67 but cost 3 typo fixes. Waiting for Robin's choice.

### Typos left in real typing (26 Nov 2026)

**Task:** Robin saw many typos not caught. Sort the ones left in his typing journal of the day (`typing_log/journal-2026-11-26.jsonl`, 119 entries, 2,326 words typed key by key, mostly the Claude app) by cause. Counts only; script in the session's scratchpad (`miss.py`).

| Cause | Typos |
|---|---|
| The model has no fix (letters swapped or doubled in short or long words, also where it is one slip away) | 9 |
| The model fixes it, Sokki didn't see the word typed: mid-text | 7 |
| Same, first word of a message or after a line break | 4 |
| Same, last word, sent with Enter (nothing to fix it on) | 1 |

Another 20 words the model left were names, product names, English spellings, slang or code, rightly left.

**Fixes the same day** (`crates/sokki-autocorrect`):
- The first word in an empty field, and a word typed right after a click, were never fixed: Sokki only counted a word from its second letter. Tests `a_typo_as_the_first_word_of_an_empty_field_is_fixed`, `a_word_typed_after_a_click_into_the_text_is_fixed`.
- A Backspace inside a word made Sokki forget it was typed, so a word corrected by hand while typing was never fixed. Now a Backspace shrinks what counts as typed by one character. Test `a_word_corrected_with_backspace_while_typing_is_still_fixed`. Whether this explains the 7 mid-text losses is not shown yet: the journal holds no keys.
- The fix list now also has a line `"what":"missed"` for every word the model would fix that Sokki leaves because it didn't see it typed, with the reason, so the next losses show their cause.
- **First day of 0.2.17** (12:50 to 14:00 local): 34 fixes and 6 missed. 5 of the 6 were "start of the word not seen typed", all mid-sentence after a space. Likely cause: two keys between two looks, one of them a Backspace (fast typing). Sokki then saw neither a plain addition nor a plain deletion and forgot the run. Now any edit at the cursor counts, as long as it adds and deletes no more characters than keys were pressed. Test `a_backspace_and_a_letter_between_two_looks_keep_the_word_typed`.

The 9 the model doesn't fix are a model question shared with the phone keyboard; not changed here.

There is no language setting for autocorrect: it handles German and English sentence by sentence. Robin's `spoken_languages` (en, de) is the dictation setting.

### The model's own misses (26 Nov 2026, evening)

**Task:** find out why the typo model (`crates/sokki-typos`) leaves typos in Robin's Mac typing although the meant word is one slip away and common, and fix what it can without new wrong edits. Measured on Robin's labelled typos and capital truth (`~/sokki-typing-data`), typing37k and 136m (4,000 words each), `de-clean-10k` and the 8 misses from the day's journal (139 entries when measured). Numbers only; examples below are made up.

**Robin's slips on the Mac** (349 labelled typos): two letters swapped 167 (48 %), a letter missing 81, an extra letter 35, a doubled letter 33, a wrong letter 23 (6.6 %), two or more slips 10. For comparison, wrong letters are about 60 % of the typos in typing37k (phones) and about 26 % in 136m (hardware keyboards).

**Causes:**
1. The model took the phone's QWERTZ keys as neighbours. Robin's Mac uses a US/ABC layout (QWERTY): y sits next to t and u, z next to s and x. So "plau" for "play" counted as a far slip and stayed.
2. A word with no candidate one slip away got no candidates at all: two slips in a long word ("fuktionirt") stayed.
3. Short swaps and swaps at a word's end: the right word leads but holds 81–94 % of the vote against a second real word one slip away (both swaps, or a swap against a missing letter). The model needs 95 %. Most of the day's misses are this case.
4. Capitals: at a sentence start the capitalised and the lower-case form of the same swap get the same answer (86 % share for both), so "the capital changes the answer" did not reproduce.

**Fix:**
- The Mac reads which letter each physical key types in the layout in use (macOS UCKeyTranslate, on the main thread, every 5 s while autocorrect is on), and the model takes those rows as neighbours (`Typos::set_keyboard`). The phone reads the keys on its screen instead (`phone-autocorrect-2026-11.md`). Log line: "autocorrect: keyboard rows set (26 letters)".
- British -ise spellings (optimised, organising) count as English words when the US list has the -ize form. With QWERTY neighbours they would otherwise become -ize.
- A word with no candidate one slip away gets the dictionary's common words (zipf ≥ 3) two slips away; the same 95 % rule decides. A letter-count bound keeps it fast: 3 ms per long unknown word (95th percentile), 38 ms without it. A minimum word length made no difference (3 to 8 letters measured the same).

**Measured** (before → after):

| Set | What | Before | After, QWERTY (Robin's Mac) | After, QWERTZ |
|---|---|---|---|---|
| Robin, 349 labelled typos | fixed right / fixed wrong / missed | 289 / 3 / 109 | 290 / 3 / 108 | 289 / 3 / 109 |
| Robin, 47,494 words | other words wrongly changed | 48 | 47 | 47 |
| Robin | German nouns left lower / wrong capitals | 417 / 238 | 418 / 238 | 418 / 238 |
| 136m, 4,000 words | right fixes / wrong changes | 543 / 56 | 558 / 56 | 557 / 56 |
| typing37k, 4,000 words | right fixes / wrong changes | 524 / 55 | 539 / 55 | 529 / 55 |
| de-clean-10k, 156,983 words | words changed | 385 | 386 (6 new, 5 gone) | 387 (2 new) |
| Journal of the day | misses fixed | 0 of 8 | 1 of 8 | 1 of 8 |

Of the 6 new changes on de-clean-10k, 2 fix real typos in the set, 1 is in a Luxembourgish sentence and 3 change names. The 1 more noun left lower case on Robin's text is an -ise word in a German sentence.

**Tried and left out** (each against the same "before"):

| Option | Robin: right / wrong / words changed | 136m right / wrong | typing37k right / wrong | de-clean changes (new / gone) | Journal |
|---|---|---|---|---|---|
| Swaps 2× more likely | 292 / 4 / 52 | 1,069 / 110 (both) | | 388 (3 / 0) | 1 of 8 |
| Wrong letters 4× less likely | 295 / 3 / 49 | 1,030 / 108 (both) | | 370 (18 / 33) | 0 of 8 |
| Hardware-keyboard slip rates (wrong letter ×0.21, missing ×0.64, extra ×0.69: the typing37k → 136m ratios) + the fix | 290 / 3 / 48 | 542 / 55 | 501 / 55 | 348 (19 / 56) | 2 of 8 |
| Same + swaps 2× | 292 / 4 / 49 | 549 / 56 | 498 / 53 | 351 (23 / 57) | 3 of 8 |
| Same + 90 % share instead of 95 % | 303 / 4 / 54 | 577 / 64 | 536 / 57 | 417 (51 / 19) | 4 of 8 |
| Same + 85 % share | 309 / 5 / 75 | 600 / 69 | 555 / 62 | 465 (91 / 11) | 5 of 8 |
| Other-language candidates 10× less likely once the sentence leans one way | 287 / 2 / 53 | 1,069 / 111 (both) | | 386 (6 / 5) | 1 of 8 |
| A built German word (compound, inflection) competes with a common German word one slip away | 297 / 4 / 81 | 1,113 / 112 (both) | | 406 (21 / 0) | 0 of 8 |

The hardware-keyboard rates remove 56 wrong changes on German text but lose as many of Robin's typos as they win (11 won, 12 lost against the fix alone) and many wrong-letter fixes in both public sets. Every option that fixes more of Robin's swaps also changes more correct words.

**Not fixed, and what would:** 7 of the day's 8 misses stay (cause 3, and two long words where keeping the typed word still scores higher than two slips). Two paths, both outside this change:
- Decide a doubtful word once the next word is typed (`NEXT_CONTEXT` in the Python twin, off because the app decides at the space). Autocorrect would have to replace the word before last.
- Give autocorrect the words Sokki learns from the journal. The Python twin's `SPLIT_DECISION` (off since 23 Nov: as many of Robin's names and tool words changed as typos fixed) could then be measured again.

### Words lost to a key during a read (26 Nov 2026, 0.2.18)

Task: find why 0.2.18 still left 5 typos as "start of the word not seen typed" (its fix list from 15:01 to 18:20: 54 fixed, 1 undone, 7 missed, of them 5 start of word and 2 paste or dictation).

- All 5 were in T3 Code (an Electron app). In 4 of them the typing journal holds the whole word as typed key by key, so the keys were there and autocorrect lost them. The fifth came 6 s after an undone fix, with one character inside the word not typed; unclear, left.
- Cause, reproduced in `tests/typing.rs`: a key that lands while Sokki reads the field. Sokki counts the keys, then reads the text, then the cursor, and each read takes a few ms in Electron. A key between the count and the text read gives one character more than keys counted. A key between the text and cursor reads gives a cursor the text doesn't have. Either way the edit doesn't fit and the word's typed run starts again mid-word. The journal doesn't lose these words: it allows one character more than keys and doesn't use the cursor.
- Fix: when the key count changed during the read, the read counts as stale. Sokki keeps the last read and looks again (it already does that when an app hasn't caught up with a key). Both race tests pass, and so do the 15 earlier typing tests.

### Silent misses, the user's own words, missing German words (26 Nov 2026, evening)

Task (from the word-learning session, Robin's words: "typos 1 letter off from my vocab should be autocorrected"): four typos typed key by key and followed by a space were neither fixed nor listed (capitalised German nouns, one wrong letter, swap or missing umlaut each; Claude and T3 Code, under 0.2.17 and 0.2.18). A typo of a Vocabulary word (a company name) wasn't fixed to it, and a mix of a German and an English plural had no German word to go to (the dictionary lacks "Transkript").

- **Silent misses:** typed cleanly in the test field, all four are fixed. In the app a look with no typed run at all (an edit that didn't fit the keys or the cursor) returned before the model ran, so nothing was listed. The likeliest cause is the read race above, fixed in 0.2.19. From now on such a look is listed as missed too ("edit not seen typed"), but only when the text changed since the last look, so a click or an arrow key lists nothing.
- **Own words:** autocorrect now reads Sokki's Vocabulary (Advanced list, meeting glossary, learned words; `all_vocabulary`) every 5 s. They are never changed, an entry of several words counts word by word, and Robin's settings give 26 words. They don't fix typos yet: the model only takes as a fix a word its frequency table knows (`table.candidate`, `CAND_ZIPF`), and the user's words aren't in that table. Making them fix targets is a model change; it has to be measured for wrong changes first (`SOKKI_TYPOS_PERSONAL=<file>` gives the CLI the same words).
- **Missing German words:** `crates/sokki-typos/data/extra-de.txt`, a list by hand next to the generated one: Transkript, Transkripte, Transkripten. They are now known words and won't be changed, but for the same reason as above they aren't fix targets yet. "Transkripts" stays out on purpose, so the model change can fix it to "Transkripte" (Robin: "closer is Transkripte").

### The user's own words as fixes (26 Nov 2026, night)

**Task:** fix a typo one slip from a word of the user's Vocabulary (Robin's example: a company name with two letters swapped) and from a word in `extra-de.txt` ("Transkripts" → "Transkripte"). Pick how common such a word counts by measuring, and measure the wrong changes first. Numbers only; examples below are made up.

**Why they weren't fixed:** a word one slip away competes only at zipf ≥ 2 in the frequency table, and Vocabulary words aren't in it. Giving them zipf 2 (as the Python twin's `PERSONAL_TARGETS` does) isn't enough. Against an unknown 7-letter word a swap reaches 79 % of the vote and needs 95 %, which takes about zipf 2.7.

**Change:** `Lexicon::set_targets`. The Mac's Vocabulary (`set_user_words`: Advanced list, meeting glossary, learned words) and `extra-de.txt` count as at least zipf 4 (`TARGET_ZIPF`) when they are one slip from the typed word. The words kept by undoing a fix stay out, since some of them are typos. The CLI's `SOKKI_TYPOS_PERSONAL` now does the same (`SOKKI_TYPOS_NO_TARGETS` turns it off).

**Measured** on QWERTY with Robin's Vocabulary (26 words): his 401 labelled typos and 48,600 words, typing37k and 136m (4,000 each), `de-clean-10k`, the day's journal (343 entries, typed spans only). "Made-up slips" are every one-slip typo of each Vocabulary word of 4+ letters (first letter kept, wrong letters only on QWERTY neighbours).

| Zipf for own words | Robin: typos right / wrong / missed | Robin's words changed | Journal fixes | de-clean changes | Public right / wrong (136m, typing37k) | Made-up slips fixed to the word / to another word (of 985) |
|---|---|---|---|---|---|---|
| none (before) | 290 / 3 / 108 | 47 | 38 | 386 | 558 / 56, 539 / 55 | 455 / 2 |
| 2.0 | 290 / 3 / 108 | 47 | 38 | 386 | same | 458 / 1 |
| 2.5 | 290 / 3 / 108 | 47 | 38 | 386 | same | 464 / 0 |
| 3.0 | 291 / 3 / 107 | 47 | 40 | 386 | same | 534 / 0 |
| 3.5 | 292 / 3 / 106 | 47 | 40 | 386 | same | 653 / 0 |
| **4.0 (chosen)** | 292 / 3 / 106 | 47 | 40 | 386 | same | 689 / 0 |
| 5.0 | 292 / 3 / 106 | **48** | 40 | 386 | same | 730 / 0 |

The 2 new journal fixes are both Robin's real typo of the company name (once capitalised, once not). Zipf 5 is the first to change one of his words. The final version also lifts a Vocabulary word the table knows below zipf 4 (708 of 985 made-up slips, otherwise the same as 4.0). None of the 26 words is ever changed.

**Larger vocabularies** (the same sets, the user's list replaced by 500 rare capitalised dictionary words of zipf 0):

| List | de-clean changes | Robin, public | Made-up slips fixed to the word / to another word |
|---|---|---|---|
| 500 words of 5–8 letters (name-like) | 386 → 387 (1 new: a word one slip from a listed name) | unchanged | 167 → 20,278 / 602 → 93 (of 24,529) |
| 500 words of any length (mostly compounds) | 386 → 386 | unchanged | 21,458 → 47,695 / 1,962 → 19 (of 49,387) |

Speed: a slip of a Vocabulary word now finds its fix one slip away, so the two-slip search is skipped. On a debug build that's 1.2 ms per word instead of 9.8 ms (median, 4,000 made-up slips).

**"Transkripts" stays as typed.** The frequency table already has Transkript (zipf 2.75) and Transkripte (2.25), so they were fix targets before. "Transkripts" is the genitive of Transkript ("des Transkripts"), so the model counts it as a known German word and never changes it. Letting it compete after "die" wasn't enough either: the English "transcripts" is one slip away too, and "Transkripte" got 54 % of the vote. Only a rule for "-s after die" plus one against English in German sentences would fix it, and the second was measured as a wash in "The model's own misses". Left out (KNOWN_ISSUES M6). `extra-de.txt` still pays off where the table lacks a word: a swap in "Transkripten" (zipf 1.25) is now fixed.

**Phone:** no change needed now. The phone keyboard (`Typos.kt`) doesn't get Sokki's Vocabulary. Its personal list holds only words kept by undoing a fix, and those stay out of the fixes here too. Its twin's `PERSONAL_TARGETS` (learned history words at zipf 2.0, off since 23 Nov) is the same idea at a zipf too low to fix a swap. If the phone ever gets the Vocabulary, the same `TARGET_ZIPF` would apply, measured again on the phone's QWERTZ.

### Typed words still lost in the Claude app (26 Nov 2026, late evening, 0.2.19/0.2.21)

Task: the count from 20:30 to 23:00. Result: 17 fixed, 1 undone, 2 skipped, 4 missed, all 4 in the Claude app, and all 4 typed key by key according to the journal (T3 Code wasn't used, so the read-race fix is unconfirmed there).
- One came 4 s after a fix that didn't arrive: Claude took 911 ms, a key landed on the word, Sokki reset. Its miss line is that same word, left as typed.
- The other three: cause not known from the counts. One possible cause is fixed in the code and covered by a test: whitespace at the very end of the field that comes or goes without a key (rich editors show an empty last line) made the edit not fit, so the word's run was lost. That the Claude app does this is not proven.
- To find the rest: a debug log line, numbers only (lengths, cursor before and after, keys pressed, same start and end), whenever a typed run of more than one character breaks although keys were pressed.

### The first day on 0.2.26 (27 Nov 2026)

Task: count fixes and misses in Robin's own typing from 0.2.26's install (26 Nov, 23:41) to 27 Nov, 15:00, and sum up the debug lines on lost typed runs. Numbers only.

- Fix list: 100 fixed (T3 Code 83, Slack 11, WhatsApp 5, Chrome 1), 5 undone, 65 missed (T3 Code 57, Slack 8). Of the missed, 47 were "edit not seen typed", 14 "start of the word not seen typed" and 5 "word not seen typed".
- Vocabulary words: 6 typos fixed to one of Robin's 26 own words, 5 more listed as missed (lost runs, below). No own word was changed.
- Night run in Chrome (01:00, 12 runs, slow and fast typing, three kinds of field): 36 of 36 typos fixed, none skipped.
- Lost typed runs (324 debug lines in the logs of 25 to 27 Nov, by shape):
  - 152: two characters at the cursor for one key. The app showed a key only with the next one: the read before counted a key whose character wasn't there yet. Then the next read had one character more than keys pressed, and the run broke.
  - 80: field cleared or a new message (right to start again).
  - 40: a word or more deleted with one key (1 to 66 characters), or one key replacing the character before it (a dead-key accent). Deleting was counted as if it added characters.
  - Pastes (up to 21,681 characters with one key) and about 20 rare shapes (none over 7): left as they are.
- Fix: keys a read counted but the field didn't show yet carry over to the next read, at most 2, so a key that typed nothing can't let a paste pass as typed. An edit now counts only the characters it added. The first letter in an empty field starts a run even when the old text began with the same letter. 22 typing tests pass, 4 of them new; the lag test fails without the fix.

### The late-key fix didn't explain it (27 Nov 2026, afternoon, 0.2.36/0.2.37)

Task: Robin asked for results after 0.2.36 shipped the fix above. From 0.2.36 (14:28 UTC) to 15:15 UTC: 13 fixed, 1 undone, 8 missed. The running app was the new build (started 15:56 local), and 9 more typed runs were lost as two characters for one key with no key owed. So the app didn't show a key late: the key was never counted.
- The journal (`typing_log/`, the same key count) shows it in every app: about 1 in 40 typed characters falls outside a typed range as a single untyped character (T3 Code 30 per 1000 typed, Slack 25, Claude 20, WhatsApp 17). These characters are ordinary letters (825) and spaces (289), not punctuation or pastes. The journal allows one character more than keys, which is why it keeps these words.
- Fix: autocorrect allows the same: up to one character more than keys pressed, only when a key was pressed (a change with no key stays "not typed"). A 3-character paste with one key is still no typing. 25 typing tests pass; the new one fails without the change.
- Open: why the hook misses about 1 key in 40 (KNOWN_ISSUES M7).

## Open questions

- Not yet tested in the installed app by a person (first release with these fixes: 0.2.15).
- The fix path is measured in Chrome only, not in the Claude or Slack app.
- Terminals give no editable text field, so Sokki does nothing there.

## Sources

1. Own lab: `scripts/autocorrect-lab.sh` and `crates/sokki-autocorrect/examples/` (a signed lab app with its own Accessibility grant and no keyboard hook). Branch `claude-r/mac-typos`, commits e7291573 to 0ad72b85.
2. Electron, Accessibility tutorial (`AXManualAccessibility`): https://www.electronjs.org/docs/latest/tutorial/accessibility
3. Chromium, accessibility overview: https://chromium.googlesource.com/chromium/src/+/main/docs/accessibility/overview.md
4. `KNOWN_ISSUES.md` M1 and M5.
