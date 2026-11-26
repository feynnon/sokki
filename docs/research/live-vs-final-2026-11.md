# Live text vs. pasted transcript, and the live dictation cleanup

25–26 Nov 2026, on Robin's Mac (M1 Pro, 32 GB). Numbers only; the dictations and their texts are private and are not in this repo. Per-dictation numbers: `data/live_vs_final.json`.

## Short version

- **The pasted transcript is slightly more accurate than the live text, and the second transcription costs almost nothing.**
  - Live and pasted text differ in 0.7–0.9 % of words (18–22 of 2,584 words, in 11–14 of 34 dictations).
  - Where they differ, two independent reference models side with the pasted text more often: in 3–6 dictations, against 0–2 for the live text.
  - The second transcription at the stop takes a median of 0.32 s, at most 3.1 s for a 175-second dictation.
  - So the second pass stays. With the cleanup on, what Robin sees as finished paragraphs is what gets pasted anyway: 19 of 20 finished paragraphs shown live were pasted unchanged.
- **The live cleanup needed two fixes before it worked on real dictations.** As first built, it ran during speech in only 2 of 33 dictations, and one 480-word dictation hit the 20-second limit, so its transcript was pasted uncleaned 23 s after the stop. After the fixes:
  - 8 dictations showed cleaned paragraphs while Robin spoke;
  - no cleanup failed and nothing had to be cleaned again from the start;
  - stop to paste took a median of 2.6 s, at most 6.4 s.
- **Short dictations are cleaned only at the stop**, by design (a piece is 60 words): 1.5 s median up to 30 words, 1.9 s up to 60 words.

## How it was measured

- **Dictations:** all 34 of Robin's Mac dictations with a recording of 4 s or more (Sokki history, 20–25 Nov). All are German; 16 minutes of audio, 2,584 words; 4–175 s long.
- **Replay:** each recording was played in real time as the microphone of a separate test app ("Sokki Test Cleanup", `scripts/test-dictation.sh`, test mode: nothing pasted, no window). It used Robin's settings: Parakeet Ultra, live text on, dictation cleanup on with Gemma 4 E4B. The app logged every live text update, the transcript used for the paste, the cleanup's steps and the pasted text. Raw logs stay in the private archive.
- **Live text:** the last live text before the stop, i.e. what the live panel showed: the decoded chunks plus the last preview of the growing chunk.
- **Pasted transcript:** what Sokki pastes. Parakeet Ultra keeps its accuracy on long audio, so the live decode only previews and the whole recording is transcribed again at the stop (`LiveDecode::Preview`).
- **References (tie-breakers):** Whisper large-v3 (Q8_0) and Canary 1B v2 (Q8_0), each on the whole recording with the language hint "de" (`crates/meeting-e2e/examples/reference_transcripts.rs`, pieces of at most 28 s cut at quiet points). They are not ground truth:
  - they differ from each other in 11.3 % of words, and from Parakeet in about 10 %;
  - much of that is spelling, numbers and compound words.
  
  So they decide only where live and pasted text differ.
- **Counting:** word-level edit distance after lower-casing, removing punctuation and removing fillers (äh, ähm, hm…). For each dictation, the live and the pasted text were each compared to both references.
- **Same audio, four rounds:** the cleanup changed between rounds; the speech recognition did not.

## Live text vs. pasted transcript (round 4, n = 34)

| | Pasted transcript | Live text |
|---|---|---|
| Words differing from Whisper large-v3 | 10.5 % | 10.6 % |
| Words differing from Canary 1B v2 | 10.8 % | 10.9 % |
| Closer to Whisper, per dictation | 5 | 2 (27 the same) |
| Closer to Canary, per dictation | 3 | 0 (31 the same) |

- Live vs. pasted: 18 words differ (0.7 %), in 11 dictations. In round 1, 2 of the 22 differing words were missing at the very end: the last preview lags the end of the audio by up to half a second.
- Across rounds 1–4 the gap between pasted and live text was 0.1–0.6 points in the pasted text's favour.
- Robin's earlier history transcripts and the replay's pasted transcripts differ in 27 words. Same audio, same model; the difference comes from the newer build's text handling.
- The second transcription at the stop: median 0.32 s, at most 1.2 s up to 64 s of audio, 3.1 s for 175 s.

**Conclusion:** dropping the second transcription would save about 0.3 s and lose a little accuracy. Keep it.

## The live cleanup, round by round

Each round replayed all 34 dictations (round 1: 33; the 175 s dictation ran into a script timeout, since fixed).

| Round | What the cleanup took while Robin spoke | Live steps | Dictations with cleaned paragraphs shown live | Cleaned again at the stop | Failed (20 s limit) | Stop to paste, median / max |
|---|---|---|---|---|---|---|
| 1 | committed chunks only | 2 | 2 | 1 dictation, all of it | 0 | 2.94 / 7.51 s |
| 2 | + sentences two previews agree on | 10 | 8 | 1 dictation, all 480 words | 1 | 2.64 / 23.07 s |
| 3 | + go back one step on a change | 26 | 8 | 2 dictations, 591 words | 1 | 2.51 / 23.18 s |
| 4 | + up to 3 % changed words keep the cleaned part | 20 | 8 | 0 | 0 | 2.58 / 6.40 s |

What each change fixed:
- **Round 1:** the decoder commits a chunk only at a pause after about 25–32 s, so most dictations had no committed text before the stop.
- **Round 2:** complete sentences that two live previews in a row agree on are settled enough to clean (`cleanup::settled`). That brought 10 live steps. But when the text later changed under a cleaned piece, the cleanup stopped cleaning live and redid everything at the stop: the 480-word dictation hit the 20 s limit.
- **Round 3:** on a change, the cleanup goes back to the step before it and continues (`Live::roll_back`). One changed word in the first piece still meant redoing every piece after it. In the 480-word dictation that happened 4 times in 10 s while Robin spoke ("das"/"es" at word 68), and again at the stop.
- **Round 4:** text that matches the cleaned words except for at most max(3, 3 %) words counts as the same (`after_prefix_close`). Those few words stay as they were cleaned: 12 words in 2 dictations. Bigger changes still go back one step: 5 times while speaking, 0 times at the stop.

Wait after the transcript in round 4, by length:

| Words | Dictations | Median | Max |
|---|---|---|---|
| up to 30 | 6 | 1.50 s | 1.67 s |
| 30–60 | 9 | 1.94 s | 2.22 s |
| 60–140 | 17 | 2.61 s | 5.71 s |
| 177 and 480 | 2 | 3.09 s | 3.13 s |

- The 5.71 s case was 101 words with no live step: its sentences did not settle before the stop, so two steps ran at the end.
- What Robin saw vs. what was pasted: 19 of 20 finished paragraphs shown live were pasted unchanged. The open last paragraph is cleaned again with the rest at the stop, and was unchanged in 1 of 8.

## Not measured

- **English dictations:** none of the 34 were English.
- **The phone:** not measured.
- **Whether the 12 kept words are right:** words where the cleaned part kept the live version are not checked against the references.
- **Robin's real-time impression of the panel:** the panel shows about three lines (400 × 120), so cleaned paragraphs scroll up as he keeps speaking (KNOWN_ISSUES C11).
