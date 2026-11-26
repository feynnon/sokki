# Sokki for Android

Record a meeting on the phone; Sokki on the Mac turns it into a note
(speakers, summary, tasks, transcript) and sends it back. Dictate on the
phone itself (Dictate tab, offline).

**Link to the Mac** (`crates/sokki-link`, iroh): direct and end-to-end
encrypted, from any network. The phone dials the Mac by its public key;
iroh finds it through n0's DNS, connects directly (hole punching) or
forwards the encrypted bytes through one of n0's free public relays.
Nothing is stored in between.

- Pairing: on the Mac, Meetings → settings → Phone → switch on; scan the QR
  code with the phone (or paste it). The Mac then remembers the phone's key.
- Deliberately manual: after recording, the phone tries once. If the Mac is
  off, the recording stays on the phone; **Check again** sends it and
  fetches finished notes. No retries in the background.
- "Email me the setup" (Mac Phone settings, phone Connect screen) opens the
  person's own mail app with `shared/invite.ts` filled in.
- n0's public relays are meant for development; for many users, run our own
  relay (open source) or pay n0. A paid mailbox for "send while the Mac is
  off" is an idea for a waitlist, not built.

## Quick Settings tile ("Record meeting" / "Meeting aufnehmen")

The first time the Meeting tab shows, Sokki offers it with Android's own
"Add tile" dialog (Android 13+, once per install). By hand: swipe down
twice, tap the pencil, find **Record meeting** (Sokki) among all tiles and
drag it into the panel. Android 17 (Pixel 7, 2026-11-24) shows a newly
added tile as a small square with only the icon; in edit mode, tap it and
drag its edge to make it wide with its text. No app can add the tile itself
or choose its size. Tapping it opens Sokki already recording (the same
recording as the red button); tapping it again stops the recording without opening anything,
and it goes to the Mac as usual. While Sokki records, the tile is on and
says "Recording". `RecordTile.kt`; the page reports its state through
`window.SokkiRecorder`.

It has to open Sokki: Android lets a tile start a foreground service, but
not one that uses the microphone (SecurityException, tested on Android 15).
Sokki then records like after a tap on its button: on screen, screen kept on.

**Phone calls** (tested on the emulator, Android 15, 2026-11-23): while a
call is active (audio mode `IN_CALL`), Android gives Sokki silence. A
recording started during a call runs and shows its clock, but Android marks
it `silenced` the whole time; a recording that is running when a call comes
in keeps going and is silent exactly while the call is active (ringing is
not silenced). Neither your own voice nor the other side gets in. Android
does the same for internet calls (`IN_COMMUNICATION`, e.g. WhatsApp); the
call audio itself is only open to pre-installed system apps. While Android
silences the recording, a line under the clock says so
(`SokkiRecorder.silenced()`). Check on a device:
`adb shell dumpsys audio | grep silenced`.

**Calls can't be recorded on the phone** (Robin, 2026-11-24): Android
gives no app the call audio, and Sokki doesn't try to get around that.
These ways around it were looked at and dropped:
- rooting the phone (BCR, phone calls only);
- the Mac posing as the phone's Bluetooth headset (you would talk to the
  Mac, not the phone);
- importing the Pixel's own call recordings.

An iPhone call taken on the Mac is different: Sokki on the Mac asks to
record it (Robin, 2026-11-25). See MEETINGS.md, "Other people's voices".

## Keyboard (voice typing in every app)

`keyboard/` is HeliBoard 4.2 (see `keyboard/UPSTREAM`) built into the app
as a library: one install has the app and the keyboard. Changes to
HeliBoard are marked "Sokki" in the code:

- The mic is pinned at the top right of the suggestion strip. Tapping it
  starts `dev.verbatim.sokki.SokkiVoice`: microphone → Rust
  (`src-tauri/src/keyboard_voice.rs`) → the streaming model (Nemotron
  Streaming 3.5, as on the Mac with a streaming model) → words typed into
  the open field about every half second, the word still being heard
  underlined. Tap again to stop; the mic is red while listening. Numbers:
  `asr-bench/README.md`, "Streaming".
