# Screen recording size ("Meeting with screen"), 27 Nov 2026

**Task (Robin, 2026-11-27):** "Right now the vid is stored in original resolution: WAY TOO
BIG! … as much compression as possible. Still having text readable: Great. But don't
overdo it." Find the smallest setting at which 11–12 pt text on screen still reads, make it
the default, and encode during capture.

## How it was recorded before

- ScreenCaptureKit (`src-tauri/swift/video_capture.swift`) at the display's full pixel size
  (3456×2234 on a 14" MacBook Pro), at most 30 frames a second (only changed frames
  arrive), written by AVAssetWriter as HEVC with **no compression settings** (the encoder's
  default bit rate).
- After Stop, `AVAssetExportPresetHighestQuality` encoded the video **a second time** into
  the `.mp4`, and `screen.mov` stayed next to it: every recording was stored twice.
- Files: `~/Movies/Sokki/<date time>/`. On 2026-11-27 that folder did not exist on the
  maintainer's Mac (no screen recordings kept), so the "before" numbers come from the bench
  below, not from real files. Nothing old to shrink.

## Method

- **Screens:** set `screen-capture-made-up` (`docs/evals.md`): a slide, a dark code editor
  (12 px Menlo), a document page in 11 px and 12 px serif (11/12 pt at 100 %), a web page four
  screens high (13 px). All made up, rendered at 2× with headless Chrome.
- **Timeline** (`scripts/screen-eval/bench.swift`, sample `75s-v1`): 75 s of slides, typing
  in the editor, the static document, the web page scrolling one screen in 3 s then resting
  2 s, and a 10 s "video call" (four tiles, moving shapes, camera noise). As with
  ScreenCaptureKit, a frame is written only when the screen changed, at most *fps* a second.
- **Encode:** AVAssetWriter, hardware HEVC/H.264 (VideoToolbox), offline. Scaling with
  CoreGraphics (high quality) stands in for ScreenCaptureKit's GPU scaler.
- **Readability:** Vision OCR (accurate, English, **language correction off**, so blurry
  words are not guessed) on the decoded frame scaled back to screen size, as a viewer sees
  it full screen. Score = share of small-text words that OCR finds in the original frame and
  also finds in the encoded one (numbers counted separately: 6/8, 1/7 go first). 8 frames:
  slide, code, document, web page resting (3) and mid-scroll (2).
- **Mac:** MacBook Pro 14" M1 Pro, macOS 26.7.3.
- Run: `scripts/screen-eval/run.sh` (through `build-slot`), outputs in
  `$SOKKI_EVALS/runs/screen-capture/2026-11-27/` (results.jsonl, crops to look at).

## Results

MB per minute of the 75 s timeline. OCR columns: share of small words found (1.00 = all).
"Doc 11/12 pt" also lists the share of numbers.

