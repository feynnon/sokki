# Dictation formatting with Qwen: findings

23–24 Nov 2026. Measured on Robin's Mac (Apple M1 Pro, 32 GB) with mlx-lm 0.32.2, on 38 of Robin's own dictations (Mac and phone, 30–484 words). The texts are private and not in this repo. Aggregated numbers are in `data/` (no dictation text).

**What "formatting" means here:** Qwen rewrites the dictation with paragraphs and bullet lists. Code then maps only the *breaks* back onto the original words, so no word changes. Thinking is off. Sampling is temperature 1.0, top-k 20, top-p 1.0.

**How quality was rated:** separate AI raters rated every version blind, 1–5 per layout. They also marked "bad" when a break is clearly wrong (not just taste). Two independent raters were used per round from round 7 on.

## Short version

- **Best quality:** Qwen3.5-4B 8-bit, with breaks voted between three tries. Over the 38 texts it scored a mean of 3.53 against 3.33–3.45 for one try, with half the clearly wrong layouts. This replicated in three rounds with two raters each.
- **The 9B is not better with this prompt.** Qwen3.5-9B 4-bit scored 3.13 against 3.33 for the 4B, with 20 against 16 bad layouts. It bullets relative clauses and sentence ends more often, and voting does not help it. The prompt and its examples were tuned on the 4B.
- **One rule mattered more than model size:** a list item may start only after `. ! ? : ; ,` or at und/oder/aber/dann (and/or/but/then). Lists of one item are dropped. That halved the bad layouts of every model.
- **Formatting while you speak** (live, 60-word pieces, last 2 paragraphs left open) keeps quality. It cuts the wait after you stop for long texts from up to 23 s to 5–9 s.
- **What doesn't help:**
  - prompt lookup (speculative decoding from the input);
  - re-formatting a growing 3-piece window;
  - "rewrite only one piece, neighbours as context" (focus);
  - batching 3 tries "for free" (it costs 2×).

## Models (one full pass, 38 texts)

| Model | Quality | Bad | Wait, median | 200+ words | Peak memory |
|---|---|---|---|---|---|
| 0.8B 8-bit | 2.87 | 4 (mostly does nothing) | 1.1 s | 5.4 s | 1.8 GB |
| 2B 8-bit | 3.16 | 9 | 2.2 s | 9.3 s | 2.9 GB |
| 4B 4-bit | 3.32 | 7 | 3.5 s | 14.4 s | 3.5 GB |
| 4B 8-bit | 3.53 | 6 | 4.7 s (3.4 s with prompt cache) | 19 s | 5.3 GB |
| 9B 4-bit | 3.13 (round 9; 4B scored 3.33 there) | 10 (4B: 8) | 3.9 s with prompt cache | 20 s | 6.1 GB |

The quality column comes from round 2, except for the 9B, which comes from round 9. Mean scores move by about ±0.2 between rounds and raters, so compare within a round.

**Chance yardstick:** at temperature 1.0, a second try of the same 4B differs from the first about as much as the 4B 4-bit or the 2B differ from it.

## What a call costs (forward-pass timing)

Synthetic tokens, reading done in 512-token portions as in generation (`data/forward_pass_timing.json`).

| Model | Reading (prefill) | Writing (one output token) | Peak |
|---|---|---|---|
| 0.8B 8-bit | ~1,480 tokens/s | 8.7 ms | 2.0 GB |
| 2B 8-bit | ~660 tokens/s | 16 ms | 3.2 GB |
| 4B 4-bit | ~300 tokens/s | 19.5 ms | 3.8 GB |
| 4B 8-bit | ~300 tokens/s | 31.8 ms | 5.7 GB |
| 9B 4-bit | ~165 tokens/s | 31.8 ms | 6.3 GB |

- **Reading** is limited by compute, so it scales with model size: 4-bit and 8-bit read equally fast.
- **Writing** is limited by memory bandwidth, so it scales with the size of the weights. That is why the 9B 4-bit writes as fast as the 4B 8-bit.
- **Neither slows down with context up to 4,000 tokens.** Qwen3.5 uses full attention in only 1 of 4 layers.
- **Fitted on real formatting calls** (system prompt cached): the 4B 8-bit costs 0.25 s + 3.1 ms per input token + 31.5 ms per output token (R² 0.99). The 9B costs 0.37 s + 5.0 ms + 31.5 ms. One written token costs about 10× (4B) or 6× (9B) a read token, so output length decides latency.
- **Prompt cache:** computing the fixed system prompt (387 tokens) once and copying its cache saves 0.5–1.4 s per call, with identical output.

