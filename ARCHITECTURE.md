# Architecture

Sokki is dictation plus meeting notes, local first, on three platforms:
macOS (a Tauri app), Linux with Omarchy (a background service with a shell
plugin) and Android (a Tauri mobile app with a keyboard). This file is the
map: where things live, how they connect, and the rules the code keeps.
The user-facing overview is the [README](README.md); the meeting pipeline in
detail is [MEETINGS.md](MEETINGS.md).

## Bird's eye view

```
 Front ends (thin, native to each platform)
   macOS: Tauri window + menu bar   Omarchy: shell plugin + sokki CLI   Android: app + keyboard
          │ in process                    │ local socket, JSON lines         │ sokki-link (iroh)
 ─────────┴───────────────────────────────┴─────────────────────────────────┴──────────
 Hosts: own the platform, run the engine
   src-tauri (macOS)               crates/sokki (Linux: `sokki daemon`)      the Mac, for the phone
 ──────────────────────────────────────────────────────────────────────────────────────
 sokki-engine: turns two recorded tracks into a note (one function), the
   meeting lifecycle as a pure state machine, language-model plumbing
 ──────────────────────────────────────────────────────────────────────────────────────
 Pure core (no platform code)
   meeting-core: echo cancellation, pipeline, dedup, summary, note
   diarize: Sortformer speakers, voice embeddings, voice profiles
   meeting-detect: which app holds the mic, debounced into Started / Ended
 ──────────────────────────────────────────────────────────────────────────────────────
 Platform adapters (compiled only on their own OS)
   macOS: Core Audio process tap (system audio), Core Audio mic clients
   Linux: PipeWire (pw-record, pw-dump --monitor), Hyprland event socket, wtype
 Model workers: separate processes, so a crash or a memory spike never takes
   the app or a running recording down
```

A meeting, on any platform:

1. **Detect.** An app opens the microphone. `meeting-detect` matches it
   against known call apps and debounces it into `Started`.
2. **Ask.** The host asks the person whether to record. No answer means no
   recording. Sokki never records on its own.
3. **Record.** Two tracks: the microphone, and what the call plays (the
   other people). Kept apart so echo can be removed and "me" is known.
4. **End.** The call app lets go of the microphone; after a grace period the
   recording stops by itself.
5. **Process** (`sokki_engine::process`): align tracks → echo cancellation →
   speakers per track → transcribe each turn → dedup and naming → summary by
   a language model → Markdown note.

## Code map

| Path | What it is |
|---|---|
| `crates/meeting-core` | The platform-independent pipeline. Defines the ports `Diarizer`, `Transcriber`, `Summarizer`, `SpeakerNamer`. No I/O except the language-model HTTP client in `summary.rs`. |
| `crates/diarize` | `Diarizer` implementations (Sortformer via transcribe.cpp; sherpa-onnx), voice embeddings, `VoiceProfiles`, model downloads. |
| `crates/meeting-detect` | Mic-client sources (`macos.rs`, `linux.rs`), the app table (`apps.rs`), the pure debounce `Detector` and the `Watcher` thread. |
| `crates/sokki-engine` | `process::run`: one recorded meeting in, one note out, with the platform's transcriber and diarizer passed in. `lifecycle`: the meeting state machine. `llm`: language-model settings, status, the built-in model (llama.cpp's `llama-server`, started for one summary and stopped after it), Ollama and model download. |
| `crates/sokki` | The Linux host. One binary: `sokki daemon` (systemd user service), the CLI (`sokki meeting …`, `sokki dictation …`, `sokki events`), and `sokki process` (the model worker). |
| `crates/sokki-link` | Phone ↔ computer link over iroh: pairing, sending a recording, getting the note back. |
| `crates/meeting-e2e` | Headless harness: real speech recognition on synthetic fixtures (`testdata/audio`), with word error rate and speaker metrics. |
| `src-tauri` | The macOS app (the desktop app inherited from upstream). `src/meeting/` is the macOS host: recording, prompts, the indicator, and the adapter around `sokki_engine::process`. |
| `src` | The Tauri window (React). |
| `mobile` | Android: Tauri mobile app plus a HeliBoard keyboard (GPL-3.0; everything in `crates/` stays MIT). |
| `packaging/omarchy` | Omarchy integration: the shell plugin (QML), the systemd unit, `omarchy-*` scripts, the PKGBUILD and the one-line installer. |

## Invariants

These hold everywhere; a change that breaks one is a bug.

- **Never record without a yes.** Detection only reports. Recording starts
  from a person's action: the prompt, a shortcut, a menu item, a CLI call.
  The prompt's timeout means no.
- **Audio stays on the device.** The only network traffic: model downloads,
  the language model when the person picks a cloud provider, and the phone
  link. Each lives in one named place (`diarize::models`,
  `meeting_core::summary`, `sokki-link`, `crates/sokki/src/models.rs`).
- **The core has no platform code.** No Core Audio, PipeWire or Hyprland
  types in `meeting-core`, `diarize`, `sokki-engine`. Platform code sits in
  files compiled only on that platform.
