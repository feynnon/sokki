# Live cleanup as one growing text, and the summary model loaded during the meeting

26 Nov 2026, on Robin's Mac (M1 Pro, 32 GB). Numbers only; the dictations are private and not in this repo. Per-dictation numbers: `data/live_pieces.json`.

**Task.** Robin, 26 Nov 2026, after seeing the live panel screenshots:
1. "Das Modell sollte bereits während des Meetings geladen werden, damit die summary dann schneller kommt."
2. "Wäre cool, wenn das Weitersprechen auf der bereinigten Variante direkt aufbaut. Also quasi ein Text, an den immer wieder hinzugefügt wird, und dann formatiert wird."

## Short version

- **Live panel:** the raw words now follow the cleaned text on the same line and are replaced there once cleaned. With the cleanup on, the panel is six lines tall instead of three. Live pieces are 30 words instead of 60; the rest at the stop is still cut at 60.
  - 30-word pieces show cleaned text while speaking in 16 of 34 dictations (8 with 60, 21 with 20; 20 words was not clearly better overall, see the table).
  - The wait after the transcript drops: median 1.90 s, max 4.13 s (60 words: 2.19 s / 5.71 s in the earlier round 4).
  - The pasted text is not measurably different: 30 vs 60 words differ in 5.9 % of words, two runs at 60 words in 5.1 % (the model samples at temperature 1.0). Both change 7.0 % of the transcript's words; neither repeats passages.
  - The raw words after the cleaned text are a median 40 (60-word pieces: 50), because the sentence being spoken is never cleaned. About 45 words fit in the six lines, so the end of the cleaned text is in view in 58 % of panel updates (41 % with 60-word pieces).
- **Summary model:** loaded a minute into the meeting when the Mac has the memory, it saves the load at the stop. In the sample meeting the summary step took 8.4–9.5 s instead of 11.9–14.9 s (4 and 5 runs). macOS raises a memory warning when this model loads either way; the guard goes by free memory instead.
- **Reading the transcript** is most of a long summary's wait: 3.6 s for 1,000 words, 31 s for 6,000, 52 s for 9,000. Doing that during the meeting is not built (see below).

## 1. Live panel: one growing text

Before: cleaned paragraphs, a blank line, then the raw words. The panel showed about three lines (400 × 120), so the cleaned text scrolled out of view while the raw words showed (KNOWN_ISSUES C11).

Now (`src/overlay/RecordingOverlay.tsx`, `src-tauri/src/overlay.rs`, `cleanup::LIVE_PIECE_WORDS`):
- the raw words follow the cleaned text after a space, in italics; when their piece is cleaned they are replaced in place;
- with "Clean up dictations" on, the panel is 400 × 184 (six lines); without it, it stays at three;
- live pieces are cut at the first sentence end after 30 words; at the stop, after 60 (fewer steps while the person waits).

### Replay of Robin's 34 Mac dictations

Same method as `live-vs-final-2026-11.md`: 34 dictations (16 min, 2,584 words), each played in real time as the microphone of a separate test app with Robin's settings, the cleanup on. Same build for all piece sizes; only `SOKKI_TEST_PIECE_WORDS` changed.

| Live piece | 20 words | 30 words (built) | 60 words |
|---|---|---|---|
| Dictations with cleaned text while speaking | 21 | 16 | 8 |
| Live steps | 90 | 75 | 24 |
| Went back a step because the live text changed | 37 | 31 | 10 |
| Failed | 0 | 0 | 0 |
| Wait after the transcript, median / max | 1.93 / 6.02 s* | 1.90 / 4.13 s | 2.49 / 11.17 s* (round 4: 2.19 / 5.71 s) |
| … 60–140 words, median | 1.91 s | 1.88 s | 3.05 s* (round 4: 2.61 s) |
| … the two longest (177 and 480 words), median | 5.18 s* | 3.00 s | – (round 4: 3.09 s) |
| Raw words after the cleaned text, median / p90 | 36 / 73 | 40 / 77 | 50 / 87 |
| Panel updates with the cleaned text's end in view (≤ 45 raw words) | 66 % | 58 % | 41 % |
| First cleaned text after the start, median | 19 s | 21 s | 34.5 s |
| Pasted text: transcript words changed | 7.6 % | 7.0 % | 7.0 % |
| Paragraphs / list items | 95 / 7 | 93 / 9 | 83 / 16 |

\* During the 20-word and the 60-word runs, release builds ran outside the build queue (the queue logged them); their waits are longer partly for that reason. Round 4 (25–26 Nov, also 60 words, nothing else running) is the fair comparison for 60 words; 20 words has none.

**Decision: 30 words.** 20 words shows cleaned text in 5 more dictations and keeps the end in view a little more often (66 % vs 58 %), but goes back a step more often and its long dictations waited longer at the stop (partly the outside builds). Neither is clearly better; 30 stays until a clean 20-word run says otherwise.

