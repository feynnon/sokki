# Autocorrect: a key typed during a fix lands in the wrong place (Mac, Nov 2026)

**Date:** 27 Nov 2026.

**Task:** Robin: "Sometimes too slow. I type, it corrects, but I've already typed the next letter, and then it goes wrong: that letter appears at the end of the word I typed." Reproduce it with a test, design a fix that never loses or misplaces a key and still feels instant, build and measure it.

## Cause

Sokki swaps a word by selecting it through Accessibility and typing the fix as key events (`crates/sokki-autocorrect`). Before the swap it checks that no key was pressed since it read the field. That check misses a key that was pressed *before* the read but hadn't reached the field yet: Chrome and Electron apps show a key to Accessibility a few ms late, and Sokki's count of "keys not shown yet" can't see a lag that stays constant (each read shows one new key and one new character, always one behind). Sokki then selects "word + space" and types "fix + space" while the next letter is already after the space: the cursor ends up *before* that letter, and every following key lands in front of it ("nicht ut … g" for "nicht gut").

A second, smaller hole: a key pressed between Sokki's last check and the app handling the selection types over the selected word. The repair for that needs Accessibility to show the key in time, which Chrome doesn't always do.

## Options weighed

| Option | Verdict |
|---|---|
| Hold the user's keys in the keyboard hook during the swap, send them on after | **Chosen.** The only one that orders the user's keys after the fix for sure. Sokki's hook is already an active event tap (it blocks shortcut keys), so it can hold keys for a few ms. |
| One atomic Accessibility replace (set value or selected text) | Chrome answers success to `AXSelectedText` and changes nothing (`mac-autocorrect-2026-11.md`); setting the whole value breaks rich editors and the app's undo. Not possible in the apps that need it. |
| Skip a fix when the word is no longer the last thing typed | Doesn't help: Sokki can't see that a key is on its way, which is the cause. |
| Log the latency | Done as well: the list of fixes has `ms` per fix; the hook logs how long keys were held (debug) and warns when a hold ran out. |

## The fix

1. While a word is swapped, the hook (`crates/sokki-keys/src/hold.rs`) holds every key press and release (not ⌘ or ⌃ shortcuts) and afterwards sends them to the app in order (`CGEventPostToPid`, the same way the fix is typed, so they come after it). Held keys are counted as typed when they come in.
2. A hold ends by itself after 300 ms; the next key then sends the held ones first. A ⌘/⌃ shortcut ends it the same way. The keyboard can't get stuck.
3. With keys held, Sokki waits until the last key that went to the app must show (40 ms after it, `KEY_SHOWS_MS`; no wait when the user paused), reads the field again, and goes on only if all that changed is characters typed at the cursor. It then selects the word and everything typed after it and types the fix and that text again, so the cursor ends where it was.
4. The same for undoing a fix with Backspace.

## Measured

**Simulation** (`crates/sokki-autocorrect/tests/fast_typing.rs`, runs headless in `cargo test`): a simulated Chrome on a simulated clock. Keys reach the app 0.5 ms after the hook counts them, the app handles keys, Sokki's typed text and Sokki's selection in arrival order, Accessibility shows the field 0 to 30 ms late, each read takes 2 ms. Made-up sentence with two typos ("Das ist nihct gut und wroks nicht so "), 8, 12, 15 and 20 keys a second with random gaps, 40 runs each: 800 runs.

| Accessibility behind | Wrong runs before | after | Typos fixed before | after | Keys held at most |
|---|---|---|---|---|---|
| 0 ms | 0 of 160 | 0 | 320 of 320 | 320 | 52 ms |
| 5 ms | 0 | 0 | 194 | 194 | 56 ms |
| 10 ms | 5 | 0 | 207 | 224 | 60 ms |
| 20 ms | 160 | 0 | 279 | 307 | 92 ms |
| 30 ms | 160 | 0 | 259 | 299 | 110 ms |
| **All** | **325 of 800** | **0** | **1,259 of 1,600** | **1,344** | |

A wrong run here is the bug Robin saw: the letter after the fixed word ends up at the end ("Das ist nicht ut und works nicht so g").

Keys are held only while a word is swapped: about 40–60 ms in an app that keeps up, up to about 110 ms when Accessibility is 30 ms behind. Typing at 15 keys a second is one key every 67 ms, so at most one or two keys come a few tens of ms late.

**Real Chrome, night run:** `scripts/night/test-autocorrect-race.sh` (03:00, Mac idle): the lab app runs Sokki's hook and types the same sentence as real key events at 12 and 15 keys a second, 10 runs each, with and without holding. Result: pending (first night run).

**Found on the way, not changed:** at 5–10 ms lag about 4 in 10 typos are left unfixed in both versions ("start of the word not seen typed": the same constant lag makes Sokki think a word wasn't typed key by key). It never puts a wrong letter in the text, but it is why Sokki sometimes doesn't fix at all. Handed to the autocorrect session.

## Sources

1. Code: `crates/sokki-autocorrect/src/lib.rs` (`replace_holding`, `calm_field`), `crates/sokki-keys/src/hold.rs`, `crates/sokki-keys/src/platform/macos/listener.rs`.
2. Chrome's Accessibility lag and `AXSelectedText`: `docs/research/mac-autocorrect-2026-11.md`.
3. Apple, `CGEventPostToPid`, `CGEventTapCreate` (active taps may drop events): https://developer.apple.com/documentation/coregraphics/quartz-event-services