- **Plain files.** Notes are Markdown in a folder the person picks. Meeting
  data is JSON and FLAC in one folder per meeting. Settings are one file the
  app reloads when it changes.
- **As few rules as possible.** A threshold or heuristic is added only when
  a real recording needs it. Code for a rule not yet proven stays in,
  switched off, with a note (`DiarizeConfig::merge_similarity`).
- **The language model's default temperature.** Sokki never sets one in a
  request. The built-in model gets its publisher's defaults (Qwen's, as in
  Ollama) when its server starts, because its GGUF file carries none.

## Platforms

| | macOS | Linux (Omarchy) | Android |
|---|---|---|---|
| Host | `src-tauri` | `crates/sokki` | `mobile/src-tauri` |
| UI | Tauri window, recording pill | Omarchy shell plugin, notifications, CLI | Tauri mobile, keyboard |
| Mic | cpal | `pw-record` | Android audio |
| Call audio | Core Audio process tap (from Mandy) | `pw-record` on the default output's monitor (only the call app's stream: possible, off) | — |
| Call detection | Core Audio mic clients (from Lark) | `pw-dump --monitor` (events) + Hyprland windows for browser calls | later |
| Prompt | macOS dialog | Omarchy shell panel, else a notification with actions | — |
| Typing | Accessibility | `wtype`, else clipboard | keyboard |
| Speech models | transcribe.cpp, Metal | transcribe.cpp, Vulkan + CPU | transcribe.cpp, CPU |
| Settings | `settings_store.json` | `~/.config/sokki/sokki.toml` | app storage |
| Data | `~/Library/Application Support/org.sokki.app` | `~/.local/share/sokki` | app storage |

## The local API (Linux)

`sokki daemon` listens on `$XDG_RUNTIME_DIR/sokki/sokki.sock`. One JSON
object per line in each direction. A client sends a request and reads one
reply; `{"cmd":"subscribe"}` instead keeps the connection open and streams
events. The CLI is a thin client of this API, and so is the shell plugin
(it runs `sokki events`). Requests and events are defined in
`crates/sokki/src/api.rs`; that file is the specification.

## Testing

- **Pure parts, by unit tests with fakes:** the detector, the meeting
  lifecycle, the PipeWire and Hyprland parsers (recorded JSON fixtures), the
  pipeline steps.
- **The pipeline, end to end:** `crates/meeting-e2e` runs the real models on
  synthetic recordings and scores word error rate and speakers. Real
  recordings stay private and never enter the repository.
- **The Linux host:** a build in an Arch Linux container
  (`packaging/docker`, aarch64 and x86_64) with a simulated Zoom call
  (`e2e.sh`), then the real thing in an Omarchy VM
  (`packaging/omarchy/vm-test`: a Chromium web-app call, the shell plugin,
  dictation, all on virtual audio devices).

## Decisions

Short records of choices that are not obvious from the code.

- **One engine, thin hosts.** Mullvad, Tailscale, 1Password and Signal all
  ship one core with a native face per platform; Sokki does the same. The
  macOS host stays a Tauri app; Omarchy gets a service and a shell plugin,
  because Omarchy 4 has no room for app windows: its bar, menus,
  notifications and panels are all plugins of one shell.
- **Shell tools before bindings on Linux.** `pw-record`, `pw-dump` and
  `wtype` ship with Omarchy, are stable, and keep the binary free of C
  bindings. A native PipeWire client can replace them behind the same
  functions if they ever fall short.
- **Everything the computer plays, not just the call app.** Recording only
  the call app's stream keeps music out, but browsers replace their audio
  stream mid-call; in the Omarchy VM the recording then fell back to the
  microphone and the transcript turned to nonsense. The default output's
  monitor never goes away. The per-app path stays in, switched off
  (`call_app_only`).
- **Models in a worker process.** Processing a meeting runs in
  `sokki process`, a child of the service, so a crash or memory spike in a
  model never stops a recording or dictation.
- **The language model built in, not Ollama.** Ollama is a separate app:
  on 2026-11-20 it was off when a meeting needed it, and its model stays
  loaded after a summary (keep_alive). Sokki now ships llama.cpp's
  `llama-server` (one static binary, Metal, `scripts/build-llama-server.sh`)
  and runs it in its own process for exactly one summary (`llm::open`);
  dropping the handle stops it, and a shell watchdog stops it when Sokki
  dies. A server process instead of linking llama.cpp: the app already
  links ggml through transcribe.cpp, and two ggml copies in one binary
  clash. Same model (Qwen 3.5 9B, Q4_K_M), same prompts, measured as fast
  as Ollama (KNOWN_ISSUES.md F8). Ollama and cloud providers stay options.
- **Echo cancellation, not voice merging.** WebRTC AEC3 on the mic track
  (with the call track as reference) removed the extra "speaker" the call
  audio created on the mic. Merging similar voices exists but is off until a
  real recording needs it.
