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
   macOS: window, menu, sokki CLI   Omarchy: shell plugin + sokki CLI   Android: app + keyboard
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
| `crates/sokki-slack` | Slack huddle notes: sign-in with OAuth and PKCE, the huddle's message, posting the summary and the transcript into its thread. |
| `crates/meeting-mail` | Notes to everyone in a meeting: calendar events and which one a recording belongs to, recipients, the objection list, the mail text, and sending through Apple Mail (AppleScript) or Gmail (`gmail.send`). The calendar itself is read by the host (macOS: EventKit, `src-tauri/swift/calendar.swift`). |
| `crates/meeting-chat` | The chat notice (macOS): find a meeting's chat field through Accessibility, post one line, watch for "stop". |
| `crates/sokki-engine` | `process::run`: one recorded meeting in, one note out, with the platform's transcriber and diarizer passed in. `lifecycle`: the meeting state machine. `llm`: language-model settings, status, the built-in models (llama.cpp's `llama-server`, started for one summary and stopped after it), Ollama and model download. `cleanup`: cleaning up a dictation while it is spoken. `hooks`: the person's own commands on events, with their queue ([HOOKS.md](HOOKS.md)). |
| `crates/sokki` | The `sokki` command line, on macOS and Linux (CLI.md). On Linux also the host, in the same binary: `sokki daemon` (systemd user service) and `sokki process` (the model worker). On the Mac it is a client of the app: the socket API, the app's headless transcription, the index. |
| `crates/sokki-api` | The local API both hosts serve: requests, replies, events, the socket's path. The specification. |
| `crates/sokki-index` | `index.sqlite`: keyword search (FTS5 unicode61 + trigram) over meetings, own notes and dictations, derived from the files and rebuildable; tasks and decisions with evidence checked against the transcript; write-back for hooks. No models, no platform code. |
| `crates/sokki-link` | Phone ↔ computer link over iroh: pairing, sending a recording, getting the note back. |
| `crates/meeting-e2e` | Headless harness: real speech recognition on synthetic fixtures (`testdata/audio`), with word error rate and speaker metrics. |
| `src-tauri` | The macOS app (the desktop app inherited from upstream). `src/meeting/` is the macOS host: recording, prompts, the indicator, and the adapter around `sokki_engine::process`. `src/local_api.rs` serves the socket API; `src/local_index.rs` keeps the index current. |
| `src` | The Tauri window (React). |
| `mobile` | Android: Tauri mobile app plus a HeliBoard keyboard (GPL-3.0; everything in `crates/` stays MIT). |
| `packaging/omarchy` | Omarchy integration: the shell plugin (QML), the systemd unit, `omarchy-*` scripts, the PKGBUILD and the one-line installer. |

## Invariants

These hold everywhere; a change that breaks one is a bug.

- **Never record without a yes.** Detection only reports. Recording starts
  from a person's action: the prompt, a shortcut, a menu item, a CLI call.
  The prompt's timeout means no.
- **Audio stays on the device.** The only network traffic: model downloads,
  the language model when the person picks a cloud provider, the phone
  link, Gmail when the person connects it to send notes, and the notes
  (summary and transcript text, never audio) posted to Slack after the
  person connected it. Each lives in one named place (`diarize::models`,
  `meeting_core::summary`, `sokki-link`, `crates/sokki/src/models.rs`,
  `meeting_mail::gmail`, `sokki-slack`). A hook the person wrote can send
  anything anywhere; Sokki itself sends nothing for it ([HOOKS.md](HOOKS.md)).
- **A meeting outranks everything.** No background work (a hook, a
  transcription from the CLI, the dictation cleanup) starts while a meeting
  records or is processed; a hook already running is paused until it is over.
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
  request. The built-in model gets its publisher's defaults (Gemma 4's
  model card) when its server starts, because its GGUF file carries none.

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

## The local API (macOS and Linux)

One API on both platforms, so one `sokki` command line and every script or
hook works on both. On Linux `sokki daemon` listens on
`$XDG_RUNTIME_DIR/sokki/sokki.sock`; on the Mac the app listens on
`~/Library/Application Support/org.sokki.app/sokki.sock` (`local_api.rs`,
only for this user). One JSON object per line in each direction. A client
sends a request and reads one reply; `{"cmd":"subscribe"}` instead keeps the
connection open and streams events (the whole state, never a diff). The CLI
is a thin client of this API, and so is the Omarchy shell plugin (it runs
`sokki events`). Requests and events are defined in
`crates/sokki-api/src/lib.rs`; that file is the specification. On the Mac
every request calls what the menu bar or the app's flags already call.
Subscribers also get each hook event (`sokki_engine::hooks::HookEvent`,
without content).

