# Meeting summaries with Qwen3.5-4B instead of the 9B

24 Nov 2026, on Robin's Mac (M1 Pro, 32 GB). Numbers only. The meetings, transcripts and summaries are private and are not in this repo.

## Short version

- **Is the 4B good enough? Not yet.** Its summaries scored lower: 2.75 out of 5 for both 4B versions, against 3.19 for the 9B.
- **The 4B fails more often, and worse.** It produced the single worst summary in the test: on a 45-minute meeting it summarised only the small talk. It also wrote decisions in English inside a German note.
- **It is faster and smaller:**
  - About 2× faster: 20–24 s per meeting against 42 s for the 9B.
  - Less memory: 3.6 GB (4-bit) or 5.2 GB (8-bit), against 6.2–6.4 GB for the 9B.
- **The test is small.** With 4 meetings × 2 tries per model, results from the same model scatter widely: the 9B ranged from 2.0 to 4.5, and the 4B 4-bit from 1.5 to 4.0. The 0.4-point gap is a signal, not proof.
- **All models make the R16 errors**: the wrong person or side gets a task, and topics get mixed up. Even the 9B gave the central responsibility to the wrong side in one of two tries. A switch to the 4B would not fix R16, and its extra misses would add to it.

## Setup

- **Same runtime and settings as Sokki.** Every model ran with:
  - Sokki's own bundled `llama-server` (`/Applications/Sokki.app/.../llm/llama-server`).
  - Sokki's exact start flags: context 16,384; `--jinja --reasoning off`; Qwen's sampling: temperature 1.0, top-k 20, top-p 0.95, min-p 0, presence penalty 1.5, repeat penalty 1.0; `--cache-ram 0`.
- **Same request code.** A small bench outside the repo calls `meeting_core::summary::LlmSummarizer::summarize_with_stats`, configured as `llm::config` configures the built-in provider: OpenAI-compatible endpoint, 16k context, no sampling values sent. The dictionary (glossary) and user name come from Sokki's settings, as `process.rs` passes them.
- **Models**, all GGUF files from unsloth:

  | Model | File size | Used where |
  |---|---|---|
  | Qwen3.5-9B Q4_K_M | 5.7 GB | Sokki's built-in model |
  | Qwen3.5-4B Q4_K_M | 2.7 GB | test |
  | Qwen3.5-4B Q8_0 | 4.5 GB | test |

- **Meetings:** 4 real meetings from Robin's Sokki, in German and English, each run twice per model:

  | Length | Transcript size | Language | Type |
  |---|---|---|---|
  | 11 min | ~2,800 tokens | English | in the room |
  | 13 min | ~4,100 tokens | German | online |
  | 29 min | ~7,300 tokens | German | online |
  | 45 min | ~8,300 tokens | English | online |

  Every request was one pass; no meeting needed splitting into parts.
- **Rating:** blind.
  - A script shuffled the 6 summaries of each meeting under letters A–F. The key was opened only after all 24 summaries were rated.
  - Each summary was scored 1–5 against the full transcript, for topics covered, decisions, and tasks with owners. Wrong facts and mix-ups (R16 style) were counted.
  - **Limit:** there was one rater, not two. The test ran as a sub-task that could not start its own rater agents, and the rater also ran the experiment.
- **Meeting safety:** a guard checked `meetings/index.json` every 5 s and stopped the model server when a meeting started recording. Robin recorded 4 meetings during the test, so the server was stopped 4 times and the cut-off runs were repeated. Each model ran alone.

## Quality (blind, 8 summaries per model)

| Model | Mean score | Range | Wrong facts | Mix-ups (owner, side, topic) |
|---|---|---|---|---|
| 9B Q4_K_M (Sokki today) | **3.19** | 2.0–4.5 | 10 | 6 |
| 4B Q4_K_M | 2.75 | 1.5–4.0 | 11 | 4 |
| 4B Q8_0 | 2.75 | 2.0–3.5 | 12 | 10 |

