<p align="center">
  <img src="docs/assets/hero.png" alt="Verbatim: meeting notes and dictation that stay on your Mac" width="100%">
</p>

<p align="center">
  <a href="https://github.com/feynnon/verbatim/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/feynnon/verbatim?style=flat-square&color=3567cc&label=release"></a>
  <img alt="macOS 14+ on Apple Silicon" src="https://img.shields.io/badge/macOS-14%2B%20·%20Apple%20Silicon-17181b?style=flat-square&logo=apple&logoColor=white">
  <img alt="Runs locally" src="https://img.shields.io/badge/runs-100%25%20on%20your%20Mac-2c7a55?style=flat-square">
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-8ab4ff?style=flat-square"></a>
</p>

<p align="center">
  <b>Record a call. Get a note with who said what, a summary, decisions and tasks.</b><br>
  Speech recognition, speaker detection and the summary run on your Mac. No account, no cloud, no bot joining your call.
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#what-you-get">What you get</a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#privacy">Privacy</a> ·
  <a href="#build-from-source">Build</a>
</p>

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/feynnon/verbatim/main/install.sh | bash
```

The script downloads the latest release, puts **Verbatim.app** in `/Applications` and opens it. Run it again to update; your settings and meetings stay.

On first start Verbatim asks for two permissions (microphone, and accessibility to type dictation into other apps) and lets you pick a speech model. For calls, macOS asks once for **System Audio Recording**, so the other side of the call is recorded too.

For summaries on your Mac, install [Ollama](https://ollama.com/download) (free). Verbatim starts it and downloads the model for you under **Models → Language model**. If you prefer, pick Claude or GPT there with your own API key instead.

<sub>Requires macOS 14.2 or later on Apple Silicon (M1 or newer). Releases are not notarized by Apple: the script installs without the browser's quarantine flag, so macOS opens the app. If you download the zip by hand, macOS blocks the first start: open System Settings → Privacy & Security and click Open Anyway.</sub>

## What you get

<p align="center"><img src="docs/assets/settings.png" alt="Verbatim settings window: a short list of options, Superwhisper-style" width="720"></p>

| | |
|---|---|
| **Records any call** | Zoom, Teams, Google Meet, Slack, FaceTime, or the room you sit in. Microphone and system audio are recorded as two tracks, so you and the others never blur. |
| **Notices meetings** | When a call app starts using the microphone, Verbatim asks whether to record. It never records on its own. |
| **Knows who spoke** | Speaker detection on the Apple GPU. On a call your microphone is you; name the others once and their voices are recognised next time. |
| **Writes the note** | Title, summary, decisions and tasks with an owner, plus the full timestamped transcript, as a Markdown file in any folder, for example your Obsidian vault. |
| **Gets names right** | Add customers, colleagues and products to the Vocabulary. Misheard versions are corrected, and only when the language model and a sound check agree. |
| **Dictation too** | Hold a shortcut, speak, and the text appears where your cursor is. That part is [Handy](https://github.com/cjpais/Handy), the app Verbatim is built on. |

A 20-minute call is ready about three minutes after you hang up, on an M-series Mac.

### An example note

```markdown
# Launch plan for the spring campaign

## Summary
- Anna and Ben agree to move the landing page launch to Monday.
- The ad budget stays at 2,000 € for the first two weeks.
- Ben asks whether the customer interviews can feed the new ad copy.

## Decisions
- Launch on Monday instead of Friday.

## Tasks
- [ ] **Ben**: send the final copy to the customer by Friday
- [ ] **Robin**: set up the first two ad variants

## Transcript
**[00:00] Anna:** Can we still make Friday, or should we move the launch?
**[00:06] Ben:** I'd rather move it to Monday, the copy isn't approved yet.
```

## How it works

```mermaid
flowchart LR
  A[Microphone + system audio] --> B[Speaker detection<br/>Sortformer, GPU]
  B --> C[Transcription<br/>Parakeet]
  C --> D[Clean-up<br/>echo, duplicates, names]
  D --> E[Vocabulary correction<br/>language model]
  E --> F[Summary, decisions, tasks<br/>language model]
  F --> G[Markdown note]
```

| Step | Default | Options |
|---|---|---|
| Speech recognition | NVIDIA Parakeet (25 European languages, fast) | Whisper, Canary, Qwen-ASR and more on the Models page |
| Speakers | NVIDIA Sortformer on the GPU, CPU fallback | Remembered voices per person |
| Summary and correction | Qwen 3.5 9B via Ollama, on your Mac | Claude, GPT or OpenRouter with your own key |

Each model is downloaded once and then runs offline. The measurements behind these choices, including what was tried and rejected, are in [KNOWN_ISSUES.md](KNOWN_ISSUES.md).

## Your voice (optional)

Speech models are weakest exactly where meetings happen: spontaneous talk, mixed languages, jargon. If you opt in, Verbatim keeps **your own voice** with its transcript: your dictations, and in calls only what you said into your microphone. Never other people's voices. It all stays on your Mac, and the page shows how many hours you have per language.

The idea: bundled with other users' data and anonymised (names and numbers removed), it could one day be sold to teams that train speech models, with a share going to you. There's no buyer yet. Nothing leaves your Mac unless you agree to a sale. The counter of how much voice data Verbatim users hold together runs on a tiny service in [`counter/`](counter/).

## Privacy

- Audio, transcripts and notes stay on your Mac. Recordings are deleted after the period you choose.
- The only network requests are model downloads, and, only if you pick a cloud model, the transcript sent to that provider for the summary.
- No account, no telemetry, no update checks.
- If you switch on **Your voice** (off by default), Verbatim keeps your own voice with its transcript on your Mac and shares only how many hours you have per language, under a random number.
- Recording people needs their consent: tell everyone on the call.

## Build from source

You need macOS on Apple Silicon, [Rust](https://rustup.rs), [Bun](https://bun.sh) and the Xcode Command Line Tools.

```bash
git clone https://github.com/feynnon/verbatim.git
cd verbatim
bun install
scripts/build-and-install.sh            # builds Verbatim and installs it to /Applications
BRAND=handy scripts/build-and-install.sh  # the same app with Handy's look
```

[MEETINGS.md](MEETINGS.md) explains the meeting pipeline, the code layout and the test harness; [CONFIG.md](CONFIG.md) lists every setting. The meeting code lives in `crates/` (platform-independent Rust) and `src-tauri/src/meeting/`.

## Credits

Verbatim is a fork of **[Handy](https://github.com/cjpais/Handy)** by CJ Pais, which does the dictation and the model management. Meeting recording comes from [Mandy](https://github.com/yuri-research-ai/Mandy), meeting detection follows [Lark](https://github.com/bkoleo/lark). Models by NVIDIA (Parakeet, Sortformer) and Alibaba (Qwen), run with [ggml](https://github.com/ggml-org/ggml) and [Ollama](https://ollama.com). Details in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## License

[MIT](LICENSE) © 2025 CJ Pais, © 2026 Robin Dehde.
