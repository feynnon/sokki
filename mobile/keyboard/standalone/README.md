# Sokki Keyboard

An Android keyboard with voice typing that never leaves the phone. It is the
keyboard of [Sokki](https://sokki.org), the free, open-source, local-first
dictation and meeting-notes app, on its own so you can fork it.

Built on [HeliBoard](https://github.com/HeliBorg/HeliBoard) (GPL-3.0), which
is built on the AOSP keyboard. `UPSTREAM` names the HeliBoard version.

What Sokki adds to HeliBoard:

- **Voice typing on the phone.** Tap the mic, speak, the text appears in any
  app while you talk. Speech recognition runs on the device (Parakeet via
  [transcribe-cpp](https://crates.io/crates/transcribe-cpp)); no audio or
  text is sent anywhere.
- **German and English together**, German capitals, umlauts next to the
  letters and on long-press, `ue`/`ss` for `ü`/`ß`.
- **Autocorrect that only fixes typos**, scored on context, never swaps a
  real word.
- **The missing space after punctuation**: "this.we" becomes "this. We"
  when the next word ends; links, files and "z.B." stay as typed.
- Smart key edges, held delete that speeds up to whole words, Sokki's look.

## Build

Needs JDK 17, the Android SDK and NDK, and for voice typing Rust with
[cargo-ndk](https://github.com/bbqsrc/cargo-ndk).

```sh
./gradlew assembleDebug
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

`-Psokki.voice=false` builds without the speech engine (the mic then says so).
The speech model (Parakeet, ~600 MB) is downloaded on the phone the first
time you use the mic.

## Where this comes from

This repository is a copy that is updated with each Sokki release. Sokki is
developed in one repository with the Mac and Linux apps; pull requests here
are welcome and are carried over by hand.

## License

GPL-3.0 (`LICENSE`), like HeliBoard. Parts from AOSP are Apache-2.0
(`LICENSE-Apache-2.0`). The word tables in `keyboard/src/main/assets/sokki/`
are derived from wordfreq word frequencies (CC-BY-SA 4.0,
`LICENSE-CC-BY-SA-4.0`), Wikipedia sentences via the Leipzig Corpora
Collection (CC BY-SA) and Tatoeba (CC BY 2.0 FR).