| Setting | Pixels | fps | MB/min | Doc 11/12 pt (words / numbers) | Code 12 px | Slides | Web resting | Web scrolling |
|---|---|---|---|---|---|---|---|---|
| **Before: capture** (HEVC, encoder default) | 3456×2234 | 30 | **10.40** | 1.00 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| **Before: + "highest quality" export** | 3456×2234 | 30 | **+16.01** | | | | | |
| Native, HEVC q 0.5 | 3456×2234 | 2 | 1.42 | 1.00 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| 1× logical, HEVC q 0.5 | 1726×1116 | 2 | 0.44 | 0.99 / 1.00 | 0.98 | 1.00 | 1.00 | 1.00 |
| 1080, HEVC q 0.3 | 1670×1080 | 2 | 0.15 | 0.73 / 0.91 | 0.69 | 1.00 | 0.86 | 0.91 |
| 1080, HEVC q 0.4 | 1670×1080 | 2 | 0.19 | 0.92 / 0.96 | 0.93 | 1.00 | 0.99 | 0.99 |
| **1080, HEVC q 0.5 (chosen)** | 1670×1080 | 2 | **0.27** | **0.99 / 1.00** | **0.99** | **1.00** | **1.00** | **1.00** |
| 1080, HEVC q 0.6 | 1670×1080 | 2 | 0.43 | 1.00 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| 1080, HEVC q 0.7 | 1670×1080 | 2 | 0.65 | 0.99 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| 900, HEVC q 0.4 | 1392×900 | 2 | 0.23 | 0.74 / 0.91 | 0.77 | 0.90 | 0.93 | 0.93 |
| 900, HEVC q 0.5 | 1392×900 | 2 | 0.33 | 0.95 / 0.98 | 0.96 | 1.00 | 0.99 | 1.00 |
| 900, HEVC q 0.6 | 1392×900 | 2 | 0.52 | 0.97 / 1.00 | 0.98 | 1.00 | 1.00 | 1.00 |
| 720, HEVC q 0.5 | 1114×720 | 2 | 0.16 | 0.65 / 0.85 | 0.73 | 0.93 | 0.93 | 0.94 |
| 720, HEVC q 0.6 | 1114×720 | 2 | 0.27 | 0.81 / 0.89 | 0.90 | 0.93 | 0.98 | 0.99 |
| 720, HEVC q 0.7 | 1114×720 | 2 | 0.43 | 0.83 / 0.87 | 0.95 | 1.00 | 0.98 | 0.99 |
| 1080, HEVC q 0.5 | 1670×1080 | 1 | 0.33 | 0.99 / 1.00 | 0.99 | 1.00 | 1.00 | 1.00 |
| 1080, HEVC q 0.5 | 1670×1080 | 5 | 0.31 | 0.99 / 1.00 | 0.99 | 1.00 | 1.00 | 1.00 |
| 1080, **H.264** q 0.5 | 1670×1080 | 2 | 0.63 | 0.99 / 0.98 | 1.00 | 1.00 | 1.00 | 1.00 |
| 1080, HEVC 300 kbit/s | 1670×1080 | 2 | 1.02 | 0.99 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |
| 1080, HEVC 600 kbit/s | 1670×1080 | 2 | 1.19 | 0.99 / 1.00 | 1.00 | 1.00 | 1.00 | 1.00 |

What it shows:

- **Resolution and the constant-quality setting do the work.** Full resolution at 30 fps
  with the default bit rate is 38× the chosen setting, and the old export added 1.5× that
  again.
- **Below 1080 lines, 11 pt text breaks first** (900: 95 % of words, 720: 65–83 %). Below
  quality 0.5 it breaks too (0.4: 92 %).
- **Frame rate barely matters for size** (1 fps 0.33, 2 fps 0.27, 5 fps 0.31 MB/min): an
  unchanged screen sends no frames, and constant quality makes small changes cheap. 1 fps
  was even larger (bigger jumps between frames). 2 fps keeps scrolling and typing followable.
- **H.264 is 2.3× HEVC** at the same quality; both run on the hardware encoder.
- **A fixed bit rate is the wrong tool** for screens: it fills the static parts (300 kbit/s
  was 3.8× the chosen size for no visible gain).
- Constant camera noise (the "video call" part) is the most expensive content; it is
  included above.

Crops of the chosen setting (document 11/12 pt, code 12 px) read cleanly by eye:
`$SOKKI_EVALS/runs/screen-capture/2026-11-27/h1080-q5-document.png`, `h1080-q5-code.png`.

## CPU and energy

Real time (75 s, `scripts/screen-eval/energy.sh`), CPU seconds per minute of recording:

| | Before (native, 30 fps) | After (1080, 2 fps, q 0.5) |
|---|---|---|
| Encoder, CPU side (`encode_cpu_s`) | 0.47 s/min (0.8 % of one core) | 0.09 s/min (0.15 %) |
| Frames handed over (stand-in for capture; `draw_cpu_s`) | 638 frames of 7.7 Mpixel, 4.3 s/min | 73 frames of 1.8 Mpixel, 0.8 s/min |
| `VTEncoderXPCService` (top) | 0.2 % CPU | not seen |