- Through the Mac first: when the phone is paired with a Mac running Sokki
  (phone link on), the keyboard opens the link when it opens and dictates
  through it. The Mac reads the audio with its own model and settings
  (Parakeet on the GPU, `meeting/phone_link.rs` `dictate_for_phone`): live
  text every 0.5 s, underlined because it may still change, and the whole
  dictation read once at the stop. Your own voice: 10 % of words differ
  from the references against Nemotron's 17 %
  (`docs/research/phone-dictation-latency-2026-11.md`). If the Mac is
  loading or busy it says so and the phone dictates by itself; if it stops
  answering (2.5 s without text, or 5 s after the stop) the phone takes
  over with the audio so far.
- Typing is plain, like Gboard without its extras (defaults in
  `latin/settings/Defaults.kt`): no word suggestions (the strip holds only
  the mic and the dictation status), no double-space period, no symbol
  hints on letters. Auto-capitalisation stays on.
- Autocorrect fixes typos only (`dev.verbatim.sokki.Typos`) and puts in German
  capitals (die liste -> Liste, den link -> Link; not in English sentences, not
  after ich/du/sie: "ich krieg"). Common German and English words ("geil"
  included), compounds, Denglish and your own words are never changed. Any other word is compared with every word one slip away
  (a letter extra, missing or changed, two letters swapped) on one score:
  how well the word fits after the previous word, plus how likely that slip
  is. It is replaced only when one word clearly wins; with no word one slip
  away, common words two slips away compete the same way. Which keys are
  neighbours comes from the layout on screen, and British -ise spellings
  count as words. On the real keyboard
  this fixes about 30% of typos with 0.6% of them wrong, and no correct word
  changed. Backspace right after a fix undoes it, and that word is kept from
  then on. Score, numbers and a replay tool: `tools/typo_model.py`;
  experiments: `tools/score_context.py`, `tools/bench_device.py`.
- German first whenever the phone has German among its languages, always
  with English alongside (and English with German): no switching for mixed
  German/English text. Plain QWERTZ (10/9/7 keys, no umlaut keys: wider
  keys, fewer slips); ä/ö/ü and ß come first on long-press a/o/u/s, and a
  missed long-press is fixed by autocorrect (konnen -> können).
- Colours: themes "Sokki" and "Sokki dark" (`KeyboardTheme.kt`), Sokki's
  greys with its blue for enter; rounded key tiles.
- Holding delete speeds up: letters (after 1 s two at a time), then from
  about 2.5 s one whole word per beat, in the same rhythm as the letters.
- Smart key edges (off by default, `dev.verbatim.sokki.KeyTargeting`): a
  tap near a letter key's edge counts for the likelier neighbour given the
  two letters before (iPhone-style; the middle of a key always gives that
  key). Letter tables: `keyboard/tools/gen_letter_tables.py`.
- Typing test (keyboard settings › Typing test; release builds show it only
  with debug settings on: About, tap the version): 40 German sentences;
  records only the taps in its own field to
  `files/typing_test/*.jsonl`. `keyboard/tools/typing_eval.py replay`
  shows the typing pattern and geometry vs. smart edges on those taps;
  `sim` runs synthetic taps.
- Tap recording in all apps (off by default; switch in the typing test):
  `files/typing_log/taps-<date>.jsonl`, never in password or incognito
  fields. `typing_eval.py daily` counts misses (a letter deleted at once and
  retyped as a neighbour), smart-edge decisions deleted at once, and what
  smart edges would fix or break on those taps.
