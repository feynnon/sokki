# Word list and word learning (Nov 2026)

Agent task (session typed-words-dictionary, 24–26 Nov 2026): find out what Sokki's word list does today, which words should go into it, how other dictation apps learn words, then build a local record of typed text ("Keep what I write") and word learning from it. Numbers only; the test words are made up or brand names.

## What the word list did (24 Nov)

- Parakeet (the default speech model) takes no word list (transcribe-cpp has no vocabulary feature for it). Only the fuzzy swap after transcription (`apply_custom_words`: Levenshtein + Soundex, up to 3-word n-grams, threshold 0.18) uses `custom_words`. Whisper, Qwen3-ASR and Granite get the list as a prompt instead.
- The meeting glossary went only into summary prompts, never into dictation.

## Measurements

| Test | Result |
|---|---|
| Parakeet on 14 TTS sentences (`say` Anna/Samantha) with 30 made-up or brand terms | right 8, wrong 22 |
| Fuzzy swap with all 31 words, same sentences | 13 fixed, 2 broken |
| Same swap on 36 of Robin's real dictations (2,479 words) | 35 wrong swaps (28 of one short word) |
| Swap only for words of 6+ letters, TTS sentences | 12 fixed, 1 broken (an n-gram swallowed a correct neighbour; fixed in `text.rs`) |
| Same, real dictations | 2 wrong swaps |
| Robin's meeting glossary as swap list, real dictations | 2 right, 4 wrong (short first names over common words) |
| Qwen 3.5 9B (built-in llama-server, ctx 4096) judging 26 typed words add/no, two runs | 24/26, 26/26; about 1 s per word; 5.7 GB |
| Word learning end to end in Sokki Test (`scripts/test-learn.sh`, made-up journal) | 6/6 checks; 4 candidates, 4 learned, 6 heard-as pairs, 12.7–17.4 s |
| Journal end to end (`scripts/test-journal.sh`, helper app) | 9/9 checks: typed vs pasted vs dictated told apart, passwords never kept, hung app doesn't split a message |

## First check on real writing (26 Nov)

Word learning on a copy of one day of Robin's Mac journal (131 messages, all with typed marks), built-in Gemma 4 E4B as judge, nothing added (`scripts/learn-review.sh`; the words stay in the vault, `reports/Sokki eval data/word-learning/`).

| | Words |
|---|---|
| Unknown typed words (capitalized, 3+ letters) | 15 |
| Typed in 2+ messages (learned now) | 0 |
| Model says add | 8: 4 right (names, a product), 4 wrong (colloquial forms, a misspelled plural) |
| Model says no | 7, all right (typos, chat abbreviations) |
| Hear test gave a heard-as form | 5 of 8 |

Three of the four wrong "add" words only ever stood at a sentence start, where German capitalizes every word; none of the four right ones did. Rule since then: a word counts only if it was capitalized inside a sentence at least once (`learn.rs`, `starts_sentence`). With it, 1 wrong of 5 "add" words remains. Robin's own rating of each word is pending.

## Robin's rating, second check (26 Nov, evening)

Task: run word learning in report mode (`scripts/learn-review.sh`, Gemma 4 E4B, with the sentence-start rule) over Robin's journal (326 typed entries, 24–26 Nov). Robin rated every verdict on a review page.

- 17 unknown words. The model said "add" to 14: 10 right, 4 wrong (71 %). It said "no" to 3: all 3 right.
- All 4 wrong "add" words were typos of a known word: one of a word already in Robin's glossary (1 letter apart), three of a German word macOS knows (one letter wrong, a missing umlaut, a wrong plural ending).
- A check against macOS spelling guesses (`NSSpellChecker.guesses`, de and en, Damerau distance): "1 edit from a word macOS knows" catches those three, but also 3 right words (two product names and a surname, each 1 edit from an ordinary word). So that test alone drops good names. "At most 1 edit from a word already in the word list, and not that word" catches the glossary typo and nothing else.

## How others do it (research agent, 24 Nov)

- Wispr Flow learns from edits in the field after a paste (vocabulary only; a deleted word never comes back), reads context through Accessibility, skips password fields, banking and URL bars.
- VoiceInk (open source) watches the field 60 s after a paste and turns edits into replacement pairs, filtered locally (at most 3 words, no function words or inflections); dismissed means never again; skips AXSecureTextField and AXProtectedContent.
- Granola learns spelling fixes right after a dictation, with an Undo pill; at most 300 terms.
- Dragon learns from documents and sent mail with a review list. Apple adds contact names, which can overwrite common words.
- Nobody else tests a word by speaking it to the speech model (Sokki's "hear test").
- Parakeet can use a word list through a CTC keyword spotter next to TDT (FluidAudio: 85.2 % of listed words found, 99.3 % precision on Earnings22, up to about 230 terms) or NeMo word boosting. Not in transcribe-cpp yet.

Sources: docs.wisprflow.ai/articles/4052411709, docs.wisprflow.ai/articles/4678293671, github.com/Beingpax/VoiceInk, docs.granola.ai/help-center/voice-typing/spelling-names-and-terms, docs.fluidinference.com/asr/custom-vocabulary.md, docs.nvidia.com/nemo/speech/latest/asr/asr_customization/word_boosting.html, arXiv 2508.07014.

## Decisions

- Learn only from words typed key by key; dictated, pasted and autocompleted text never counts (Robin, 25 Nov: learned words from dictation were garbage).
- The swap uses learned and glossary words only at 6+ letters; `custom_words` as before; heard-as pairs are fixed exactly.
- "Keep what I write" and word learning are on by default for everyone; the data stays on the Mac (Robin, 26 Nov).
- A word must be capitalized inside a sentence at least once (first real-writing check, above).
- A word one slip (one letter wrong, missing, extra, or two swapped) from a word already in the user's list is a typo of it, not a new word; words of 5+ letters only (Robin, 26 Nov: "als Sicherheitsnetz", since autocorrect is off by default). On the second check it removes the glossary typo and none of the 10 right words: the model's "add" goes from 10 of 14 to 10 of 13 right.
- Typos of ordinary German words (a wrong letter, a missing umlaut) are autocorrect's job: its model fixes them, but on 26 Nov it left them unfixed and unlogged; handed to the autocorrect session, with the user's word list as autocorrect words and a German word missing from its list.

## Open

- A third check after a week of writing, when more words are typed in 2+ messages.
- Parakeet word list through CTC word spotting (proposed follow-up).
