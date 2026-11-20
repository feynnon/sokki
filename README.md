<p align="center">
  <img src="docs/assets/hero.png" alt="Sokki. Every word. Yours." width="100%">
</p>

<p align="center">
  <a href="https://github.com/feynnon/sokki/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/feynnon/sokki?style=flat-square&color=3567cc&label=release"></a>
  <img alt="macOS 14.2+ on Apple silicon" src="https://img.shields.io/badge/macOS-14.2%2B%20·%20Apple%20silicon-17181b?style=flat-square&logo=apple&logoColor=white">
  <a href="#license"><img alt="Open source" src="https://img.shields.io/badge/open%20source-MIT-8ab4ff?style=flat-square"></a>
</p>

<h3 align="center">Dictation and meeting notes. Local. Free forever. Open source.</h3>

<p align="center"><a href="https://sokki.org">sokki.org</a> · <sub><b>Sokki</b> (速記) is Japanese for shorthand.</sub></p>

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/feynnon/sokki/main/install.sh | bash
```

Run it again to update. Settings and meetings stay. macOS 14.2 or later, Apple silicon.

Summaries run on your Mac, free. Sokki downloads the model under **Models → Language model**. Or pick Claude or GPT there with your own key.

## Speak anywhere.

Hold a key, talk, let go. The text lands at your cursor, in any app. 25 languages.

## Asks before it records.

When Zoom, Teams, Meet, Slack or FaceTime start a call, Sokki asks once. No bot joins.

- **Two tracks.** Your microphone and the call, recorded apart.
- **Who said what.** Speakers detected on the Apple GPU. Name a voice once.
- **The note.** Summary, decisions, tasks with an owner, full transcript. Markdown, in any folder.
- **Names right.** Add people and products to the Vocabulary.

A 20 min call is ready about 3 min after you hang up, on an M-series Mac.

<p align="center"><img src="docs/assets/settings.png" alt="Sokki settings window" width="720"></p>

```markdown
# Launch plan for the spring campaign

## Decisions
- Launch moves to Monday.

## Tasks
- [ ] **Ben**: final copy by Friday
- [ ] **You**: two ad variants

## Transcript
**[00:00] Anna:** Can we still make Friday?
**[00:06] Ben:** I'd rather move it to Monday.
```

## How it works

| Step | Default | Options |
|---|---|---|
| Echo removal | WebRTC echo cancellation | — |
| Speakers | NVIDIA Sortformer, GPU | Remembered voices |
| Speech recognition | NVIDIA Parakeet, 25 languages | Whisper, Canary, Qwen-ASR and more |
| Summary | Qwen 3.5 9B, built in, on your Mac | Ollama; Claude, GPT or OpenRouter with your key |

Models download once, then run offline. Measurements behind these choices: [KNOWN_ISSUES.md](KNOWN_ISSUES.md).

## Stays on your Mac.

- Audio, transcripts and notes stay on your Mac. Meeting audio is deleted after the period you choose.
- Leaves only: model downloads, an update check, the transcript if you pick a cloud model, and a small counter message a few times a day.
- No account, no usage reports unless you turn them on.
- Recording people needs their consent. Tell everyone on the call.

**Your data.** Your own recordings stay on your Mac until you delete them. Three choices, all off: **Help improve Sokki** sends a daily usage report under a random ID; **Ask me when there's a buyer** tells you when someone wants data like yours, and nothing is sent unless you answer; **Share all my recordings** uploads your recordings and their transcripts unchanged, for Sokki to use, share or sell. Details: [sokki.org/privacy](https://sokki.org/privacy/) and [`counter/`](counter/).

## Build from source

You need macOS on Apple Silicon, [Rust](https://rustup.rs), [Bun](https://bun.sh) and the Xcode Command Line Tools.

```bash
git clone https://github.com/feynnon/sokki.git
cd sokki
bun install
scripts/build-and-install.sh            # builds Sokki and installs it to /Applications
BRAND=handy scripts/build-and-install.sh  # the same app with Handy's look
```

[MEETINGS.md](MEETINGS.md) explains the meeting pipeline, the code layout and the test harness; [CONFIG.md](CONFIG.md) lists every setting. The meeting code lives in `crates/` (platform-independent Rust) and `src-tauri/src/meeting/`.

## Credits

Sokki is a fork of **[Handy](https://github.com/cjpais/Handy)** by CJ Pais, which does the dictation and the model management. Meeting recording comes from [Mandy](https://github.com/yuri-research-ai/Mandy), meeting detection follows [Lark](https://github.com/bkoleo/lark). Models by NVIDIA (Parakeet, Sortformer) and Alibaba (Qwen), run with [ggml](https://github.com/ggml-org/ggml) and [llama.cpp](https://github.com/ggml-org/llama.cpp). Details in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## License

- **Mac app and shared code** (everything outside `mobile/`): [MIT](LICENSE) © 2025 CJ Pais, © 2026 Robin Dehde.
- **Android app** (`mobile/`): [GPL-3.0](mobile/LICENSE), because its keyboard is built on [HeliBoard](https://github.com/HeliBorg/HeliBoard). The shared Rust crates it uses (`crates/`) stay MIT.