The encoding itself runs on the media engine and costs almost no CPU either way. What
shrinks is the work around it: 8.7× fewer frames and 4.3× fewer pixels per frame to
capture, scale, copy and encode (≈ 37× fewer pixels a minute). `top`'s energy impact of the
bench process is not shown: it also contains the OCR step at the end. The media engine's and
GPU's own energy needs `powermetrics` (sudo) and was not measured. Audio capture runs on its
own threads and is not touched by any of this.

## Choice

**HEVC (hardware), at most 1080 pixels high, at most 2 frames a second, quality 0.5, a key
frame every 10 s**: `ENCODING` in `src-tauri/src/video/mod.rs`. Encoded once, during the
capture; ScreenCaptureKit scales on the GPU, so the smaller size also costs less to capture.
The render now copies that video into the `.mp4` unchanged (no second encode), mixes
microphone and call sound into one mono AAC track (16 kHz, 32 kbit/s ≈ 0.24 MB/min) and
deletes `screen.mov` afterwards.

| | Before | After |
|---|---|---|
| Video on disk per minute (bench) | 10.40 (`screen.mov`) + 16.01 (`.mp4`) = **26.4 MB** | **0.27 MB** (only in the `.mp4`) |
| Plus sound | in the `.mp4` (AAC, export default) and the FLAC copies | ≈ 0.24 MB AAC in the `.mp4`, plus the FLAC copies (≈ 0.2–0.7 MB/min each) as before |

No setting was added: one default fits (CLAUDE.md "As few rules as possible").

**Limits:** a 1080-line cap is the 1× size of a 4K screen and below 1× on a 5K or 6K screen
(27"/32" at 2×), where 11 pt text gets fewer pixels than measured here. Not measured; add a
"max height = logical height" rule only if a real recording shows unreadable text. Real
ScreenCaptureKit scaling was not part of the bench (see the app test below).

## App test

`WITH_SCREEN=<bench video> scripts/test-meeting.sh mic.wav system.wav 30` in Sokki Test
(headless, made-up call `testdata/audio/call_tracks`), 2026-11-27:

- The `.mp4`: HEVC 1670×1080, 73 frames in 75 s, **one AAC track (mono, 16 kHz)**,
  456,430 bytes = **0.36 MB per minute with sound**, ready 1.8 s after the stop (the old
  export re-encoded: 11 s for 75 s of video in the bench). `screen.mov` was deleted after
  the render.
- **Found on the way: the videos had no sound.** AVFoundation decoded zero samples from
  Sokki's own FLAC files: their STREAMINFO said min frame size 0xFFFFFF, max 0 (the writer
  never filled them in). ffmpeg and Sokki's own decoder ignore that, AVFoundation does
  not. Fixed in `src-tauri/src/meeting/flac.rs` (real frame sizes at `finish`); older FLAC
  files keep the bad header.
- Sokki Test has no Screen Recording permission, so the test used the bench's file instead
  of ScreenCaptureKit (the app never asks in test mode). Real capture at 1080/2 fps is the
  same API with a smaller `width`/`height` and a longer `minimumFrameInterval`; it is
  checked by the same test once Sokki Test may record the screen.
- Unit tests: `cargo test --lib -- meeting::flac video::` (with `SOKKI_VIDEO_FIXTURES`: the
  `.mp4` is at most the screen video plus twice the sound, so no second encode).

## Sources

- Apple, AVFoundation `AVVideoQualityKey` (constant-quality encoding, 0.0–1.0),
  `AVVideoMaxKeyFrameIntervalDurationKey`, `AVAssetWriterInput(outputSettings: nil,
  sourceFormatHint:)` (passthrough), `AVAssetReaderAudioMixOutput`:
  https://developer.apple.com/documentation/avfoundation
- Apple, ScreenCaptureKit `SCStreamConfiguration` (`width`, `height`,
  `minimumFrameInterval`): https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration
- Apple, Vision `VNRecognizeTextRequest`: https://developer.apple.com/documentation/vision/vnrecognizetextrequest
