# Third-party notices

Verbatim is a fork of **[Handy](https://github.com/cjpais/Handy)** by CJ Pais,
released under the MIT License. Handy's copyright notice is kept in
[`LICENSE`](LICENSE), which covers Handy's code and Verbatim's additions.

## Code and ideas from other projects

| Project | What Verbatim uses | License |
|---|---|---|
| [Handy](https://github.com/cjpais/Handy) by CJ Pais | The whole dictation app this fork is built on | MIT, © 2025 CJ Pais |
| [Mandy](https://github.com/yuri-research-ai/Mandy) | Meeting recording: microphone plus system audio as two tracks (`src-tauri/src/meeting/`) | MIT, © 2025 CJ Pais (Handy fork) |
| [Lark](https://github.com/bkoleo/lark) by bkoleo | The approach for detecting meeting apps (`crates/meeting-detect`) and the idea for removing microphone bleed (`crates/meeting-core/src/dedup.rs`) | MIT, © 2025 CJ Pais (Handy fork) |

Both forks ship Handy's MIT License unchanged; its full text is in [`LICENSE`](LICENSE).

## Models

Models are not part of this repository. Verbatim downloads them on first use
from their publishers, and each keeps its own license, shown on its model
card:

- Speech recognition: NVIDIA Parakeet and other models from Handy's catalog
  (Models page in the app)
- Speaker detection: NVIDIA Sortformer (streaming, 4 speakers)
- Summaries on your Mac: Qwen 3.5 via [Ollama](https://ollama.com)

Optional cloud summaries (Anthropic, OpenAI, OpenRouter) are subject to each
provider's terms.

## Libraries

The Rust crates and npm packages Verbatim depends on are listed in
`Cargo.lock` and `bun.lock`, each under its own license.