Per meeting, mean of the two tries:

| Meeting | 9B | 4B Q4 | 4B Q8 |
|---|---|---|---|
| 11 min, English | 3.0 | 3.0 | 2.75 |
| 13 min, German | 3.25 | 3.0 | 2.5 |
| 29 min, German | 2.5 | 3.0 | 2.25 |
| 45 min, English | **4.0** | **2.0** | 3.5 |

What went wrong, by type (counts over 8 summaries each):

| Failure | 9B | 4B Q4 | 4B Q8 |
|---|---|---|---|
| Missed the meeting's main content (only small talk) | 0 | 1 | 0 |
| Central responsibility given to the wrong side | 1 | 0 | 2 |
| Task with wrong owner or recipient | 1 | 1 | 2 |
| Invented task | 0 | 2 | 1 |
| Invented names or details | 4 | 0 | 1 |
| Plan stated as done | 1 | 0 | 0 |
| Wrong language in part of the note | 0 | 1 | 0 |
| No tasks or decisions extracted at all | 2 | 0 | 2 |

- The 4B Q4's best tries were good, best of 6 in two meetings. Its worst were the worst in the test.
- The 9B was the most reliable on the longest meeting.

## Speed and memory

| Model | Time per summary, median (range) | Reading | Writing | Peak memory of the model server |
|---|---|---|---|---|
| 9B Q4_K_M | 42 s (23–73 s) | ~220 tokens/s | ~19.5 tokens/s | 6.2–6.4 GB |
| 4B Q4_K_M | 24 s (14–30 s) | ~380 tokens/s | ~31 tokens/s | 3.6 GB |
| 4B Q8_0 | 20 s (11–37 s) | ~450 tokens/s | ~29 tokens/s | 5.2 GB |

- **Model loading:** 5.2 s for the 9B, 3–4 s for the 4B.
- **Memory** is the peak resident memory of `llama-server`, sampled every second, model file pages included.
- **Reading** (prompt processing) dominates for long meetings. A 45-minute meeting takes about 19 s of reading on the 4B Q8 and about 38 s on the 9B.
- **Q8 vs Q4 on the 4B:** in llama.cpp on Metal, Q8_0 reads faster than Q4_K_M, because Q4_K_M has to be unpacked first. That is the reverse of MLX's dictation measurements, where 4-bit and 8-bit read equally fast.

## What this means

- Swapping the 9B for the 4B would halve the summary wait and save 1–3 GB, but summaries would get less reliable. The extra failures are the worst kind: a meeting summarised as small talk, invented tasks.
- If memory or speed ever forces it, the 4B Q4_K_M is the better 4B: same quality as Q8, 1.6 GB smaller, and fewer mix-ups in this sample.
- Not tested:
  - A summary prompt tuned for the 4B. The current prompt was written and tuned on the 9B.
  - More meetings and a second rater.
  - The 4B as a fallback on machines with less memory, where the 9B does not fit at all.

## Gemma 4 E4B (25 Nov 2026)

### Short version

- **Gemma 4 E4B did at least as well as the 9B in this test.** It scored 3.44 against the 9B's 3.19, with 4 wrong facts against 10, and 2 mix-ups against 6.
- **It is about 2× faster and smaller:**
  - 22 s median per summary, against 42 s for the 9B.
  - 4.7 GB peak memory, against 6.2–6.4 GB.
- **It was steadier than the 9B.** All 8 Gemma summaries scored 3.0–4.0. The 9B ranged from 2.0 to 4.5.
- **It did not make the worst errors:**
  - It always gave the central responsibility to the right side. The 9B got this wrong once.
  - It never covered only small talk.
  - It never invented a task.
- **What breaks:**
  - It pads the decision list with points that are not decisions ("was discussed", "is promising").
  - In one case it put placeholders ("[the person being referred to]") where it should have used a name or left the item out.
  - It still misassigns details. Twice in one meeting it gave a detail to the wrong side: who handles leads, and whose call funnel optimisation is.