## Live while speaking

Waits below are replayed at Robin's measured speaking pace (2.8 words/s), with steps queued on one GPU. They are for texts of 100+ words.

| Approach (4B 8-bit) | Wait, median | 200+ words | Max | Quality vs one full pass |
|---|---|---|---|---|
| Full pass at the end, prompt cache | 5.8 s | 15–23 s | 23 s | – |
| Live, 40-word pieces, 1 paragraph open | 4.6 s | 4.8 s | 5.4 s | worse (bad 11 vs 7) |
| **Live, 60-word pieces, 2 paragraphs open** | 5.4 s | 8.1 s | 9.1 s | same |
| Growing 3-piece window (fixed only at paragraph ends) | 8.1 s | 14.4 s | 18.8 s | same, not better |
| Fixed 3-piece window, each piece voted from its 3 positions | 3.8 s (all texts) | 7.8 s | 8.8 s | slightly worse |
| Focus: rewrite 1 piece, neighbours as context | 5.4 s (all texts) | 7.5–14 s | 40 s | worse on long texts (1.9–2.4 vs 3.2) |
| Vote: 2 live tries + window as tie-breaker | 15–21 s | – | 42–64 s | **best** (long texts 3.75–3.80, 0–1 bad) |
| Batched vote: 3 tries per live step in one batch, last step one try | 5.9–6.5 s | – | 11.5–20 s | same as one try (3.15 vs 3.05; vote as built 3.6, round 10) |

On the 9B, live with 60-word pieces waits 5.8 s median (200+ words: 9.8 s, max 11.6 s).

- **Focus loses** because each piece is formatted nearly alone. Lists across piece boundaries fall apart, and fragments become bullets. One call ignored "output only this part" and wrote 850 words for a 53-word piece (48 s). It also writes 16 % more tokens per text than the window, because each target repeats the sentence before it.
- **Voting wins** because errors at temperature 1.0 are mostly random: one try makes the slip, the other doesn't. It loses a few good lists that only one try found.
- **Voting is too slow live as built:** three live streams need more GPU time than speaking takes, so a queue builds up. Batching 3 tries takes 1.9–2.1× one call (120 words: 7.4 s → 13.9 s), not ~1.3×. Untested cheaper setups:
  - vote only on text fixed while speaking: tested (row "Batched vote"). Fast, but it lost the gain, likely because the end of the text gets one try and the three voters share one prompt;
  - vote the last step too (expected ~10–12 s wait): not tested;
  - vote only from 100 words (short texts gain nothing).
- **Live bug, fixed by one clean-up pass:** a fixed paragraph could keep a colon when the list after it dissolved later, and a paragraph could start in lower case at a step boundary. Re-applying the breaks to the finished text removes both.

## Contention and safety

- **Transcription slows while Qwen works.** While a Qwen 4B step runs, Parakeet (`sokki transcribe`) takes about 1.9× as long: 14 s of audio 0.30 → 0.54 s, 64 s of audio 1.40 → 2.74 s. Qwen is about 15 % slower while transcription runs. Live, Qwen is busy 15–40 % of the speaking time.
- **Prompt lookup is slower:** 7.6 s vs 4.7 s median. At temperature 1.0 only 21 % of drafted tokens are accepted, and the hybrid cache cannot be rolled back, so every rejection costs another pass. A first version also never stopped, because a drafted end token was always rejected.
- **Sokki's own 9B** (Qwen3.5-9B-Q4_K_M via llama-server, 16k context) uses about 6.4 GB: 5.7 GB model file, 512 MiB KV cache, 50 MiB recurrent state, 153 MiB compute. It writes about 20 tokens/s. This is the memory limit for heavy test jobs on Robin's Mac (see `~/PycharmProjects/CLAUDE.md`).
- **A meeting lost its audio.** On 24 Nov at 10:02, 16 headless transcriptions plus a Qwen loop ran during a meeting, and that meeting lost its call audio. Heavy jobs now wait while a meeting is recording or processing.

## Not tested

- The phone: everything above is Mac only.
- A prompt tuned for the 9B.
- Sokki's GGUF 9B for formatting quality (only its memory and speed).
- A vote that is both live-fast and as good as the vote as built.

## Cleanup (words + layout) with Gemma 4, 24 Nov evening

Robin approved a new target: fix the words too (fillers, repetitions, self-corrections, misheard words, broken grammar), keep meaning, order and tone, paragraphs and lists (he prefers more bullets to fewer). 15 hand-written ideal corrections (8 phone, 7 Mac) are the test set; texts are private, not in this repo.

