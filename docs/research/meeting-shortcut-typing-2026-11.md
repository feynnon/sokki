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

## Second report: "meetings are detected when I just type" (2026-11-27 afternoon)

**Task:** find which path detects or starts a meeting during long typing
with no call, fix it for every user, test it with made-up data.

**Installed:** 0.2.34 (with the Option+M fix).

**Cause: call detection, not the voice detector and not the shortcut.**
- `sokki.log` (UTC): the prompt "record this call?" follows each
  `meeting event: Started { app: … "Google Chrome" …, call: None }`.
  On 27 Nov: 11 Chrome sessions, 9 of 20–63 s (08:37–08:57, 09:41, 12:50,
  12:51, 12:58), 2 real calls the user recorded (10:00, 13:00).
- In each short session the typing journal (metadata only: which app, how
  many typed characters) shows the user typing in Slack, WhatsApp or T3 Code,
  none in Chrome.
- 6 of the 9 came from Chrome helper processes of version 158, while the
  user's own Chrome runs version 157 since 22 Nov: newly started Chrome
  instances, most likely test browsers of agent sessions. The other 3 are
  the user's Chrome (pid of the audio helper "Aperitif"); which tab held the
  mic is not in the log.
- The detector counted a browser as a call as soon as its audio helper
  held the mic (`IsRunningInput`). Nothing about a call was checked.
- Auto start did not start anything while the user typed: every typing
  window was refused by the own-voice check (best similarity 0.02–0.50,
  bar: 2 pieces at 0.5). No Option+M press while typing after 0.2.31.

**Measured:** during a real Chrome call (27 Nov, 14:0x local) Chrome's
audio helper held the mic and played audio at every poll (5 of 5, `cargo
run -p meeting-detect --example in_out`).

**Fix:** a browser that holds the mic counts as a call only once it also
plays audio, or once its tab is a known call service. Native call apps are
unchanged (they hold the mic only in a call). Logged as "holds the mic but
plays nothing; not a call yet". Tests: `crates/meeting-detect/src/watcher.rs`
(`browser_holding_the_mic_while_nothing_plays_is_no_call`,
`browser_call_starts_once_the_other_side_plays`, named and native calls
unchanged, no busy deadline).

**Keyboard sound and the voice detector (Earshot, threshold 0.5):** made-up
typing (`scripts/make-typing-wav.py`) at 5–12 keys/s, levels 0.05–0.8: at
most 3.5 s of "speech" in a 30 s window, at the loudest setting; 0 s for the
others. The bar is 12 s. Unit test `typing_is_not_speech` (meeting/auto.rs,
must stay under 6 s); headless test `scripts/test-typing-no-meeting.sh`
(results below).

**Headless result (2026-11-27 15:17 local, Sokki Test from commit acfaf697,
`scripts/test-typing-no-meeting.sh`, set `typing-noise-made-up`):**

| Clip | Expected | Result |
|---|---|---|
| `typing-steady` (90 s, 8 keys/s, 0.3) | no meeting | no meeting; never reached 12 s of speech (no voice check ran) |
| `typing-fast-loud` (90 s, 12 keys/s, 0.8) | no meeting | no meeting; never reached 12 s of speech |
| control `known-talk` (known voice) | listens in or starts | listened in after 13 s: 3 of 3 pieces the user's, similarity 0.92; dropped after 38 s of the clip (no speech for 30 s), as designed |

Not covered headless: real keyboards on real microphones (the made-up clicks
stand in), and the browser rule in the running app (unit-tested; the Core
Audio part needs a real browser holding the mic).

**Installed app, 0.2.36 (2026-11-27 14:47 UTC):** a headless test Chrome
(own profile, `open -g -n`, a local page holding the mic with
getUserMedia, nothing playing) held the mic for about 30 s. Core Audio: Chrome
in the mic list at every poll, nothing in the playing list. sokki.log:
"Google Chrome holds the mic but plays nothing; not a call yet" 7 s in, no
`meeting event: Started`, no prompt. Still to see: the next real browser call
prompting as before.
