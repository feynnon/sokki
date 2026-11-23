# Sokki: developer guide

Sokki is a fork of [Handy](https://github.com/cjpais/Handy) (MIT) that adds
meeting notes to its dictation: record a meeting, and get a Markdown note
with speakers, a summary, decisions and action items. Everything runs on
the Mac, unless you choose a cloud language model (see 7). This file covers
the meeting pipeline, code layout and tests; the [README](README.md) is the
user-facing overview.

The app has its own bundle identifier (`org.sokki.app`), so it never shares
settings with an installed upstream app. Data stored under the identifier it
had before (2026-11-23) moves over on the first start
(`src-tauri/src/legacy_data.rs`). Updates come from Sokki's own feed
(`github.com/feynnon/sokki`).

## Look

Own name, V-wave app and menu bar icons, a calm blue on dark grey. Files:
`src-tauri/brands/sokki/` (icon sources and set),
`src-tauri/resources/brands/sokki/` (menu bar icons),
`src-tauri/src/brand.rs`, `src/lib/brand.ts`, `src/styles/brand-sokki.css`,
`src/components/icons/SokkiLogo.tsx`. Some screens still carry branches for
the upstream look (`IS_SOKKI` is always true); they go once the main window
work has landed.

Sokki also has a simpler window, after Superwhisper: sidebar Home,
Meetings, Vocabulary | Settings, Sound, Models; descriptions under each
option instead of info icons; no footer bar; no menu bar icon (the meeting
pill shows recording); first start is permissions, then the model. Options
not shown keep their defaults and stay in the config file
(`src/components/settings/sokki/`). The Vocabulary list goes only into the
meeting summary prompt: the direct dictation word swap turned „Lied“ into
„Lead“ and „Mach“ into „Nuxa“ with these words.

Icons are drawn as SVG in
`src-tauri/brands/sokki/src/`; after a change: `rsvg-convert` to PNG, then
`bun run tauri icon src-tauri/brands/sokki/src/icon-1024.png -o src-tauri/brands/sokki/icons`.

## What it does

1. **Dictation** – as in upstream Handy.
2. **Meeting recording** – shortcut ⌥M, tray item, the Meetings page, or
   automatically (see 3). A small pill at the top of the screen shows a
   waveform, the time and a stop button.
   - Two tracks: microphone and system audio (the remote side of a call on
     this Mac). Without system audio (no permission, no call) the meeting
     is mic-only; this is detected automatically.
   - Recording code from [Mandy](https://github.com/yuri-research-ai/Mandy) (MIT).
3. **Meeting detection** – when Zoom, Teams, Meet in a browser, FaceTime,
   Slack, … start using the mic, a dialog asks whether to record. When the
   call ends, the recording stops after 20 s unless you say otherwise.
   Approach from [Lark](https://github.com/bkoleo/lark) (MIT).
4. **During the recording** (`sokki-engine/src/live.rs`): both tracks go,
   sample for sample as written to their files, to one worker at low
   priority. It lines them up, removes the echo from the mic as a stream
   (the same filter as step 5.1, bit-identical), cuts each track into
   8–25 s pieces at the quietest point and transcribes them one at a time
   with word times (one 41-minute call: 338 pieces, 3 min of GPU work over
   the call). Every 5 minutes it also finds the speakers of the last 6
   minutes of each track (Sortformer, as in step 5.2) and joins the
   windows into one set of speakers per track (`meeting-core/src/stitch.rs`):
   a window's speaker is the one it shares the 30 s of committed overlap
   with, else the closest voice (cosine ≥ 0.6), else new; the last 30 s of
   each window are left to the next. Three real calls: 99.2–100 % of the
   speech has the same speaker as diarizing the whole track, 99.8–100 % of
   transcript lines. Once a window's speakers are final, the turns its
   pieces can't cover (a piece across a change of speaker) are transcribed
   too, exactly as step 5.4 will ask for them: the run at the end finds
   them by their audio (`live::TextCache`) and gets the same text. At the
   stop the last pieces and the last speaker window of each track run side
   by side, and the run takes over the echo-free mic instead of filtering
   it again (`live::take_mic`). Speakers run on their own thread, so
   pieces keep flowing while a window is diarized. Between the 5-minute
   windows a provisional window every 2 minutes diarizes as if the call
   ended there; at the stop only the rest is left (at most ~3 minutes of
   audio). Measured on a simulated 13-minute call: the note 6.8 s after
   the stop (was 22 s with live speakers, 41 s without the live part), the
   app's CPU during the call 30 % on average (13 % without provisional
   windows). It keeps both tracks in memory while the
   call runs (~230 MB per hour each), as the run at the end would.
5. **Processing** after the meeting (measured: a 21-minute call is ready
   ~3 minutes after stopping):
   1. decode, align the two tracks; remove the remote voices from the mic
      track with WebRTC echo cancellation (AEC3, pure-Rust `sonora`,
      `meeting-core/src/aec.rs`, ~6 s per 20 min) so they are not found as
      an extra speaker on the mic;
   2. speakers per track: Sortformer on the GPU in a child process
      (`meeting/diarize_worker.rs`, ~20 s per 20 min of audio; CPU fallback);
      (merging voices that sound alike exists but is off, see
      `DiarizeConfig::merge_similarity`);
   3. drop mic turns that are acoustic echo of the system track;
   4. transcribe each speaker turn (Parakeet, the dictation engine, ~20 s); a
      turn that pieces from the recording cover takes its words from them
      instead, when each of those pieces holds that one speaker only
      (`pipeline::texts_from_pieces`; 41-minute call: 180 of 231 turns,
      76 s → 15 s; word errors on call1402's checked passages 7.4 % turn by
      turn, 7.2 % from pieces);
   5. text dedup, merge ghost speakers, names: on an online call the mic
      voice is you; on a single mic nobody is named; remembered voices by
      their embedding;
   6. summary, decisions, tasks with the local LLM (Qwen 3.5 9B built in:
      llama.cpp's `llama-server` on the GPU, ~70–90 s for a 41-min meeting
      on an M1 Pro; started for the summary, stopped right after); the
      dictionary is in its prompt, so names come out spelled right. The
      transcript stays as recognised: a separate correction pass cost a
      whole model round and was removed on 2026-11-20 (KNOWN_ISSUES R17);
   7. Markdown note in the notes folder.
6. **Speakers** – rename "Speaker 2" in the app; with *Remember voice* the
   voice is recognised in later meetings (sherpa-onnx embeddings).
7. **Language model** – summary (5.6). Chosen under
   Models → *Language model*, a list like the speech models: Qwen 3.5 9B on
   this Mac, built in (the page downloads the model, 5.7 GB, checksum
   verified) or through Ollama (started only by hand), or Claude / GPT through Anthropic, OpenAI or OpenRouter with
   an API key (the transcript then goes to that provider; the key is shared
   with dictation post-processing). No temperature is sent: every model runs
   at its own default. Other models can be set in the config file
   (`meetings.llm_provider`, `llm_model`, `cloud_model`). Request shapes are
   covered by tests against a fake server (`summary.rs`); a real-request
   check: `MEETING_CLOUD=anthropic MEETING_CLOUD_MODEL=claude-opus-5-5 MEETING_CLOUD_KEY=… cargo test -p meeting-core cloud_summary_smoke -- --ignored --nocapture`.
8. **Safety** – a memory guard stops the app above 6 GB while a meeting is
   processed (`meeting/memguard.rs`); recordings are closed at quit and
   processed at the next launch.

## Videos (screen and camera)

A video is a meeting recording (microphone and system sound, so it gets a
transcript and note like any meeting) plus a full screen video and/or a full
camera video, recorded with ScreenCaptureKit and AVFoundation
(`src-tauri/swift/video_capture.swift`, Rust side `src-tauri/src/video/`).
Sokki's own windows (the camera bubble, the recording bar, the pill) are left
out of the screen video. After Stop, Sokki renders the chosen look into one
`.mp4` with sound: bubble, side by side or vertical (9:16); "two files"
renders nothing. The look can be changed later on the Videos page. Files go to
`~/Movies/Sokki/<date time>/`.

Starts: "New video" on the Videos page opens the recording bar
(`src/video-bar/`); "Meeting with screen" on the Meetings page and "Mit
Bildschirm" in the call-detected dialog record a meeting with its screen.
`videos.enabled` off hides all of it.

Tests: `SOKKI_VIDEO_FIXTURES=<dir with screen.mov, camera.mov, mic.flac>
cargo test --lib video::` renders every look from sample files. Capture
itself needs Screen Recording and Camera permission, so it is not tested
automatically yet (`SOKKI_TEST_SCREEN` / `SOKKI_TEST_CAMERA` skip capture and
copy those files in at Stop, but meetings have no test mode for the audio).

## Layout

| Path | What |
|---|---|
| `crates/sokki-engine` | One meeting in, one note out (`process::run`, shared by the Mac app and the Linux service); the meeting lifecycle state machine; language-model settings, status, the built-in model server, Ollama |
| `crates/sokki` | The Linux/Omarchy host: `sokki daemon` (PipeWire recording, call detection, dictation), the CLI and the processing worker. See ARCHITECTURE.md |
| `crates/meeting-core` | Platform-independent pipeline: types, pipeline, acoustic echo filter, text dedup, LLM summary (Ollama / OpenAI-compatible, map-reduce for long meetings), Markdown notes, speaker rename |
| `crates/diarize` | Diarization: Sortformer (default) and sherpa-onnx backends, speaker embeddings, voice profiles |
| `crates/meeting-detect` | Which processes use the mic (Core Audio), meeting-app matching, debounce watcher |
| `crates/meeting-e2e` | Headless end-to-end harness: real models on synthetic fixtures, scored against ground truth; examples for real-call evals (`raw_transcript`, `summary_eval`, `diarize_once`; `live_eval`: transcript during the call vs after it, time and word difference) |
| `testdata/generate.py` | Builds the synthetic fixtures (macOS voices + ffmpeg) into `testdata/audio/` (gitignored) |
| `testdata/real/` | Real-call eval data (gitignored, private): uncorrected transcripts, hand labels (34 misheard words, 37 words that must stay), dictionary |
| `scripts/correction_eval.py` | Scores speech models' transcripts (and, from the removed correction step, correction outputs) against `testdata/real` labels |
| `src-tauri/src/meeting/` | App side: capture (`mic.rs`, `system_tap.rs`, `flac.rs`), `process.rs`, `store.rs` (meeting index), `detect.rs` |
| `src-tauri/src/commands/meeting.rs` | Tauri commands and the `MeetingChanged` event |
| `src/components/settings/meetings/` | Meetings page |

Data on disk:
- meeting audio + index: `~/Library/Application Support/org.sokki.app/meetings/`
- voice profiles: `…/org.sokki.app/voice-profiles.json`
- diarization models: `…/org.sokki.app/meeting-models/` (downloaded on first use)
- notes: the folder set on the Meetings page (default `~/Documents/Sokki`)

## Tests

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" CMAKE_POLICY_VERSION_MINIMUM=3.5
python3 -I testdata/generate.py                      # once: synthetic fixtures

(cd crates/meeting-core   && CARGO_TARGET_DIR=/tmp/hm-target-core   cargo test)
(cd crates/meeting-detect && CARGO_TARGET_DIR=/tmp/hm-target-detect cargo test)
(cd crates/sokki-engine   && CARGO_TARGET_DIR=/tmp/hm-target-engine cargo test)
(cd crates/diarize        && CARGO_TARGET_DIR=/tmp/hm-target-diarize cargo test --release)
(cd crates/meeting-e2e    && CARGO_TARGET_DIR=/tmp/hm-target-e2e    cargo test --release -- --nocapture --test-threads=1)

# with live summaries from local Ollama
(cd crates/meeting-e2e && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-e2e cargo test --release -- --nocapture --test-threads=1)
(cd crates/meeting-core && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-core cargo test --test live_llm -- --nocapture --test-threads=1)
```

The e2e harness needs `~/.cache/sokki/models/parakeet-tdt-0.6b-v3-Q8_0.gguf`
(see `crates/meeting-e2e/src/lib.rs`). It prints, per scenario, word error
rate, speakers found, speaker attribution and leftover echoes, and writes the
notes to `$TMPDIR/hm-e2e-notes/`.

To add a real recording as a test case: put `mic.wav` (and `system.wav`)
as 16 kHz mono WAV plus a `truth.json` (see an existing scenario) into a new
folder under `testdata/audio/`, and add its name to the scenario lists in
`crates/meeting-e2e/tests/pipeline_e2e.rs`.

### Real-call evaluation (transcripts)

```sh
# transcript of a recording (same pipeline as the app)
(cd crates/meeting-e2e && cargo run --release --example raw_transcript -- mic.wav system.wav ../../testdata/real/<call>/raw.json)
```

Count only first runs for timing: Ollama reuses cached prompt parts on repeats.

### Transcript during the call

```sh
# one recorded meeting: time after the stop and word difference, today vs live
# (prints numbers only; transcripts go to the out dir, keep it outside the repo)
(cd crates/meeting-e2e && MEETING_ASR_MODEL=<parakeet-ultra-Q8_0.gguf> cargo run --release --example live_eval -- <meeting dir> /tmp/live-eval)
# a simulated call in the test app, in real time, with CPU and memory (NO_LIVE=1 for the A/B)
ffmpeg -i <meeting dir>/mic.flac -ar 16000 -ac 1 /tmp/mic.wav     # likewise system.flac
scripts/test-meeting.sh /tmp/mic.wav /tmp/system.wav
```

## Building and installing

```sh
scripts/build-and-install.sh          # build, then install when no meeting is busy
scripts/build-and-install.sh --no-build
```

The script signs with the code-signing certificate **"Sokki Dev"**
from the login keychain when it exists, so macOS keeps the granted
permissions across installs. Create it once (Keychain Access → Certificate
Assistant → Create a Certificate…; Name "Sokki Dev", Identity Type
"Self-Signed Root", Certificate Type "Code Signing"). Without it, builds are
signed ad hoc and every install asks for all permissions again.

Manual build:

```sh
bun install
bun run tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
# → src-tauri/target/release/bundle/macos/Sokki.app
```

Needs Xcode Command Line Tools 26.x (Swift for the system-audio tap). With
Command Line Tools only, Apple Intelligence is built as a stub; meeting
summaries use Ollama instead.

## Keeping up with upstream

```sh
git fetch upstream
git merge upstream/main     # or a release tag
```

The meeting code lives mostly in new files, so conflicts should be rare
(expect them in `lib.rs`, `settings.rs`, `tray.rs`, `bindings.ts` and the
translation files).
