# Research: models, prompts and measurements

These are the measurements behind Sokki's choices of language model, prompt, live mode and runtime. Read them before you change any of those, and add to them when you measure something new. Decisions were reverted or questioned later when the reasons were not written down (for example, why the paste uses a second transcription: see "Open questions").

## Files

| File | What it covers |
|---|---|
| `dictation-formatting-and-cleanup-2026-11.md` | Dictation layout with Qwen3.5 (0.8B–9B), voting, live chunking, forward-pass timing. Then word cleanup with Gemma 4 E4B, the model comparison, latency in MLX and in Sokki's llama-server (plain, n-gram, MTP), and the shipped setup rated blind. |
| `meeting-summaries-2026-11.md` | Meeting summaries: Qwen3.5 4B and Gemma 4 E4B against Sokki's Qwen3.5 9B. Speed, memory, two blind rating rounds. |
| `muesli-and-cleanup-log-2026-11.md` | Running log of the Müsli tests (Sokki's own 0.8B/2B cleanup models) and the later Qwen/Gemma rounds. |
| `phone-dictation-latency-2026-11.md` | Live dictation on the phone: Parakeet Ultra vs Nemotron Streaming on the Pixel 7, dictating through the Mac, newer phones' CPU. |
| `data/results.json`, `data/forward_pass_timing.json` | Layout rounds and read/write speed per model. |
| `data/cleanup_ratings.json` | Every blind rating of the word-cleanup rounds, per rater, version and dictation id (scores and error counts, no text). |
| `data/cleanup_latency.json` | Wait after the speaker stops, per setup and dictation id; llama-server read/write speed and MTP acceptance. |
| `data/summary_runs_and_ratings.json` | Every summary run (time, tokens, counts) and the second blind rating, per model and meeting. |
| `android-phone-transcript-2026-11.md` | Meeting transcript on the phone without a Mac (Pixel 7): 30 minutes in 12.6 minutes, about 7 % battery, 3 % word errors; stops while the app is not on screen. Led to: no meeting transcripts on the phone (Robin, 26 Nov 2026). |
| `live-vs-final-2026-11.md` | Live text vs. pasted transcript on 34 real dictations, with Whisper large-v3 and Canary 1B v2 as tie-breakers; the live dictation cleanup in four rounds (what it cleans while you speak, redo, wait). |
| `word-learning-2026-11.md` | The word list and word learning: what the swap fixes and breaks, the language model as word judge, how other dictation apps learn words, the typed-text journal tests. |
| `data/live_vs_final.json` | Per dictation and round: word differences, distances to both references, cleanup steps, redo and wait. |
| `live-pieces-and-summary-preload-2026-11.md` | The live panel as one growing text: live pieces of 20, 30 and 60 words on the same 34 dictations (wait, quality, how much raw text shows). The summary model loaded during the meeting (memory, the macOS memory warning, time saved) and how long reading a transcript takes. |
| `data/live_pieces.json` | Per dictation and piece size: live steps, wait, raw words shown, words changed. |
| `instant-paste-2026-11.md` | The dictation in the field at once, its raw end cleaned in place: how it decides it may edit, the replay of 34 dictations (time to the first paste and to the edit), the night lab in real text fields. |
| `data/instant_paste.json` | Per dictation: seconds to the first paste and to the edit, words already right. |
| `donation-ask-2026-11.md` | Wording of the donation ask on the milestone card: studies, A/B tests and real numbers from app tip jars. |
| `monetisation-2026-11.md` | How comparable dictation and meeting apps and solo open-source makers earn money, grants, and ranked options for Sokki. |
| `income-ai-era-2026-11.md` | Second round: how Handy got sponsors, Porzio perks, Obsidian, VoiceInk, team features without a cloud, new ranking. |
| `mac-autocorrect-2026-11.md` | Mac autocorrect in Chrome and Electron apps: how Sokki reaches and writes their text fields, the lab test with Chrome in front, CPU and memory with autocorrect on and off, and the fix for German capitals in English sentences. |
| `speaker-names-call-apps-2026-11.md` | Speaker names: voice grouping across 46 meetings, what Zoom and Meet show through Accessibility, why Meet needs captions on, the law on stored voices. |
| `cloudflare-kv-usage-2026-11.md` | Cloudflare's free plan: which KV operations the counter and website use per day, why 26.11. neared the delete cap (the move to R2), launch-day estimate, Workers Paid or not. |
| `second-brain-2026-11.md` | A built-in second brain (Karpathy's LLM wiki): the tools, why people quit, a local Mac stack (FTS5 + sqlite-vec, EmbeddingGemma), small-model limits, a phased plan. Research only, nothing built. |
| `ios-keyboard-dictation-2026-11.md` | What a dev podcast says about dictation on iOS: the keyboard microphone workaround (getUserMedia web view), who copied it, and what users still miss. |

## Decisions these measurements support (Nov 2026)

- **Dictation cleanup:** Gemma 4 E4B (Q4_K_M) with prompt v4, cleaned live in 60-word pieces with the last paragraph left open, in Sokki's llama-server with MTP speculative decoding. Robin approved the target (fix words and layout, keep meaning) and the setup on 24 Nov 2026.
- **Meeting summaries:** Gemma 4 E4B replaces Qwen3.5 9B. Robin approved this on 25 Nov 2026 after two blind rounds.
- **Live pieces:** 30 words while speaking, 60 at the stop (26 Nov 2026): twice as many dictations show cleaned text live, a shorter wait, the same result (`live-pieces-and-summary-preload-2026-11.md`).
- **Summary model:** loaded a minute into a meeting when a quarter of memory stays free after it (26 Nov 2026).
- **Live cleanup input:** the committed live text plus the sentences two previews agree on; a few changed words (up to 3 %) keep the cleaned part, bigger changes go back one step. Measured on 34 real dictations (`live-vs-final-2026-11.md`).
- **Meetings on Android:** the phone records, only a Mac transcribes. Robin decided on 26 Nov 2026 after the Pixel 7 run (`android-phone-transcript-2026-11.md`): "meeting transcription never happens on the phone - just too battery intensive". No streaming to the Mac either: sending the file after the recording is enough.
- **Paste:** stays the second transcription of the whole recording (more accurate, 0.3 s).
- **Sampling:** each model's own card settings. Exception: Qwen's card presence penalty (1.5) made it paraphrase freely, so it is off.
- **Meet speaker names:** Sokki turns Meet's captions on while recording; nothing else shows the speaker reliably from outside the browser (research 26 Nov 2026, `speaker-names-call-apps-2026-11.md`).
- **Word learning:** only words typed key by key; the swap only for learned words of 6+ letters; a word must be capitalized inside a sentence once; "Keep what I write" and word learning on by default (Robin, 25–26 Nov 2026).
- **Memory:** a heavy job may use up to 6.4 GB (Robin's limit, `~/PycharmProjects/CLAUDE.md`).

## Open questions

- **Live transcript vs. second transcription: answered 26 Nov 2026** (`live-vs-final-2026-11.md`). On 34 real dictations the two differ in 0.7–0.9 % of words, and two reference models side with the pasted (second) transcription more often. The second pass costs a median of 0.32 s. It stays.
- **Phone:** dictation through the Mac (`phone-dictation-latency-2026-11.md`) is so far measured with the Mac playing the phone; the Pixel run over home Wi-Fi is still to do.

## How to add an entry

Keep it short. Each entry gives the date, the question or the task the agent was given, the result in numbers, and the sources (links, or the script and data file). Without the date nobody can tell later whether it is out of date (Robin, 26 Nov 2026).

## Data policy

- The dictations, meeting transcripts and hand-written ideal corrections are Robin's private data. They are archived outside the repo, with the test scripts; ask Robin for access.
- Files here hold only numbers, model names and dictation ids. Keep it that way: no transcript text, names or quotes.
