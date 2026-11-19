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

## Keyboard (voice typing in every app)

`keyboard/` is HeliBoard 4.2 (see `keyboard/UPSTREAM`) built into the app
as a library: one install has the app and the keyboard. Changes to
HeliBoard are marked "Sokki" in the code:

- The mic is pinned at the top right of the suggestion strip. Tapping it
  starts `dev.verbatim.sokki.SokkiVoice`: microphone → Rust
  (`src-tauri/src/keyboard_voice.rs`, same engine and cutting as the
  Dictate tab) → text typed into the open field piece by piece. Tap again
  to stop; the mic is red while listening.
- Setup in the app (Dictate tab): two buttons open Android's keyboard list
  and picker; a third asks for the microphone if needed.
- Debug builds: `Android/data/dev.verbatim.mobile/files/sokki_test.wav`
  (16 kHz mono) is "spoken" instead of the microphone, for tests on the
  emulator.

## License

GPL-3.0 ([LICENSE](LICENSE)): the keyboard is built on
[HeliBoard](https://github.com/HeliBorg/HeliBoard) (GPL-3.0), so the Android
app as a whole is GPL-3.0. Sokki's own files here are GPL-3.0 too. The shared
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

The Mac side is `src-tauri/src/meeting/phone_link.rs` in the main app.

## Later (Robin's idea, 2026-11-18)

One app for three things: meetings, your own voice notes (copy the text),
and a **keyboard**. The keyboard is the normal keyboard plus a microphone at
the top right that dictates into any app, and maybe a second button that
opens Sokki to start a meeting. On Android that is an `InputMethodService`
(native Kotlin) that calls the same Rust speech engine. Call detection
(incoming and outgoing calls) comes later. For now, the user starts every
recording.