Blind rating by two AI raters against the ideals (1–5; meaning errors = changed, dropped or added content, rater 1 / rater 2):

| Setup | Overall | Meaning errors | Wait after you stop |
|---|---|---|---|
| Gemma 4 E4B 4-bit, prompt v4, whole text at the end | 3.0 | 9 / 7 (round 1), 13 / 14 (round 2) | 2–5 s up to 100 words, 8–9 s at 180, 27 s at 484 |
| Gemma 4 E4B, live 60-word pieces, last paragraph open | 3.0–3.1 | 14 / 13 | 1.3–4.7 s up to 120 words, 3.9–6.2 s for 140–484 words |
| Gemma 4 E4B, live, last 2 paragraphs open | 2.9 | 14 / 13 | up to 16 s (open part grows) |
| Gemma 4 E2B 4-bit, whole text | 2.8 | 17 / 19 (drops content) | similar to E4B |
| Qwen3.5 9B 4-bit, prompt v2 | 2.9 | 17 / 19 | – |
| Qwen3.5 4B 8-bit, prompt v2 | 2.9 | 13 / 15 | – |
| Ministral 3 3B | – | answers instead of cleaning | – |

- Sampling: model card. For Qwen3.5-4B/9B the card's non-thinking setting is temperature 0.7, top-p 0.8, top-k 20 (earlier formatting runs used the 2B card's 1.0/1.0 by mistake). The card's presence penalty 1.5 made Qwen paraphrase much more (distance to ideal 0.51 vs 0.36), so it is off. Gemma 4: 1.0 / 0.95 / 64.
- Thinking mode (Qwen 4B): 6,000–8,000 thinking tokens, 200–270 s per text, once no answer. Unusable.
- Gemma 4 E4B: reads ~240–450 tokens/s, writes ~25 tokens/s on M1 Pro (other jobs were running), peak 4.5–4.8 GiB. Caching the 634-token system prompt saves 0.75–4 s per call with identical output.
- Known live artifact: a list heading repeated at a piece boundary (1 of 15). Remaining Gemma errors: a dropped "not", a dropped priority clause, dropped closing requests.
- Not tested: phone speed, Gemma 4 via llama.cpp (Sokki's runtime), a dropped-word safety check.

### Gemma 4 E4B in Sokki's runtime (bundled llama-server, Sokki's start flags), 24 Nov late

Q4_K_M GGUF (unsloth), ctx 8192, --parallel 1, --jinja, --reasoning off, card sampling 1.0/0.95/64, --cache-ram 0. Same 15 dictations, prompt v4. Run while another session's emulator and Rust build were active and swap was nearly full, so all times are pessimistic.

| | Writing speed | Whole text at the end: 85 / 188 / 484 words | Live, 60-word pieces, 1 open: wait 140–484 words |
|---|---|---|---|
| MLX 4-bit (my earlier runs) | ~25 tokens/s | 4.4 / 7.9 / 26.9 s | 3.9–6.2 s |
| llama-server, as Sokki starts it | 32.8 tokens/s | 3.8 / 8.2 / 22.6 s | 2.6–7.3 s |
| + `--spec-type ngram-simple` | 35.7 tokens/s (26 % of drafts accepted) | 3.5 / 7.6 / 16.5 s | 1.6–4.7 s |
| + `--spec-type draft-mtp --model-draft mtp-gemma-4-E4B-it.gguf` (96 MB) | **53.1 tokens/s (82 % accepted)** | **2.6 / 5.3 / 14.4 s** | **1.6–4.6 s** |

- Short texts (30–60 words): 0.9–1.7 s with MTP, whole text at the end.
- Same closeness to the ideals in all three llama-server setups (distance 0.228; MLX 0.234), so speculative decoding does not change quality.
- Memory: resident 4.0–4.5 GB (mostly the mapped model), footprint 0.5–0.6 GB on top; under the 6.4 GB limit.

### Shipped setup rated (25 Nov)

Blind round with two new raters, same 15 ideals: Gemma 4 E4B live in Sokki's llama-server with MTP scored 3.07 / 2.93 (meaning errors 10 / 11), the same as the MLX whole-text run (3.07 / 2.87, 11 / 9) and MLX live (3.13 / 3.00, 15 / 13). Wait after release, replayed at 2.8 words/s: 1.0–1.5 s up to 60 words, 3.0 s at 85, 2.2 s at 188, 4.3 s at 484.
