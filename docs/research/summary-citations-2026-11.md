# Citations in meeting summaries

26–27 Nov 2026, on Robin's Mac (M1 Pro, 32 GB). Numbers only. The real meetings, transcripts and
summaries are private and are not in this repo.

**Task (Robin, 26 Nov 2026):** "each claim in the summary is backed up with citation of the
transcript that leads to this claim." Four questions: (1) Do the summaries change? (2) Do they
become more reliable? (3) How much longer do they take? (4) Does it help a person find what was said
and when?

## Short version

- **Best citations come without the model.** Matching each point's words against the transcript
  lines (no language model, under 0.1 ms per meeting) cites the right lines fully for 78–79 % of
  points and wrongly for 1–2 %. The model's own citations: 59–60 % fully right, 11–12 % wrong. A
  follow-up question in the same chat: 34 % fully right, 32 % wrong.
- **Asking the model to cite makes the summary more reliable.** With numbered lines and a `cites`
  field per point, 90 % of points are fully supported by the transcript, against 83 % today. On the
  6 real meetings: 83 % against 71 %. The gain is in "partly right" points (a wrong detail, a
  discussion written up as a decision): 27 % → 15 % on real meetings. Unsupported points stay at 1–2 %.
- **The summary changes a little.** Fewer points and far fewer decisions on real meetings (12 instead
  of 23 over 12 runs); the decisions it drops are mostly the padded ones Gemma was known for
  (`meeting-summaries-2026-11.md`). On the made-up meetings the content differs about as much as
  two runs of today's prompt differ.
- **It costs 3–8 s.** Median summary time on real meetings 19.8 s → 23.4 s (+3.6 s, +18 %), on the
  made-up meetings 25.0 s → 33.3 s (+7.6 s). The model writes 57–64 % more tokens (the cite
  numbers). Memory is unchanged (5.56 GB peak); word matching needs none.
- **Recommendation:** ask the model for cited points (better summaries), show the citations from
  word matching on those points (better citations). Word matching can also run on old notes and on
  points the user edited, at view time.

## Setup

- **Runtime:** Sokki's bundled `llama-server` with the built-in Gemma 4 E4B Q4_K_M and its MTP draft
  model, Sokki's exact flags (16k context, `--jinja --reasoning off`, the model card's sampling,
  `--cache-ram 0`). Temperature is the model's own.
- **Today's request (A)** was captured byte for byte from `meeting-core`'s summarizer through a
  proxy, with the app's dictionary and user name, then replayed.
- **Approaches:**
  - **A** today's prompt, no citations.
  - **B** the same prompt with every transcript line numbered (`#12 [03:45] Speaker: …`), every
    point, decision and task an object `{"text", "cites": [12, 13]}` (1–4 line numbers),
    enforced by a JSON schema (llama.cpp grammar).
  - **C** no model: `scripts/summary-citations/cite.py` matches A's points against the lines
    (shared words weighted by rarity, prefix match for inflections and compounds, numbers exact,
    a small bonus for the task owner's own lines; up to 3 lines within 60 % of the best score).
  - **CB** the same matching on B's points.
  - **D** A, then a second turn in the same chat: "for each point, the timestamps of the lines that
    state it" (schema with one list per point). It reuses the transcript from llama-server's cache:
    only 390–930 new prompt tokens.
- **Meetings:** 11, each run twice.
  - 6 real meetings from Robin's Sokki (11–45 min; the 4 of `meeting-summaries-2026-11.md` and 2
    from 26 Nov), 37–130 transcript lines, 3,400–8,300 prompt tokens, all in one request.
  - 4 made-up meetings, set `summary-citations-made-up` (German and English, 8–26 min, 110–211
    lines), and Sokki's 2-minute sample meeting (18 lines).
- **Reliability check (Q2):** blind LLM judges (Claude agents), on run 2 of every meeting. Each
  packet held the numbered transcript and all points of A and B shuffled together; each point's
  citation sets were shown under shuffled letters (X/Y/Z), not by approach. Per point: is it
  supported by the transcript (yes / partly / no), and do the cited lines alone state it (full /
  partial / none)? 293 points in round 1 (A with C and D, B with its own cites), 132 in round 2 (B
  with its own cites and CB). A second judge re-judged 2 meetings (56 points): agreement 53/56 on
  support and 87/91 on citation sets.
- **Run safety:** one build-slot job (low priority), stopped if a meeting started; peak memory
  checked every second.

## Q1: Do the summaries change?

| 12 real runs / 10 made-up runs | A (today) | B (with cites) |
|---|---|---|
| Points / decisions / tasks, real | 72 / 23 / 23 | 60 / 12 / 21 |
| Points / decisions / tasks, made-up | 63 / 49 / 86 | 49 / 39 / 76 |
| Characters, real / made-up | 13,114 / 16,212 | 12,584 / 16,720 |
| Share of points with a matching point in the other version (word overlap ≥ 0.25), real | A run 1 vs run 2: 0.48 | B vs A: 0.38 |
| … made-up | 0.89 | 0.88 |

