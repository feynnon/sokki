# Research: models, prompts and measurements

These are the measurements behind Sokki's choices of language model, prompt, live mode and runtime. Read them before you change any of those, and add to them when you measure something new. Decisions were reverted or questioned later when the reasons were not written down (for example, why the paste uses a second transcription: see "Open questions").

## Files

| File | What it covers |
|---|---|
| `dictation-formatting-and-cleanup-2026-11.md` | Dictation layout with Qwen3.5 (0.8B–9B), voting, live chunking, forward-pass timing. Then word cleanup with Gemma 4 E4B, the model comparison, latency in MLX and in Sokki's llama-server (plain, n-gram, MTP), and the shipped setup rated blind. |
| `meeting-summaries-2026-11.md` | Meeting summaries: Qwen3.5 4B and Gemma 4 E4B against Sokki's Qwen3.5 9B. Speed, memory, two blind rating rounds. |
| `muesli-and-cleanup-log-2026-11.md` | Running log of the Müsli tests (Sokki's own 0.8B/2B cleanup models) and the later Qwen/Gemma rounds. |
| `data/results.json`, `data/forward_pass_timing.json` | Layout rounds and read/write speed per model. |
| `data/cleanup_ratings.json` | Every blind rating of the word-cleanup rounds, per rater, version and dictation id (scores and error counts, no text). |
| `data/cleanup_latency.json` | Wait after the speaker stops, per setup and dictation id; llama-server read/write speed and MTP acceptance. |
| `data/summary_runs_and_ratings.json` | Every summary run (time, tokens, counts) and the second blind rating, per model and meeting. |
| `android-phone-transcript-2026-11.md` | Meeting transcript on the phone without a Mac (Pixel 7): 30 minutes in 12.6 minutes, about 7 % battery, 3 % word errors; stops while the app is not on screen. |
| `live-vs-final-2026-11.md` | Live text vs. pasted transcript on 34 real dictations, with Whisper large-v3 and Canary 1B v2 as tie-breakers; the live dictation cleanup in four rounds (what it cleans while you speak, redo, wait). |
| `word-learning-2026-11.md` | The word list and word learning: what the swap fixes and breaks, the language model as word judge, how other dictation apps learn words, the typed-text journal tests. |
| `data/live_vs_final.json` | Per dictation and round: word differences, distances to both references, cleanup steps, redo and wait. |
| `donation-ask-2026-11.md` | Wording of the donation ask on the milestone card: studies, A/B tests and real numbers from app tip jars. |
| `monetisation-2026-11.md` | How comparable dictation and meeting apps and solo open-source makers earn money, grants, and ranked options for Sokki. |
| `mac-autocorrect-2026-11.md` | Mac autocorrect in Chrome and Electron apps: how Sokki reaches and writes their text fields, the lab test with Chrome in front, CPU and memory with autocorrect on and off, and the fix for German capitals in English sentences. |
| `speaker-names-call-apps-2026-11.md` | Speaker names: voice grouping across 46 meetings, what Zoom and Meet show through Accessibility, why Meet needs captions on, the law on stored voices. |

## Decisions these measurements support (Nov 2026)

- **Dictation cleanup:** Gemma 4 E4B (Q4_K_M) with prompt v4, cleaned live in 60-word pieces with the last paragraph left open, in Sokki's llama-server with MTP speculative decoding. Robin approved the target (fix words and layout, keep meaning) and the setup on 24 Nov 2026.
- **Meeting summaries:** Gemma 4 E4B replaces Qwen3.5 9B. Robin approved this on 25 Nov 2026 after two blind rounds.
- **Live cleanup input:** the committed live text plus the sentences two previews agree on; a few changed words (up to 3 %) keep the cleaned part, bigger changes go back one step. Measured on 34 real dictations (`live-vs-final-2026-11.md`).
- **Paste:** stays the second transcription of the whole recording (more accurate, 0.3 s).
- **Sampling:** each model's own card settings. Exception: Qwen's card presence penalty (1.5) made it paraphrase freely, so it is off.
- **Meet speaker names:** Sokki turns Meet's captions on while recording; nothing else shows the speaker reliably from outside the browser (research 26 Nov 2026, `speaker-names-call-apps-2026-11.md`).
- **Word learning:** only words typed key by key; the swap only for learned words of 6+ letters; "Keep what I write" on by default, learning off until checked on real writing (Robin, 25–26 Nov 2026).
- **Memory:** a heavy job may use up to 6.4 GB (Robin's limit, `~/PycharmProjects/CLAUDE.md`).

## Open questions

- **Live transcript vs. second transcription: answered 26 Nov 2026** (`live-vs-final-2026-11.md`). On 34 real dictations the two differ in 0.7–0.9 % of words, and two reference models side with the pasted (second) transcription more often. The second pass costs a median of 0.32 s. It stays.
- **Phone:** nothing here was measured on Android.

## How to add an entry

Keep it short. Each entry gives the date, the question or the task the agent was given, the result in numbers, and the sources (links, or the script and data file). Without the date nobody can tell later whether it is out of date (Robin, 26 Nov 2026).

## Data policy

- The dictations, meeting transcripts and hand-written ideal corrections are Robin's private data. They are archived outside the repo, with the test scripts; ask Robin for access.
- Files here hold only numbers, model names and dictation ids. Keep it that way: no transcript text, names or quotes.
