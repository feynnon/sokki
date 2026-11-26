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
- "Keep what I write" is on by default for everyone; the data stays on the Mac (Robin, 26 Nov). Word learning is off by default until the words it would learn have been checked on Robin's real writing.

## Open

- Check of learned words on real writing: after about a week of Robin's journal, run the learner without adding anything, Robin rates every word; add the precision here.
- Parakeet word list through CTC word spotting (proposed follow-up).