Searching is not part of the API: `sokki search`, `get`, `meetings`,
`tasks` read `index.sqlite` directly (after bringing it up to date), so they
work while Sokki is closed. They show nothing until the person turns on
`agent_access`.

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
  clash. The model is Gemma 4 E4B (Q4_K_M with its MTP draft model) since
  25.11.2026; it replaced Qwen 3.5 9B after two fully blind ratings on 4
  real meetings (3.25 of 5 against 2.38/2.25, about half the wrong facts,
  22 s instead of 42 s; 5.6 GB resident with the draft model, against 6.4
  GB). Summaries and dictation
  cleanup share its files, so it is one download; the 9B's file is deleted
  once Gemma is there. Ollama and cloud providers stay options.
- **The summary model loads during the meeting.** A minute into a
  recording, `meeting/preload.rs` loads the built-in model when a quarter of
  the Mac's memory stays free after it (`llm::preload`, throttled disk
  reads; macOS's background band would also slow the summary, and it can't
  be lifted later). It waits idle and the summary takes it (`llm::open`);
  processing stops it if no summary does. The transcript itself is read only
  after the stop: speaker names and the list of speakers, which open the
  prompt, are known only then (KNOWN_ISSUES S14).
- **Dictations cleaned while they are spoken.** `cleanup` cuts the
  transcript's settled sentences into pieces of about 30 words while it is
  spoken (60 at the stop)
  and cleans each one with the built-in model in its own `llama-server` (MTP
  speculative decoding: about 53 tokens/s on an M1 Pro), keeping the last
  paragraph open for the next piece; at the stop only the rest is left
  (about 1 s for 30–60 words, 1.6–4.6 s for 140–484). The server stays up
  between dictations and stops after 2 minutes without one. It never runs
  next to a meeting: not while one records or is processed, not while a
  local summary model is open (it is stopped when one of those begins).
  A model that fails or takes over 20 s leaves the transcript as it was.
  The live panel shows one growing text: the cleaned part, then the newest
  words raw on the same line (dimmed), replaced in place once cleaned; with
  the cleanup on it is six lines tall instead of three
  (`overlay::set_tall_live`). At the stop the text goes into the field at
  once: the paragraphs cleaned live plus the rest as transcribed
  (`cleanup::Draft`). When the last step is done, only the end that changed
  is selected through Accessibility and pasted over (`instant_paste`,
  `sokki_autocorrect::tail`), and only while the field still ends with
  exactly the draft at the cursor and no key was pressed. Where Sokki can't
  read the field, it pastes once, cleaned, as before. Between the paste and
  the edit the panel shrinks to its pill.
  On by default since 26.11.2026 (turned on once for existing installs,
  settings schema 10); without the model (8 GB Macs, or not downloaded
  yet) it does nothing and the transcript is pasted as before.
- **Keyword search first, one derived index.** Agents found facts in
  memory as well or better with keyword search, filters, stable ids and
  cited turns than with vectors (`docs/research/memory-layer-2026-11.md`).
  So `index.sqlite` has no embeddings yet: FTS5 whole words plus trigrams
  (German compounds; `memory-index-recall-2026-11.md`), fed from the notes,
  the meeting folders and `history.db`, which stay the truth. Any row can be
  rebuilt; deletes remove every row of a source with `secure_delete`. The
  index is in the app's data folder, not the notes folder, so a synced
  notes folder never carries it. Vectors come after a private eval shows a
  gain.
- **The Mac gets the Linux CLI, not its own.** The bash `scripts/sokki`
  had three commands and no events; the Rust binary gives both platforms
  the same commands. The app bundles it (`resources/bin/sokki`, built by
  `scripts/build-sokki-cli.sh`, signed by `release.sh`) and `install.sh`
  links it into `~/.local/bin`.
- **Hooks bring the use cases.** Sokki provides facts and events (a note
  is ready, a dictation is done, a schedule is due); what happens then is
  the person's own code in `~/.config/sokki/hooks.toml`, run by one runner
  in `sokki-engine` that both hosts share. The contract is git's and Claude
  Code's: JSON on stdin, environment variables, an exit code. A hook is
  free code: whatever is in hooks.toml runs, no approval, so an agent can
  check one in (Robin, 27.11.2026: "A hook can do anything - why do any
  gating at all?"). The one guard is the platform's: on macOS hooks start
  with responsibility disclaimed, so they never get Sokki's microphone,
  Accessibility or Screen Recording grants (`hooks/spawn.rs`). `scope`
  optionally narrows what goes on stdin. The queue lives in SQLite so a
  quit loses nothing. Robin's plan of 26.11.2026: Sokki as a platform, LLM
  workflows on one's own data.
- **Echo cancellation, not voice merging.** WebRTC AEC3 on the mic track
  (with the call track as reference) removed the extra "speaker" the call
  audio created on the mic. Merging similar voices exists but is off until a
  real recording needs it.
