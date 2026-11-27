# Adding an underlined word to the dictionary (Mac, Nov 2026)

**Date:** 26 Nov 2026.

**Task:** Robin sees words underlined while typing on the Mac and wants to add them to "the dict" right away. Find out what underlines them, what "the dict" should be, and how Sokki can add a word so it is never underlined and never "corrected" again. UX first (four versions), then build.

## Findings

- **What underlines:** macOS's own spell checker (NSSpellChecker), not Sokki. Chrome and Electron apps (Slack, Claude) on the Mac ask the same system spell checker, so the red line looks the same everywhere. Sokki's autocorrect (`crates/sokki-autocorrect`) never draws anything; it only replaces a word after the space.
- **Sokki can teach macOS a word:** `NSSpellChecker.learnWord` from Sokki's process. Tested with a made-up word from a small command-line tool: before, the checker called it unknown; after `learnWord`, a *second, separate* process called it known (the word lives in the system's spelling service, shared by every app); `unlearnWord` took it out again. Robin's `~/Library/Spelling/LocalDictionary` is empty: he has never used "Learn Spelling".
- **Not yet checked:** whether a word already underlined in an open Chrome field loses its line at once or only once the text is edited again (needs a front window; night run).
- **Sokki has three word lists today, not synced:**
  1. Wörterbuch (`meetings.glossary`, plus `custom_words`) and learned words (`vocabulary_learned`), for dictation and summaries.
  2. `autocorrect_kept_words.txt`: words kept by undoing a fix with Backspace. Only autocorrect reads it.
  3. macOS's learned words. Sokki writes none.
  So a word in the Wörterbuch is still underlined by macOS, and autocorrect doesn't know it.
- **"The dict" should be one list:** the Wörterbuch. Adding a word there also teaches it to macOS (no underline in any app) and to autocorrect (never changed). Removing it removes it from all three.
- **Word learning** (`learn_words.rs`) takes candidates only among words the spell checker *doesn't* know, and skips everything already in the Wörterbuch or rejected. A word taught to macOS is therefore never a learning candidate again, which is right.

## Caveats from the neighbouring sessions (26 Nov)

- Autocorrect: `Lexicon::set_personal` replaces the whole set, so pass kept + dictionary words together on every reload; keep the Backspace skip list to kept words only. Personal words are also *fix targets* (a typo one letter from a name can become the name): measure on the eval sets in `mac-autocorrect-2026-11.md` before shipping.
- Word learning: learned words aren't proven precise yet (4 of 8 "add" verdicts wrong in the first real check), so only words the user adds go to macOS at once; learned words only after review or behind a switch. Removing a word must call `unlearnWord`. Test mode (`SOKKI_TEST_MIC`, `org.sokki.test`) must never call `learnWord`: it would write made-up test names into the real Mac dictionary.

## UX: four versions

Drawn on the "Sokki app" canvas, page "Offen" (boards `Wort-1-Kuerzel` … `Wort-4-Von-selbst`): 1 a shortcut on the word at the cursor, 2 a small pill after an underlined word Sokki saw typed (and after an undone fix), 3 underlined words of the focused field in Sokki's menu bar menu, 4 learned automatically after the second identical use. Robin's pick (27 Nov 2026): **2**, "bei mir aktiv, bei allen anderen noch aus".

## Built (27 Nov 2026)

- Switch `autocorrect_add_words` (Settings › Typing, below autocorrect; off by default, on for Robin).
- Autocorrect reports every word typed key by key that it leaves (`Outcome::Typed`). If the Mac's spell checker doesn't know it and it isn't in the Wörterbuch, a pill shows where the dictation pill sits for 4 s: "Quorbit ins Wörterbuch?" with ⌃⌥W, "Hinzufügen" and ✕. Each word only once per run; ✕ never again (`dictionary_declined_words.txt`). Never over a dictation.
- ⌃⌥W is caught by Sokki's keyboard hook only while the pill shows (`sokki_keys::catch_key`); the rest of the time it is the app's.
- A fix undone with Backspace puts the typed word in the Wörterbuch right away, with "Rückgängig" on the pill.
- One dictionary: the Wörterbuch (`meetings.glossary`). With the switch on, Sokki teaches its words to the Mac's spell checker within 2 s of any change (the page, the pill, `sokki config`) and takes back what is removed; switched off, it takes back everything it taught (`spelling_taught_words.txt`; words the user taught macOS by hand are never touched). Learned words (`vocabulary_learned`) are not taught.
- Tests: engine `a_word_typed_key_by_key_with_no_fix_is_reported_for_the_dictionary_offer`; hook `a_caught_key_needs_its_exact_modifiers_and_ends`; `dictionary.rs` unit tests (3); Playwright `tests/word-pill.spec.ts` (5); end to end in a test app `scripts/test-word-pill.sh` (6/6 on 27 Nov: three words taught, one taken back on removal, all taken back with the switch off, the real Mac's dictionary untouched, no word in the log).
- Not tested without a person yet: the pill after real typing (autocorrect is off in test mode; needs a keyboard), and whether Chrome drops an existing underline at once.

## Sources

1. Apple, NSSpellChecker (`learnWord(_:)`, `unlearnWord(_:)`, `hasLearnedWord(_:)`): https://developer.apple.com/documentation/appkit/nsspellchecker
2. Chromium uses the platform spell checker on macOS: https://chromium.googlesource.com/chromium/src/+/main/components/spellcheck/browser/spellcheck_platform_mac.mm
3. Own test, 26 Nov 2026: a Swift tool calling `learnWord` / `unlearnWord` on a made-up word, checked from a second process.
4. Code: `crates/sokki-autocorrect/src/lib.rs` (kept words), `src-tauri/src/learn_words.rs`, `crates/sokki-journal/src/mac.rs` (`spell_known`).
