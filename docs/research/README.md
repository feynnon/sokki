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

## Decisions these measurements support (Nov 2026)

- **Dictation cleanup:** Gemma 4 E4B (Q4_K_M) with prompt v4, cleaned live in 60-word pieces with the last paragraph left open, in Sokki's llama-server with MTP speculative decoding. Robin approved the target (fix words and layout, keep meaning) and the setup on 24 Nov 2026.
- **Meeting summaries:** Gemma 4 E4B replaces Qwen3.5 9B. Robin approved this on 25 Nov 2026 after two blind rounds.
- **Sampling:** each model's own card settings. Exception: Qwen's card presence penalty (1.5) made it paraphrase freely, so it is off.
- **Memory:** a heavy job may use up to 6.4 GB (Robin's limit, `~/PycharmProjects/CLAUDE.md`).

## Open questions

- **Live transcript vs. second transcription.** The paste uses a second transcription of the whole recording, because the live text differed by 3.4–5.6 % of words on a 90 s German dictation (English 34 s: 0 %, German 13 s: 0.9 %; Sokki Board card "Live-Text am Mac"). Nobody has measured which version is more correct. A test is planned; when it lands, add it here.
- **Phone:** nothing here was measured on Android.

## Data policy

- The dictations, meeting transcripts and hand-written ideal corrections are Robin's private data. They are archived outside the repo, with the test scripts; ask Robin for access.
- Files here hold only numbers, model names and dictation ids. Keep it that way: no transcript text, names or quotes.
