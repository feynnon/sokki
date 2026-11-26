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

## Streaming (`--stream`), what the keyboard does since 2026-11-25

`VB_RIGHT=6 asr-bench nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf audio.wav --stream`
feeds 200 ms blocks in real time into the model's own stream, as the Mac
does with a streaming model: the model keeps its context and adds words
about every half second (`VB_RIGHT`: lookahead in 80 ms frames). It prints
when the text grows, how far it lags behind the audio, how busy the engine
is, and whether a word already shown ever changed (`rewrites`: none so far,
so the keyboard types words for good as they come). `VB_SCRIPT=file.txt`
compares the final text with the script, `VB_LANG=de-DE` sets the language,
`VB_THREADS` the CPU threads, `VB_PAGEOUT=1` (also for `--live`) pushes the
process out of memory after the warm-up, as Android does to an idle keyboard.

Why: the Mac's live text (Parakeet Ultra) re-reads the growing piece every
0.5 s on the GPU. A phone CPU can't, so the keyboard read 1–1.5 s snippets
without context plus whole pieces at pauses, two passes that fought over
the CPU. A streaming model reads each second once, with context.

Nemotron Streaming 3.5, made-up speech (macOS voices), vs the script:

| Clip | Lookahead | First words | Text behind speech (median / worst) | Off the script |
|---|---|---|---|---|
| German, 26 s (Anna), Mac Metal | 0 (80 ms) | 1.5 s | 0.12 / 0.57 s | 2.8 % |
| same | 6 (0.56 s) | 1.9 s | 0.08 / 0.37 s | 0.0 % |
| same | 13 (1.1 s) | 2.4 s | 0.17 / 0.61 s | 0.0 % |
| German, 22 s (Anna), Mac, 6 | | 1.9 s | | 0.0 % |
| English, 14 s (Samantha), Mac, 6 | | 1.4 s | 0.46 / 1.08 s | 4.7 % |
| German, 26 s (Anna), emulator CPU, 6 | | 1.9 s | 0.21 s | 0.0 % (engine busy 44 %) |

(Clips start with 0.5 s of silence.) Parakeet Ultra (the model before),
each clip transcribed whole: German 26 s 2.8 %, German 22 s 2.3 %, English
0.0 %. The robotic "Flo" voice failed with both models (Nemotron 86–98 %,
Ultra 91 %); Anna reading the same text: 0 % / 2.3 %. The language hint barely changes
anything (English with `de-DE`: 2.3 %; German with `en-US`: 11 %), so the
keyboard lets the model detect it.

Threads matter more than anything else: the engine's threads wait for each
other, so one thread pushed aside stalls all. Emulator (4 cores) with two
other busy processes, German 26 s: 4 threads, text 41 s behind (engine
busy 247 %); 3 threads 0.26 s (47 %); 2 threads 0.25 s (68 %). The keyboard
uses all cores but two, at most 4. The old keyboard ran two passes of 4
threads on the Pixel's 8 cores; whenever both ran, a 1 s snippet took 3.2 s
instead of 0.4 s (Robin's Pixel, 2026-11-24).

On Robin's Pixel 7 (2026-11-25), German 26 s clip, lookahead 6: text 0.23 s
behind the speech (worst 0.28 s), engine busy 41 % with 4 threads (3: 47 %,
2: 47 %); with two other busy processes 0.38 s (4 threads). Pushed out of
memory first (`VB_PAGEOUT=1`): first words unchanged at 2.0 s, slowest feed
0.67 s. Lookahead 0 can't keep up on the phone (engine busy 130 %, text up
to 8 s behind). The old two-pass keyboard on the same clip (`--live`,
`VB_CHUNK=1.0:1.5 VB_PAR=4:4 VB_LIVE=6:15:0:0.5`): first words 2.5 s, live
text frozen 3.9 s at the first pause, final text 2.8–4.1 s after each pause.

In the keyboard on the Pixel (the same clip as `sokki_test.wav`, logcat
`SokkiVoice`): first words 1.8 s after it starts listening (was 2.9 s),
longest wait for new words 1.2 s (was 4.1 s), done 0.2 s after stop (was
3.1 s), ready 1.0 s after Android closed the keyboard (Parakeet Ultra:
17–22 s), back in memory 0.7 s after opening the keyboard following 20 hours
unused.

### Nemotron vs Parakeet Ultra, English, German and mixed (2026-11-25)

13 made-up clips (macOS voices Samantha, Daniel, Anna; mixed = Anna reading
German with English words, and one dictation switching German → English →
German), 383 words. Ultra transcribes each clip whole (its best case);
Nemotron live (`--stream`). Word errors, and errors counting punctuation
and capitals too:

| | English (133 words) | German (149) | Mixed (101) | All words | With punctuation |
|---|---|---|---|---|---|
| Parakeet Ultra, whole | 5 | 3 | 14 | 22 (5.7 %) | 44 |
| Nemotron live, lookahead 6 | 7 | 3 | 19 | 29 (7.6 %) | 59 |
| Nemotron live, lookahead 13 | 6 | 3 | 17 | 26 (6.8 %) | 52 |

Both stumble on English words in German ("Friday" → "FreeDive" /
"Friedai"). Ultra often joins English sentences with commas; Nemotron
leaves out the last period (K11). Ultra can't stream (`stream begin`
refuses it, also as buffered stream), so live text with Ultra means
re-reading audio, which the phone's CPU can't keep up with. Kept:
Nemotron, lookahead 6.
