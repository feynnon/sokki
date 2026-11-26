# Third-party notices

Sokki is a fork of **[Handy](https://github.com/cjpais/Handy)** by CJ Pais,
released under the MIT License. Handy's copyright notice is kept in
[`LICENSE`](LICENSE), which covers Handy's code and Sokki's additions.

## Code and ideas from other projects

| Project | What Sokki uses | License |
|---|---|---|
| [Handy](https://github.com/cjpais/Handy) by CJ Pais | The whole dictation app this fork is built on | MIT, © 2025 CJ Pais |
| [Mandy](https://github.com/yuri-research-ai/Mandy) | Meeting recording: microphone plus system audio as two tracks (`src-tauri/src/meeting/`) | MIT, © 2025 CJ Pais (Handy fork) |
| [Lark](https://github.com/bkoleo/lark) by bkoleo | The approach for detecting meeting apps (`crates/meeting-detect`) and the idea for removing microphone bleed (`crates/meeting-core/src/dedup.rs`) | MIT, © 2025 CJ Pais (Handy fork) |

Both forks ship Handy's MIT License unchanged; its full text is in [`LICENSE`](LICENSE).

## Vendored crates

Three crates from the Handy project live in this repository under new names,
so Sokki no longer depends on them from crates.io. Each keeps its license
file unchanged.

| Crate here | Origin | Changes | License |
|---|---|---|---|
| `crates/sokki-keys` | [handy-keys](https://github.com/handy-computer/handy-keys) 0.3.5 | Renamed; `to_handy_string` is `to_shortcut_string`; log and thread names | MIT, © 2026 handy-computer |
| `crates/sokki-recorder` | [handy-recorder](https://github.com/handy-computer/handy-recorder) 0.1.1 | Renamed; uses `crates/sokki-cpal`; log and thread names | MIT, © 2026 handy-computer |
| `crates/sokki-cpal` | [handy-cpal](https://github.com/handy-computer/cpal) 0.18.2, a fork of [cpal](https://github.com/RustAudio/cpal) | Package and library renamed (`Cargo.toml`), README replaced; code unchanged | Apache-2.0, © the cpal authors |

## Models

Models are not part of this repository. Sokki downloads them on first use
from their publishers, and each keeps its own license, shown on its model
card:

- Speech recognition: NVIDIA Parakeet and other models from Handy's catalog
  (Models page in the app)
- Speaker detection: NVIDIA Sortformer (streaming, 4 speakers)
- Summaries and dictation cleanup on your Mac: Gemma 4 E4B (Apache-2.0,
  as stated on Google's model card google/gemma-4-E4B-it) by Google
  DeepMind, with its MTP draft model, as GGUF files quantized by Unsloth;
  optionally through [Ollama](https://ollama.com)

Optional cloud summaries (Anthropic, OpenAI, OpenRouter) are subject to each
provider's terms.

The sample meeting's voices (`src-tauri/resources/sample-meeting/call.flac`)
were spoken by [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M)
(Apache-2.0) from an invented script (`testdata/sample_meeting/`).

## Libraries

- [sonora](https://github.com/dignifiedquire/sonora) (BSD-3-Clause), a
  pure-Rust port of WebRTC's audio processing, for echo cancellation of the
  microphone track.
- [llama.cpp](https://github.com/ggml-org/llama.cpp) (MIT), whose
  `llama-server` ships inside the app and runs the summary and dictation
  cleanup models on the GPU.

The Rust crates and npm packages Sokki depends on are listed in
`Cargo.lock` and `bun.lock`, each under its own license.

## sonora license

```
Copyright (c) 2011, The WebRTC Project Authors. All rights reserved.
Copyright (c) 2016, Arun Raghavan and contributors. All rights reserved.
Copyright (c) 2026, dignifiedquire. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

  * Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.

  * Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in
    the documentation and/or other materials provided with the
    distribution.

  * Neither the name of Google nor the names of its contributors may
    be used to endorse or promote products derived from this software
    without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## llama.cpp license

```
MIT License

Copyright (c) 2023-2026 The ggml authors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
