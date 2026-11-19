# asr-bench

Measures whether a phone can transcribe by itself: loads a Parakeet GGUF and
transcribes a 16 kHz mono WAV, printing load time, speed and peak memory.

```sh
source ../scripts/android-env.sh
mkdir -p android-stubs && $NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin/llvm-ar rc android-stubs/libpthread.a
CMAKE_POLICY_VERSION_MINIMUM=3.5 ANDROID_NDK_ROOT=$NDK_HOME \
RUSTFLAGS="-L $PWD/android-stubs -C link-arg=-lc++_static -C link-arg=-lc++abi" \
cargo ndk -t arm64-v8a -P 30 build --release
adb push target/aarch64-linux-android/release/asr-bench model.gguf audio.wav /data/local/tmp/
adb shell "cd /data/local/tmp && ./asr-bench model.gguf audio.wav"
```

(Android's libc includes pthread; the empty `libpthread.a` satisfies the
engine's link flag, and libc++ is linked statically.)

Pixel 7 (Tensor G2, Android 17), CPU only, 149 s of German speech, 2026-11-18:

| Model | Load | Transcribe | Speed | Peak memory |
|---|---|---|---|---|
| Parakeet v3 Q8 | 22 s | 40 s | 3.7× real time | 1.1 GB |
| Parakeet Ultra Q8 | 17 s | 40 s | 3.7× real time | 1.2 GB |

A 20-minute meeting: about 5½ minutes on the phone vs about 20 s on an
M-series Mac. Same words as on the Mac.

## Dictation latency (model already loaded, Parakeet Ultra, median of 3)

`asr-bench model.gguf audio.wav --dictation`. Time from "stop speaking" to text:

| Spoken | Pixel 7 (CPU) | Mac M-series (Metal) |
|---|---|---|
| 3 s | 0.82 s | 0.07 s |
| 5 s | 1.17 s | 0.11 s |
| 10 s | 2.30 s | 0.18 s |
| 30 s | 7.26 s | 0.53 s |
| 5 s pieces while speaking, last piece | 1.26 s (each piece ≤ 1.4 s, keeps up) | 0.09 s |

Phone → Mac over home Wi-Fi: 65 ms per request (median of 10, worst 372 ms),
so dictation via the Mac is about 0.3 s for 10 s of speech. Loading the
model on the phone once: 17–22 s; it then stays in memory (1.2 GB).

## Cutting live dictation into pieces (`--chunking`)

How much do pieces cost, compared with transcribing the whole recording?
Deviation from the whole-recording transcript (Parakeet Ultra, Mac):

| Cut | Clean speech (2 synthetic) | Robin's real mic, 3 min | Audio left at stop |
|---|---|---|---|
| fixed 5 s | 7–22 % | 12.4 % | up to 5 s |
| fixed 8 s | 4–5 % | 9.4 % | up to 8 s |
| pauses, min 3 s | 1–4 % | 10.0 % | ~1.4 s |
| pauses, min 6 s, 300 ms, max 15 s (default) | 2–4 % | 6.2 % | ~1.4 s |

Quiet far-field audio (synthetic phone on speaker): 27 % off the script with
the default cut vs 14 % whole. The earlier 30–36 % also included a bug: a
"silence" check skipped pieces it judged empty and so dropped whole sentences
of the quieter voice. It is gone: every piece is transcribed, cutting is only
for speed (test `no_audio_is_lost`). Parakeet returns nothing for 8 s of
digital silence, room noise (RMS 0.002 and 0.006) or 50 Hz hum, so the skip
wasn't needed. A smaller quiet margin (0.0005) helps far-field (21 %) but
leaves an 8.4 s piece at stop on real speech, so the default stays 0.002.
Far-field is the meeting case, which transcribes the whole recording anyway.

Longer pieces (min 6 / 8 / 10 / 12 s) don't measurably help:

| min, max | Clean speech vs script | Real mic vs whole | Far-field vs script | Audio left at a random stop (avg / max) |
|---|---|---|---|---|
| 6 s, 15 s | 8.2 %, 3.8 % | 6.2 % | 27.1 % | 3.5 / 9.9 s |
| 8 s, 16 s | 9.4 %, 3.8 % | 6.8 % | 17.6 % | 4.6 / 12.6 s |
| 10 s, 20 s | 8.2 %, 5.0 % | 7.1 % | 21.2 % | 5.6 / 14.8 s |
| 12 s, 20 s | 9.4 %, 3.8 % | 6.2 % | 18.8 % | 6.5 / 17.1 s |
| fixed 10 s | 7.1 %, 7.5 % | 5.6 % | 16.5 % | 5.0 / 10.0 s |

(Whole recording vs script: 8.2 %, 5.0 %, 14.1 % far-field. The clean clips
have 80–90 words, so one word is ~1.2 %.)

What that means for the wait after stopping on a Pixel 7, which
transcribes about 0.24 s per second of audio: 6 s pieces ≈ 0.8 s on
average and 2.4 s at worst; 10 s pieces ≈ 1.3 s and 3.6 s. Measured live
with 6 s pieces: 0.3–1.0 s. The emulator is no guide here: it needs about
as long as the audio itself (3.6–7.7 s for a 6–7 s piece), so pieces queue
up and the wait reached 14.7 s once.
Pause detection: `dictation-core`.

On the Pixel 7 (CPU, live through the app, 2026-11-18): text ready
0.27–1.04 s after stopping (39 s and 33 s of clean speech); 0.7 s for
Robin's 3-minute real mic.

## Live text (`--live`), as the keyboard does it

`VB_LIVE="min_s:max_s:first_s:step_s" VB_TAIL=0.8 asr-bench model.gguf audio.wav --live`
plays the audio in real time: finished pieces first, and when idle the
growing piece is transcribed as a preview. A piece whose last preview already
heard all of its speech keeps that preview as its final text (no second run).

Pixel 7, Parakeet Ultra, 4–8 s pieces, one run each (2026-11-19):

| Audio | First words | Text behind speech (median / worst) | Wait after stop |
|---|---|---|---|
| 27 s synthetic German | 0.9 s | 1.3–1.7 s / 3.6–4.7 s | 1.9–2.7 s |
| Robin's 149 s real mic | 0.9–1.6 s | 1.3–1.4 s / 4.3–5.2 s | 0.0–1.6 s |

Before (6–15 s pieces, no previews): first words after 6–15 s, wait after
stop 0.3–1.0 s. The engine (≈0.3 s per second of audio on this phone) is
busy most of the time with previews; a third of the pieces skip the second
run. A streaming model would remove the re-reading.

Variants compared on the Pixel 7 (one run each; accuracy = deviation from the
whole recording transcribed at once, Robin's 149 s real mic; 1 % ≈ 3–4 words):

| Variant | First words (real / synthetic) | Wait after stop (real / synthetic) | Accuracy |
|---|---|---|---|
| 6–15 s pieces, no live text (before) | 8.6 / 18.9 s | 0.0 / 2.8 s | 6.5 % |
| first piece 2–4 s, then 6–15 s | 5.0 / 4.9 s | 0.0 / 1.8 s | 6.8 % |
| first piece 3–5 s, then 6–15 s | 6.0 / 5.9 s | 0.0 / 1.8 s | 6.8 % |
| 4–8 s with live text (keyboard now) | 1.5 / 0.9 s | 0.7 / 1.3 s (other runs 0–2.7 s) | 8.3 % |

Without live text the text arrives in steps of 6–15 s. A first piece that
waits for a 300 ms pause doesn't help: the first such pause often comes
only at the 15 s limit (`VB_LIVE=…:first_min`, first piece ends 2 s after it).

### Snippets plus final pieces (what the keyboard does now)

`VB_CHUNK=1.0:1.5 VB_PAR=4:4 VB_LIVE=6:15:0:0.5`: live text from 1–1.5 s
snippets, each read once on one session; pieces cut at pauses after 6–15 s
read whole on a second session in parallel (same model). Pixel 7, one run each:

| Audio | First words | Live text behind | All words at stop | Final text vs whole | Snippets alone |
|---|---|---|---|---|---|
| Robin's 149 s real mic | 1.4 s | 0.7 s (worst 1.9 s) | 0.0 s | 6.5 % | 38 % |
| 27 s synthetic | 1.4 s | 0.7 s (worst 3.5 s) | 0.0 s | 1.3 % | 8 % |

The final text of each piece arrives 2–4 s after its pause. Threads: 4+4 best;
2+4 lagged (0.6 s at stop on real mic); library default similar to 4+4.
Without the second session, a stop during a long final pass waited up to 2.9 s.