- Journal of everything written, the developer page "Keep what I write"
  (app, Dictate tab, bottom link; off by default): `files/typing_log/journal-<date>.jsonl`,
  one entry per message or edit (the field's text just before it was emptied
  or left, with app, time and any text it already held). Typing, dictation
  and pasting alike; a dictation adds `dictated: [{at, raw}]`, what the speech
  engine wrote, so raw transcript and sent text sit side by side. Never
  password, number/PIN or incognito fields. On while `typing_log/journal_on`
  exists. The page lists the days, shows each entry, sends the days to the
  paired Mac (link request `journal`; Mac: `<app data>/typing_log/phone/`) and
  deletes them. Test without a Mac: `cargo run --example journal_mac -- <dir>`
  in `crates/sokki-link` prints a pairing link for the emulator.
- Missing space after punctuation: a word typed right after "." "," "!" "?"
  ":" ";" gets its space when it ends ("done.we " -> "done. We ";
  `dev/verbatim/sokki/Spacing.kt`, tests in `keyboard/src/test`, run with
  `./gradlew :keyboard:testDebugUnitTest` in `src-tauri/gen/android`). Links,
  files, numbers and one-letter abbreviations ("z.B.") stay joined. After a
  dictation, the next typed word gets a space too.
- Typing logs and the journal are left out of Android backups and device
  transfers (`res/xml/sokki_*_rules.xml`).
- Setup in the app (Dictate tab): two buttons open Android's keyboard list
  and picker; a third asks for the microphone if needed.
- Debug builds: `Android/data/org.sokki.app/files/sokki_test.wav`
  (16 kHz mono) is "spoken" instead of the microphone, for tests on the
  emulator.
- The keyboard's file providers (clipboard history, gesture data) are
  named after the app ID (`org.sokki.app.clipprovider`,
  `org.sokki.app.gesturedata`), not HeliBoard's fixed names, so Sokki
  installs next to HeliBoard (KNOWN_ISSUES X12).

## License

GPL-3.0 ([LICENSE](LICENSE)): the keyboard is built on
[HeliBoard](https://github.com/HeliBorg/HeliBoard) (GPL-3.0), so the Android
app as a whole is GPL-3.0. Sokki's own files here are GPL-3.0 too. The keyboard's letter tables and word table (`assets/sokki/letters_*.bin`, `words.bin`) are derived
from [wordfreq](https://github.com/rspeer/wordfreq) word frequencies (CC-BY-SA 4.0). The word-pair
table (`assets/sokki/bigrams.bin`) is derived from sentences of the German and English
[Wikipedia dumps](https://dumps.wikimedia.org/) (CC BY-SA 4.0) and
[Tatoeba](https://tatoeba.org) sentences (CC BY 2.0 FR). The shared
crates in `../crates/` stay MIT.

## Build and test (no hands needed)

```sh
source scripts/android-env.sh                 # Java, SDK, NDK, Rust
bun install
bun run tauri android build --debug --target aarch64 --apk
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

- Emulator: `emulator -avd verbatim_pixel7 -no-window -no-audio`.
- `bun scripts/cdp.ts add <audio> <seconds>` adds a recording without the
  microphone (debug builds); `bun scripts/cdp.ts eval '<js>'` reads app state;
  `window.__verbatim.pair(ticket)` and `.checkAgain()` drive the link.
- Screens: `adb exec-out screencap -p > shot.png`; taps: `adb shell input tap x y`.
- New worktree with `src-tauri/target` copied from another checkout: the
  build fails with "Unresolved reference 'TauriActivity'". Tauri's build
  script writes `gen/android/.../generated/TauriActivity.kt` but only runs
  again when that file changes at the old checkout's path. Run this once,
  then build again:
  `cargo clean -p tauri --target aarch64-linux-android --manifest-path src-tauri/Cargo.toml`

The Mac side is `src-tauri/src/meeting/phone_link.rs` in the main app.

## Later (Robin's idea, 2026-11-18)

One app for three things: meetings, your own voice notes (copy the text),
and a **keyboard**. The keyboard is the normal keyboard plus a microphone at
the top right that dictates into any app, and maybe a second button that
opens Sokki to start a meeting. On Android that is an `InputMethodService`
(native Kotlin) that calls the same Rust speech engine. Call detection
(incoming and outgoing calls) comes later. For now, the user starts every
recording.
