# Müsli, Qwen and Gemma dictation cleanup: running log (23–25 Nov 2026)

A log the test sessions kept while they worked, newest at the end. The main findings are in `dictation-formatting-and-cleanup-2026-11.md`; aggregated numbers in `data/`. The dictations themselves are private and not in the repo.

Full report (private, contains Robin's dictations): private notes (Müsli Diktat-Cleanup Test.md, not in the repo).

- Published checkpoints: v14 MLX 8-bit in **2B** (2.0 GB) and **0.8B** (0.8 GB); v12 also as GGUF (2B, 0.8B), LFM 1.2B/1.7B and Gemma-E2B variants. The "Muesli = 0.8B" claim in the private note "Cloud Training für Sokki Shape" covers only the small one.
- Model card: English only; greedy decoding; fixed English prompt built by hand (no system turn, empty `<think>` block).
- M1 Pro, mlx-lm 0.32.2, warm: 0.8B 0.55 s (14 words) / 0.85 s (47) / 1.9 s (177); 2B 1.15 / 2.0 / ~6–7 s; 484 words: 2B 16 s, 0.8B 10 s. Load 1.2–2.1 s. Peak 1.1–2.3 GB (0.8B), 2.2–3.3 GB (2B).
- Robin's 36 history dictations (all German): 2B changed 18 (12 better, 5 mixed, 1 worse); 0.8B changed 13 (6 better, 6 worse, e.g. "Dann" → "Danke"). Neither fixes misheard names/terms. English control sentences: both perfect.
- Integration idea (not built): optional step before paste, English by default, via the bundled llama-server with GGUF, guards (empty / >3x / >15% words changed / >1.5 s → paste raw), custom words in `<APP-CONTEXT>`, store result in `post_processed_text`.
- Setup to reuse: venv + scripts archived privately with the test data (`run_muesli.py`); models in `~/.cache/huggingface/hub` (v14 2B + 0.8B, 2.8 GB).

## Update (same day): thinking, prompt lookup, Parakeet, live cleanup

- Thinking was off in all runs (model-card prompt closes `<think></think>`). With it on, neither model answered within 2048 tokens (loops, 15–39 s). Keep it off.
- mlx-lm has no prompt-lookup mode; its draft-model mode fails on Qwen3.5 (linear-attention ArraysCache is not trimmable). Hand-built PLD (snapshot ArraysCache list + trim KVCache on reject): identical output in 8/8 runs. Pitfall: `ArraysCache.state` returns the live list, so copy it (`list(c.cache)`), or rollback silently corrupts.
- Parakeet (`sokki transcribe`, fresh process each run, load time excluded) vs Müsli warm, same recordings, 14/47/177/484 words: Parakeet 0.25/0.46/2.2/3.5 s; 0.8B 0.31/0.59/2.3/5.5 s, with PLD 0.24/0.29/0.63/2.1 s; 2B 0.61/1.2/3.9/12.6 s, with PLD 0.44/0.87/1.45/3.9 s.
- Live simulation (2B + PLD, per sentence, cleaned text so far as APP-CONTEXT, word-loss check per sentence → keep the original sentence): wait after release median 0.61 s, max 0.96 s, at any length; 9/162 sentences kept as original, mostly correctly (a translation, a deleted half-sentence, "weiß nicht" → "weiß"). Not yet measured on Sokki's real live text (Sokki Test with SOKKI_TEST_MIC would show it).
- MLX buffer cache grew to 9 GB with PLD's changing shapes, and mac-guard ended the job; `mx.set_cache_limit(512 MB)` + `mx.clear_cache()` per call kept it at ~3 GB.

## Update 2: Müsli on 108 real transcripts (private notes (Müsli im Einsatz.md, not in the repo))

- Data: 47 Mac dictations (history.db + voice-data/manifest.jsonl), 35 Mac meeting turns (from meeting notes), 26 Android phone-call turns (5 TTS test calls, 9 unique). Android keyboard (dev.verbatim.mobile) stores no dictation texts, only a counter file `files/keyboard_dictations`.
- Formatting: 0/108 got paragraphs or lists (no cues in real speech). With cues ("erstens…", "number one…", letters) even 0.8B formats German well; "App: Notes" context turns an enumeration into bullets (0.8B yes, 2B no). German spoken commands ("Neue Zeile", "Neuer Absatz") stay in the text.
- 0.8B fixes built: token mask (only tokens from the transcript + punctuation/newlines/markers/digits), plus a word-alignment filter that keeps the output's formatting but undoes disallowed word edits (allowed: filler, one of a doubled pair, self-correction cue, list cue → marker, joined words, spelling fix only if the macOS spell checker rejects the original word). Scripts (archived privately with the test data): `fmt_eval.py`, `spell/spell.swift`.
- Proposed (not built): rule-based spoken commands before the model; paragraphs from VAD pauses; filler-list policy per use (meeting strict / dictation loose); prompt cache.

## Update 3 (23.11.): formatting without Müsli
- Paragraphs from speech pauses (rule: sentence end + pause, ≥12 words/paragraph, texts ≥40 words): T8 layout exactly ideal; 15/18 long Mac dictations get sensible paragraphs. Saved recordings have pauses capped at ~0.9 s by the VAD (VAD_PREFILL_MS/VAD_OFFLINE_HANGOVER_MS 450), so only pause yes/no is usable offline; live knows real length. Phone keeps no audio.
- Spoken commands ("neuer Absatz", "neue Zeile", "Stichpunkt", EN equivalents) as a rule: works on TTS test files; 0 false positives in 146 real texts.
- Plain Qwen3.5 2B/4B with prompts: they format (Müsli doesn't), best layout F1 0.77 (4B, rewrite + word filter, no presence penalty) but ~6 s and 5.1 GB, rewords heavily; 9B skipped (5.6 GB file > 6 GB cap).
- Details: private notes (Müsli Testfälle Formatierung).md, section "Formatierung ohne Müsli".

## Update 4 (23.11. evening): Qwen sizes + latency
Full report (private, not in the repo): Qwen model comparison and latency.md
- Qwen3.5 4B-4bit and 2B format about as differently from 4B-8bit as 4B-8bit does from itself with another seed (temp 1.0). Blind mean (new rule): 4B 3.53, 4B-4bit 3.32, 2B 3.16, 0.8B 2.87 (0.8B barely structures).
- A layout rule fix cut "bad" layouts in half: a list item only starts after punctuation or at und/oder/aber/dann (or where Qwen wrote a colon with no dropped words), and no one-item lists.
- Prompt cache of the fixed system prompt (387 tokens): ~1 s saved per call, same output.
- Prompt lookup: slower on Qwen3.5 (hybrid cache can't roll back; 21% acceptance at temp 1.0). Dropped.
- Live chunking: 40-word chunks are fast but visibly worse; 60-word chunks with the last 2 paragraphs open match the full pass on long texts (10 texts, small sample). Combined (full under 100 words, live above): 3.4 s median, max 9 s (was 4.7 / 23 s), 4B 8-bit ~5 GB.
- Nothing built into Sokki; scripts are archived privately with the test data.

## Update 5 (24.11.)
- Live while speaking: Parakeet transcription ~1.9x slower while a Qwen 4B step runs (14 s audio 0.30 -> 0.54 s), still ~25x real time; Qwen ~15 % slower. Qwen busy 15-40 % of speaking time.
- "Re-format the last 3 chunks" window: no measurable quality gain (two blind raters: 3.6 vs 3.2, then 3.1 vs 3.1, 10 long texts); window grows to 220 words (commit only at paragraph ends), so long texts wait 10-14 s incl. backlog. Recommendation unchanged: full pass <100 words, live 60-word chunks / 2 open from 100 words.
- Report: private notes (Qwen model comparison and latency).md, section 5.

## Update 6 (24.11.)
- Break voting (2 samples of recommended + 3-chunk window as tie-breaker): best quality so far; two blind raters: bad 7-8 -> 3-4 of 38, mean 3.39 -> 3.53/3.58, never clearly worse; gain on texts >=100 words.
- Batch of 3 = 1.9-2.1x one call (not free), 5.7 GB peak. Three live streams simulated: 10-26 s wait on long texts. Cheaper designs untested.
- Live bug found: committed paragraph can keep a colon when its list dissolves later; fixed by one read-back clean-up pass over the final text.
- Following the release manager's rules since 10:28: check meetings/index.json before heavy runs, < 4 GB per job.

## Update 7 (24.11. afternoon)
- Focus ("rewrite 1 of 3 pieces") worse on long texts (2.35 vs 3.2) and not faster at release. Fixed 3-piece window: fast (≤ 9 s) but not better.
- Qwen3.5-9B (MLX 4-bit) not better than 4B 8-bit with this prompt (3.13 vs 3.33; bad 20 vs 16); vote doesn't help it. Barely slower (3.9 vs 3.4 s).
- 4B vote confirmed again (rounds 8 and 9, two raters each).
- Robin's memory limit = Sokki's 9B, measured 6.4 GB; in ~/PycharmProjects/CLAUDE.md. Findings: `dictation-formatting-and-cleanup-2026-11.md` in this folder.

## Update 8 (24.11. evening): word cleanup with Gemma 4
- Robin approved full cleanup (words + layout) against 15 hand-written ideals (private notes (Ideal dictation corrections).html", private). Robin likes Gemma 4 E4B (4-bit MLX) with prompt v4 and wants it kept.
- Blind raters vs ideals: Gemma 4 E4B 3.0/5, fewest meaning errors of the models that really clean; Qwen3.5 9B cleans most but changes meaning more; Ministral 3 3B answers instead; Qwen thinking mode unusable (200–270 s/text).
- Latency: live 60-word pieces with the last paragraph open = same quality, wait 3–6 s at any length (full pass: 27 s at 484 words). System-prompt cache saves 0.75–4 s.
- Numbers only: `dictation-formatting-and-cleanup-2026-11.md` in this folder. Nothing built into Sokki.
- Robin's decision (24.11. evening): live cleanup in 60-word pieces, last paragraph kept open, with Gemma 4 E4B and prompt v4 should be Sokki's standard dictation cleanup. Not built yet; llama-server (Sokki's runtime) benchmark running.
- Follow-up queued (Robin, 24.11.): after the Mac Gemma cleanup is built and proven, a new session: (1) latency test of Gemma 4 directly on the Pixel, (2) phone sends dictation to the Mac, Mac cleans near-live and sends back. Board card "handy-diktat-gemma" (next). Start it when session 1bec1746 closes.