- B writes fewer, slightly longer points and drops about half the decisions on real meetings. Of
  B's decisions 22/25 were fully supported, of A's 32/40.
- On the made-up meetings (clear decisions and tasks), B's content is as close to A as A is to
  itself. On the real meetings it moves a little further than a rerun does.
- Cited lines spread over the meeting alike: 7–8 of 10 time slices per summary in every approach.

## Q2: Reliability

Points supported by the transcript (judge round 1, run 2):

| | n | yes | partly | no |
|---|---|---|---|---|
| A (today), all | 161 | 83 % | 16 % | 1 % |
| B (with cites), all | 132 | **90 %** | 8 % | 2 % |
| A, real meetings | 59 | 71 % | 27 % | 2 % |
| B, real meetings | 48 | **83 %** | 15 % | 2 % |

Round 2 judged B's points again: 89 % yes, 11 % partly, 1 % no.

Cited lines state the point (supported points only):

| Citations | n | full | partial | none |
|---|---|---|---|---|
| C: word matching on A | 160 | **79 %** | 19 % | 2 % |
| CB: word matching on B | 131 | **78 %** | 21 % | 1 % |
| B: the model's own (round 1 / round 2) | 130 / 131 | 60 % / 59 % | 28 % / 30 % | 12 % / 11 % |
| D: follow-up question | 160 | 34 % | 34 % | 32 % |

On real meetings only: C 66 / 29 / 5, CB 70 / 28 / 2, B 74 / 17 / 9 (round 2: 72 / 19 / 9), D 41 / 40 / 19.

- "Partial" is mostly the right place with one line too few or too many (judges marked a set
  partial when one of three lines was off-topic, or when the number sat in the next line).
- D cites far off: 24 % of its sets are more than 10 lines from the matched lines. The model
  recalls timestamps badly after it has written the summary.
- Word matching does worst where a point paraphrases (real meetings, English summary words for a
  German line); the model does worst on long made-up meetings with many short turns.
- **Limits:** LLM judges, not people; 11 meetings, one run judged; a made-up meeting is easier than a
  real one; the judges saw both A's and B's points of a meeting side by side (approach labels
  hidden, but the citation styles differ).

## Q3: Latency and memory

Run 2 (llama-server's cache holds the meeting, as in the app), median:

| | A (today) | B | A + D | A + C or B + CB |
|---|---|---|---|---|
| Real meetings (6) | 19.8 s | 23.4 s | 19.8 + 6.2 s | + < 0.1 ms |
| Made-up meetings (5) | 25.0 s | 33.3 s | 25.0 + 12.7 s | + < 0.1 ms |
| Sample meeting | 8.4 s | 11.2 s | 8.4 + 5.8 s | + < 0.1 ms |
| Written tokens, real | 341 | 534 | + 258 | 0 |

- B's extra time is all writing: its prompt is 5–10 % longer (the line numbers), its answer
  57–64 % longer.
- D re-reads only the new question (390–930 tokens) when the meeting is still in the cache; without
  it (run 1) D took as long as A. Its answer is pretty-printed JSON; a compact format would roughly
  halve it.
- Peak memory of llama-server: 5.56 GB, the same for all approaches.
- Not tested: meetings long enough to be summarised in parts (map/reduce). The cites of each part
  would have to survive the combining step; word matching on the final points does not depend on
  that.

## Q4: What it gives the person

Four ways to show the citations, drawn with made-up content on the design canvas (page "Offen",
row "Belege in der Zusammenfassung"): click a point and the transcript opens beside it at the
highlighted lines; a small timestamp at the end of each point; the quoted lines on hover; play the
recording from that moment. All are instant: the citations exist when the note is shown.

## Built (27 Nov 2026)

Robin picked versions 2, 3 and 1 together, and cited summaries for every meeting:

- Every meeting read in one pass gets the B prompt (`meeting-core/src/summary.rs`). Meetings read
  in parts keep today's prompts (not measured).
- The reader shows citations from word matching on the note's points (`meeting_core::cite`, a port
  of `cite.py`: the same lines for 572 of 573 bench points; the one left is a tie that Python breaks
  by its random hash order). They are found when the note is shown, so old notes and edited points
  get them too. The model's own `cites` are not kept.
- A time mark at every cited point, the quote on hover with "Listen" (only while the recording is
  kept) and "Show in transcript", and a click on the point jumps to the highlighted lines.

## Sources

- Bench: `scripts/summary-citations/` (`run.sh`, `bench.py`, `cite.py`, `analyze.py`,
  `make_judge*.py`, `score_judge.py`).
- Outputs (private): `$SOKKI_EVALS/runs/summary-citations/` (`out/`, `analysis.json`,
  `judge/score.txt`, `judge2/score.txt`).
- Data: `summary-citations-made-up` (`docs/evals.md`) and 6 meetings from the app's `meetings/`.
- llama-server bundled with the installed Sokki (0.2.26 at the time of the run), Gemma 4 E4B Q4_K_M with `mtp-gemma-4-E4B-it.gguf`.