- **The rating is weaker than for the 4B.** See the limits below. Treat this as "good enough to try in Sokki", not as proof that it is better.

### Setup

- Same 4 meetings, 2 summaries per meeting, Sokki's bundled `llama-server` and the same summary code, as above.
- **Model:** `gemma-4-E4B-it-Q4_K_M.gguf`, 5.0 GB file.
- **Start flags:** Sokki's, with Gemma's sampling from its model card in place of Qwen's:
  - temperature 1.0, top-p 0.95, top-k 64;
  - min-p 0, because the card names no min-p and llama-server's own default would be 0.05;
  - no presence penalty, repeat penalty 1.0;
  - `--reasoning off`. No thinking text appeared in any output.
- **How it ran:** one detached `build-slot` job at low priority. It got the slot after 44 minutes and took 185 s for all 8 summaries. No meeting interrupted it, and memory stayed below the 6.4 GB limit.

### Quality

| Model | Mean score | Range | Wrong facts | Mix-ups |
|---|---|---|---|---|
| 9B Q4_K_M (Sokki today) | 3.19 | 2.0–4.5 | 10 | 6 |
| Gemma 4 E4B Q4_K_M | **3.44** | 3.0–4.0 | 4 | 2 |
| 4B Q4_K_M (for reference) | 2.75 | 1.5–4.0 | 11 | 4 |

Per meeting, mean of the two tries:

| Meeting | 9B | Gemma 4 E4B |
|---|---|---|
| 11 min, English | 3.0 | 3.0 |
| 13 min, German | 3.25 | 3.5 |
| 29 min, German | 2.5 | 3.5 |
| 45 min, English | 4.0 | 3.75 |

**Limits of this rating:**
- **One rater again.** The test ran as a sub-task that could not start rater agents, so there was no second independent rater.
- **The 9B scores come from the first round**, where the rater did not know which model wrote what. This time the Gemma summaries were shuffled together with the 9B summaries. The rater recognised the 9B texts from the first round, so the Gemma summaries were not rated fully blind.
- **The 9B was not re-rated.** Its scores were not adjusted for any shift in standards between the two rounds.

### Speed and memory

| Model | Time per summary, median (range) | Reading | Writing | Peak memory | Model loading |
|---|---|---|---|---|---|
| 9B Q4_K_M | 42 s (23–73 s) | ~220 tokens/s | ~19.5 tokens/s | 6.2–6.4 GB | 5.2 s |
| Gemma 4 E4B Q4_K_M | 22 s (12–33 s) | ~590 tokens/s | ~33 tokens/s | 4.7 GB | 3.1 s |

Gemma reads about 2.7× and writes about 1.7× as fast as the 9B. Its summaries came out longer: 222–551 written tokens, against 126–445 for the 4B.

### Not tested yet

- A second, independent rater.
- More meetings: the long-meeting result was 3.75 against 4.0 on one meeting only.
- Gemma's own recommended prompt format.
- How Gemma handles meetings that need splitting into parts. None of these 4 needed it.
- Gemma's licence terms for shipping it inside Sokki.

## Second rating, fully blind, two raters (25 Nov)

Same 8 Gemma 4 E4B and 8 Qwen3.5 9B summaries (4 meetings × 2 runs), mixed and unlabelled, rated by two new independent raters against the transcripts.

| | Qwen3.5 9B (Sokki today) | Gemma 4 E4B |
|---|---|---|
| Score (1–5), rater A / B | 2.38 / 2.25 | **3.25 / 3.25** |
| Lowest score | 2 | 3 |
| Wrong facts, A / B | 12 / 24 | **7 / 12** |
| Mix-ups, A / B | 9 / 6 | **3 / 3** |
| Missing topics or tasks, A / B | 27 / 32 | 27 / 36 |

- Gemma scored higher in every one of the 4 meetings, for both raters.
- Both models leave out a lot; Gemma about as much as the 9B.
- Hard for raters: garbled transcript in one meeting, speaker labels that shift between people in two meetings.
