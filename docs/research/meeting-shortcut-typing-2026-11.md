# Meetings started by typing: the Option+M shortcut (2026-11-27)

**Task (2026-11-27):** "A meeting started by itself, the room was silent."
Find what started it and make sure a meeting never starts without a call or
speech.

## Result

Not auto start, not a call, the CLI, a hook, a schedule or another agent. Every
one of the starts came from the meeting shortcut, Option+M, pressed while the
user typed.

- `sokki.log` (times in UTC): both starts on 2026-11-27 (05:31:06 and
  08:30:46) say `native keys event: binding=meeting, hotkey=option+m,
  state=Pressed`, then `Meeting start via the meeting shortcut`. Pressed and
  released within the same second, no dictation key, no detected call
  (`meeting_detect` logged nothing then).
- The Mac's keyboard layout is "U.S." On it, umlauts are typed with the dead
  key Option+U, then the vowel.
- Sokki's typing journal (local, `typing_log/`): at each start the user was
  typing, in a text field, a word with "m" right before an umlaut ("mö",
  "mü"). The same holds for both Option+M presses on 2026-11-26 (08:31:30,
  13:34:38). 4 of 4 Option+M presses in the logs.
- Typing "mö" fast: Option goes down for the umlaut before M is down, so M
  arrives as Option+M, and U follows while Option is still held.
- One of the four became a 41 s meeting with a note (a few words on the mic),
  one was dropped as "no speech", one was the 13:34 test (dropped), one ran
  on as a real meeting.

## Fix

`sokki_keys::HotkeyManager::register_on_clean_release`: the meeting shortcut
fires when its modifiers are released, and only if no other key went down
in between. A deliberate Option+M is Option, M, release; typing "mö" always
has U in the chord, which marks it as typing (`Ignored`, logged as "not used:
another key went down"). Dictation keys keep firing on press (hold to talk).
Cost: a deliberate Option+M starts when Option is released, not when M goes
down (the time Option stays held, typically a tenth of a second).

Tests: `crates/sokki-keys/src/manager.rs`
(`typing_an_umlaut_after_m_does_not_fire_option_m`, the real key order;
`plain_registration_fires_on_the_typed_umlaut`, the old behaviour) and
`src-tauri/src/shortcut/native_keys.rs` (meeting is the only binding on clean
release).

## Sources

- `~/Library/Logs/org.sokki.app/sokki_2026-11-27_09-09-23.log`,
  `sokki_2026-11-26_13-35-04.log` (local).
- `~/Library/Application Support/org.sokki.app/typing_log/journal-2026-11-2{6,7}.jsonl`
  (local, private; only the letter pairs were checked).
- `defaults read com.apple.HIToolbox AppleSelectedInputSources`: U.S.

## Night test (real key events, installed app)

`scripts/night/test-meeting-shortcut.sh`, only when nobody uses the Mac and
no meeting records or processes. It posts real key events (with Option's
FlagsChanged, as a keyboard does; `osascript ... using option down` sends
none, so it can't stand in) into a blank TextEdit document and checks
`sokki.log`:

1. "mö" typed fast (Option, M, U, Option up, O): no "Meeting start via the
   meeting shortcut", and one "Shortcut meeting (option+m) not used".
2. Deliberate Option+M: a meeting starts; a second Option+M stops it. The
   silent test meeting goes to the Trash by itself (no speech).

It prints PASS, or the first step that failed. The process that runs it needs
Accessibility, or its events don't reach Sokki.
Result: pending (first run in the night of 2026-11-27).