- **Quality.** Pasted texts at 30 and at 60 words differ in 5.9 % of words, 20 and 30 words also in 5.9 %; two runs at 60 words (this one and round 4) differ in 5.1 %. The piece size changes the result about as much as running it again does. Repeated 5-word sequences: 9 at 20 and at 30 words, 11 at 60, against 9 in the transcripts themselves, so pieces do not duplicate text where they meet. List items vary between runs at the same piece size too (10 and 16 at 60 words). One pasted text at 20 words contains a "…" the transcript does not have (none at 30 or 60).
- **Why the raw part stays long.** The cleanup only takes whole sentences that two live previews agree on; the sentence being spoken stays raw, and a piece waits for 30 words of such sentences. A smaller piece shortens only the second part.

### Four looks for the raw words

On the design canvas, page "Offen": italic (built), dimmed, dotted underline as for text still being typed, and on a light highlight. Screenshots from the test harness with invented text (`tests/panel-versions.spec.ts`, `DESIGN=1`).

## 2. The summary model during the meeting

Built (`src-tauri/src/meeting/preload.rs`, `llm::preload`):
- 60 s into a recording, the built-in model (Gemma 4 E4B with its MTP draft model, 16k context) starts in llama-server and waits; the summary takes it (`llm::open`). Processing stops it if no summary does; so does discarding the meeting.
- Only with memory to spare: after its 5.6 GB at least a quarter of the Mac's memory must still be free, with no memory pressure. A 32 GB Mac needs 13.6 GB free, a 16 GB Mac 9.6 GB; an 8 GB Mac never preloads. Without it, the model loads for the summary as before.
- While it waits, it is stopped at critical memory pressure, below 15 % free, or at a memory warning below 25 % free.
- Disk reads are throttled (`taskpolicy -d throttle`) so the recording keeps the disk. macOS's background band (`taskpolicy -b`) would also throttle CPU and GPU, and can't be lifted afterwards: a process started in it stayed ~5× slower after `taskpolicy -B -p` and after `setpriority` (tested 26 Nov), so the summary would have run slowly.

### Measurements

**Waiting costs nothing but memory.** The loaded server used 0.21 s of CPU in 20 s idle (1 % of one core), at 5.4 GB resident.

**Sample meeting** (`scripts/test-sample.sh`, the bundled 2-minute invented call, model loaded 30 s in):

| Run | Model | Summary step | Processing after the stop |
|---|---|---|---|
| without preload (5 runs) | loaded for the summary, 3.8–5.6 s | 11.9–14.9 s (median 13.7) | 13.1–16.5 s |
| with preload, kept (4 runs) | loaded during the call, 3.3–5.1 s | 8.4–9.5 s (median 8.9) | 9.6–10.9 s |
| with the first guard (3 runs) | stopped during the call on the memory warning | loaded again: 11.8 s in one | – |

The preload saves the 4–5 s load, and the summary step itself also runs about 1 s faster (the model's files are already in memory). Peak llama-server memory: 5.4 GB either way.

**Memory warning.** macOS raises its memory warning (`kern.memorystatus_vm_pressure_level` 2) when this model loads, whether during the call or for the summary: in 4 of 7 preloads (free memory at least 34 %) and in both normal loads with per-second samples (off3, off5: at least 39–42 % free). During a preload it can stay while the model waits, with about 50 % of memory free again, and it cleared 6 s after the model stopped. A first guard stopped the model on any warning, and so threw away three of four preloads; the built one goes by free memory and kept all four since (one of them with a warning at 35 % free).

**Call audio.** Test mode plays the call from a file, so it can't show whether the real system-audio tap drops out under this load. The load reads 5 GB with throttled disk reads for 4–12 s and then does nothing. The heavy load on 24 Nov 2026 that cost a call its audio was eight transcriptions plus a model in a loop. The tap is rebuilt when it goes silent since 24 Nov (`system_tap`).

### Reading the transcript during the meeting: measured, not built

Prefill speed of the summary model in Sokki's llama-server (M1 Pro, invented German meeting text, same flags as Sokki):

| Transcript | Prompt tokens | Reading time |
|---|---|---|
| 1,000 words | 1,780 | 3.6 s |
| 3,000 words | 5,349 | 11.8 s |
| 6,000 words | 10,928 | 30.8 s |
| 9,000 words | 16,208 | 51.8 s |

For a meeting of half an hour or more, reading the transcript costs several times the 4–5 s load. It can't be read in ahead with today's prompt:
- the prompt opens with the list of speakers;
- the transcript's speaker names are set only after the stop (voices, the call app, the calendar);
- llama-server reuses work only for an identical start of the prompt.

What would work: summarise parts of a long meeting while it records and only combine them at the stop (the map step that long meetings already use), or put the transcript as heard first and the names after it. Both change the summary prompt and put 10–50 s of GPU work into the call. That needs a blind rating and a test meeting under that load first (KNOWN_ISSUES S14).

## Sources

- Replay logs and per-dictation results: private, `reports/Sokki eval data/live-vs-final/logs5` (30 words), `logs6` (60), `logs7` (20), `results-*.json`.
- Sample meeting runs and per-second memory: build-slot jobs of 26 Nov 2026 (local).
- Prefill benchmark: llama-server b12572 bundled with Sokki, Gemma 4 E4B Q4_K_M with the MTP draft model, `--ctx-size 16384`.
