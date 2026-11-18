# Verbatim: developer guide

Verbatim is a fork of [Handy](https://github.com/cjpais/Handy) (MIT) that adds
meeting notes to its dictation: record a meeting, and get a Markdown note
with speakers, a summary, decisions and action items. Everything runs on
the Mac, unless you choose a cloud language model (see 7). This file covers
the meeting pipeline, code layout and tests; the [README](README.md) is the
user-facing overview.

Internally the app is still called **Handy Meetings** (the plain Handy look,
`BRAND=handy`); it has its own bundle identifier
(`dev.handymeetings.app`), so it never shares settings with an installed
Handy. The upstream updater is switched off (`settings.rs`,
`update_checks_forced_disabled`).

## Brands

Two looks from one code base, chosen at build time:

- **Handy Meetings** (`BRAND=handy`): Handy's name, hand logo and pinks, so
  changes can go back upstream.
- **Verbatim** (`BRAND=verbatim`, the default of `scripts/build-and-install.sh`):
  own name, V-wave app and menu bar icons, a calm blue on dark grey. Files:
  `src-tauri/brands/verbatim/` (config overlay, icon sources and set),
  `src-tauri/resources/brands/verbatim/` (menu bar icons),
  `src-tauri/src/brand.rs`, `src/lib/brand.ts`, `src/styles/brand-verbatim.css`,
  `src/components/icons/VerbatimLogo.tsx`.

Verbatim also has a simpler window, after Superwhisper: sidebar Home,
Meetings, Vocabulary | Settings, Sound, Models; descriptions under each
option instead of info icons; no footer bar; no menu bar icon (the meeting
pill shows recording); first start is permissions, then the model. Options
not shown keep their defaults and stay in the config file
(`src/components/settings/verbatim/`). The Vocabulary list feeds only the
meeting correction: Handy's direct dictation word swap turned „Lied“ into
„Lead“ and „Mach“ into „Noxi“ with these words.

Both use the same bundle identifier, settings and meetings; installing one
replaces the other in /Applications. Icons are drawn as SVG in
`src-tauri/brands/verbatim/src/`; after a change: `rsvg-convert` to PNG, then
`bun run tauri icon src-tauri/brands/verbatim/src/icon-1024.png -o src-tauri/brands/verbatim/icons`.

## What it does

1. **Dictation** – unchanged Handy.
2. **Meeting recording** – shortcut ⌥⇧M, tray item, the Meetings page, or
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
4. **Processing** after the meeting (measured: a 21-minute call is ready
   ~3 minutes after stopping):
   1. decode, align the two tracks;
   2. speakers per track: Sortformer on the GPU in a child process
      (`meeting/diarize_worker.rs`, ~20 s per 20 min of audio; CPU fallback);
   3. drop mic turns that are acoustic echo of the system track;
   4. transcribe each speaker turn (Parakeet, Handy's engine, ~20 s);
   5. text dedup, merge ghost speakers, names: on an online call the mic
      voice is you; on a single mic nobody is named; remembered voices by
      their embedding;
   6. dictionary correction with the local LLM (`correct.rs`, ~70 s): only
      lines with a word that sounds like a dictionary entry are sent; the
      model reports misheard dictionary words; code checks each change;
   7. summary, decisions, tasks with the local LLM (Ollama, `qwen3.5:9b`,
      ~50 s; the model is loaded during step 2 and unloaded 1 min after);
   8. Markdown note in the notes folder; corrections listed at the end.
5. **Speakers** – rename "Speaker 2" in the app; with *Remember voice* the
   voice is recognised in later meetings (sherpa-onnx embeddings).
7. **Language model** – correction (4.6) and summary (4.7). Chosen under
   Models → *Language model*, a list like the speech models: Qwen 3.5 9B on
   this Mac (needs Ollama; the page offers to install, start it and download
   the model), or Claude / GPT through Anthropic, OpenAI or OpenRouter with
   an API key (the transcript then goes to that provider; the key is shared
   with dictation post-processing). No temperature is sent: every model runs
   at its own default. Other models can be set in the config file
   (`meetings.llm_provider`, `llm_model`, `cloud_model`). Request shapes are
   covered by tests against a fake server (`summary.rs`); a real-request
   check: `MEETING_CLOUD=anthropic MEETING_CLOUD_MODEL=claude-opus-5-5 MEETING_CLOUD_KEY=… cargo test -p meeting-core cloud_summary_smoke -- --ignored --nocapture`.
8. **Safety** – a memory guard stops the app above 6 GB while a meeting is
   processed (`meeting/memguard.rs`); recordings are closed at quit and
   processed at the next launch.

## Layout

| Path | What |
|---|---|
| `crates/meeting-core` | Platform-independent pipeline: types, pipeline, acoustic echo filter, text dedup, LLM summary (Ollama / OpenAI-compatible, map-reduce for long meetings), Markdown notes, speaker rename |
| `crates/diarize` | Diarization: Sortformer (default) and sherpa-onnx backends, speaker embeddings, voice profiles |
| `crates/meeting-detect` | Which processes use the mic (Core Audio), meeting-app matching, debounce watcher |
| `crates/meeting-e2e` | Headless end-to-end harness: real models on synthetic fixtures, scored against ground truth; examples for real-call evals (`raw_transcript`, `correct_eval`, `summary_eval`, `diarize_once`) |
| `testdata/generate.py` | Builds the synthetic fixtures (macOS voices + ffmpeg) into `testdata/audio/` (gitignored) |
| `testdata/real/` | Real-call eval data (gitignored, private): uncorrected transcripts, hand labels (34 misheard words, 37 words that must stay), dictionary |
| `scripts/correction_eval.py` | Scores correction outputs (and other speech models' transcripts) against `testdata/real` labels |
| `src-tauri/src/meeting/` | App side: capture (`mic.rs`, `system_tap.rs`, `flac.rs`), `process.rs`, `store.rs` (meeting index), `detect.rs` |
| `src-tauri/src/commands/meeting.rs` | Tauri commands and the `MeetingChanged` event |
| `src/components/settings/meetings/` | Meetings page |

Data on disk:
- meeting audio + index: `~/Library/Application Support/dev.handymeetings.app/meetings/`
- voice profiles: `…/dev.handymeetings.app/voice-profiles.json`
- diarization models: `…/dev.handymeetings.app/meeting-models/` (downloaded on first use)
- notes: the folder set on the Meetings page (default `~/Documents/Handy Meetings`)

## Tests

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" CMAKE_POLICY_VERSION_MINIMUM=3.5
python3 -I testdata/generate.py                      # once: synthetic fixtures

(cd crates/meeting-core   && CARGO_TARGET_DIR=/tmp/hm-target-core   cargo test)
(cd crates/meeting-detect && CARGO_TARGET_DIR=/tmp/hm-target-detect cargo test)
(cd crates/diarize        && CARGO_TARGET_DIR=/tmp/hm-target-diarize cargo test --release)
(cd crates/meeting-e2e    && CARGO_TARGET_DIR=/tmp/hm-target-e2e    cargo test --release -- --nocapture --test-threads=1)

# with live summaries from local Ollama
(cd crates/meeting-e2e && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-e2e cargo test --release -- --nocapture --test-threads=1)
(cd crates/meeting-core && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-core cargo test --test live_llm -- --nocapture --test-threads=1)
```

The e2e harness needs `~/.cache/handy-meetings/models/parakeet-tdt-0.6b-v3-Q8_0.gguf`
(see `crates/meeting-e2e/src/lib.rs`). It prints, per scenario, word error
rate, speakers found, speaker attribution and leftover echoes, and writes the
notes to `$TMPDIR/hm-e2e-notes/`.

To add a real recording as a test case: put `mic.wav` (and `system.wav`)
as 16 kHz mono WAV plus a `truth.json` (see an existing scenario) into a new
folder under `testdata/audio/`, and add its name to the scenario lists in
`crates/meeting-e2e/tests/pipeline_e2e.rs`.

### Real-call evaluation (correction)

```sh
# uncorrected transcript of a recording (same pipeline as the app)
(cd crates/meeting-e2e && cargo run --release --example raw_transcript -- mic.wav system.wav ../../testdata/real/<call>/raw.json)
# app's correction on all calls, then score: fixed / broken / other changes
(cd crates/meeting-e2e && cargo run --release --example correct_eval -- qwen3.5:9b)   # CORRECT_TEMP=0.2 to compare
python3 -I scripts/correction_eval.py --score-only out_rust.json
```

Count only first runs for timing: Ollama reuses cached prompt parts on repeats.

## Building and installing

```sh
scripts/build-and-install.sh          # build, then install when no meeting is busy
scripts/build-and-install.sh --no-build
```

The script signs with the code-signing certificate **"Handy Meetings Dev"**
from the login keychain when it exists, so macOS keeps the granted
permissions across installs. Create it once (Keychain Access → Certificate
Assistant → Create a Certificate…; Name "Handy Meetings Dev", Identity Type
"Self-Signed Root", Certificate Type "Code Signing"). Without it, builds are
signed ad hoc and every install asks for all permissions again.

Manual build:

```sh
bun install
bun run tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
# → src-tauri/target/release/bundle/macos/Handy Meetings.app
```

Needs Xcode Command Line Tools 26.x (Swift for the system-audio tap). With
Command Line Tools only, Apple Intelligence is built as a stub; meeting
summaries use Ollama instead.

## Keeping up with Handy

```sh
git fetch upstream
git merge upstream/main     # or a release tag
```

The meeting code lives mostly in new files, so conflicts should be rare
(expect them in `lib.rs`, `settings.rs`, `tray.rs`, `bindings.ts` and the
translation files).
