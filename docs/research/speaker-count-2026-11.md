# Speaker count: one voice split in two, video sound as speakers (Nov 2026)

Why a meeting of two people came out with five speakers, and what fixes it. Numbers only; the recording is private and stays out of the repo. Tools: `crates/meeting-e2e/examples/speaker_count.rs` (replays a meeting's speaker detection the app's way, window by window, then the pipeline) and `lang_check.rs` (loudness and language per line).

## 1. The meeting (26 Nov 2026)

**Task given:** "Here are 4 speakers. In reality we were 2 … explore how we get to the right speaker count and whether background noise should be thrown out." Robin and one colleague, in person, 10.6 min, started with the meeting shortcut, no call app. A video ad played in the browser for 8 s at 5:30.

**What the app wrote:** 5 speakers.
| Track | Speaker | Speech | Who |
|---|---|---|---|
| mic | the user (voice profile, 0.89) | 178.0 s | Robin |
| mic | Speaker 2 | 134.1 s | colleague from minute 5 on, plus a short stray cluster |
| mic | Speaker 3 | 82.7 s | colleague, minutes 0–4 |
| system | Speaker 4 | 6.4 s | the ad |
| system | Speaker 5 | 0.6 s | the ad |

Speaker 2 and Speaker 3 have cosine similarity 0.93: one voice.

**Replay:** `speaker_count` reproduces it (132.9 / 178.0 / 83.1 s on the mic, 6.4 / 0.6 s on the system track) when the echo filter starts where the system audio starts, as in the app. With the echo filter from 0 s the first window comes out differently (no split, a 10.9 s stray cluster instead): the windows are sensitive to small changes in the audio.

## 2. Why one voice became two

The app finds speakers in windows of 5 min with 1 min overlap (`meeting_core::stitch`). Window 0–300 s found three clusters: Robin (99.3 s), the colleague (83.7 s) and a second cluster of the colleague (37.7 s, cosine 0.68 to his main one). Window 240–600 s found him as one cluster (101 s); the stitcher matched it, through the overlap, to the *second* cluster. From minute 5 on he was "Speaker 2", before it "Speaker 3". Nothing joined them afterwards: `DiarizeConfig::merge_similarity` only acts inside one window and was off.

**Other meetings:** voice similarity between speakers of the same meeting, from the app's own `speakers.json` (38 meetings, 127 pairs):
- 9 pairs in 8 meetings at 0.76–0.98 (this one included). Where names were given, these are one person split in two (for example the same colleague on 24 Nov: 0.91).
- 3 pairs at 0.67–0.72, unverified.
- Two different, named people: at most 0.59. The rest: under 0.5.

**Fix (built):** after the windows are joined, speakers of one track whose voices have cosine ≥ 0.75 become one (`stitch::merge_same_voice`, `PipelineConfig::same_voice`). The one with more speech keeps its label; the joined voice gets the speech-weighted mean embedding.

| | Mic speakers | All speakers |
|---|---|---|
| Before | 3 | 5 |
| After | 2 | 4 |

Replay of 16 other real meetings (speaker detection the app's way, window by window; 7 h of audio): joins in 6 of them, 8 joins in all (an 80-min call 6 → 4 on the system track, a 37-min call 8 → 6, a 45-min call 10 → 9, three meetings in the room one join each). Each join matches a pair that scored 0.77–0.98 in that meeting's `speakers.json`; where names were given, the joined voices were one person, and the two named different people in those meetings (0.59) stayed apart. 10 meetings unchanged. Not verified by ear: the 0.77 pair of a meeting on 26 Nov (4 → 3 in the room).

**End to end on this meeting** (window replay, then the pipeline with Parakeet Ultra and the voice profiles, the user's profile as the user): before 5 speakers (mic 132.9 / 178.0 / 83.1 s, system 6.4 / 0.6 s); after 4 (mic: the user 178.0 s, named by profile; the colleague 215.6 s as "Speaker 2"; system 6.4 / 0.6 s).

**Second bug, found on the way (built):** with any speech on the system track, the mic speaker with the most speech was taken as the user. After the join the colleague had more mic speech (215.6 s) than Robin (178.0 s), so he would have been named after the user, next to Robin's voice profile: two "Robin"s. Now a mic speaker that the voice profiles recognise as the user is the user.

## 3. Sound from the Mac in a meeting in the room

The system track had sound only from 5:30 to 5:38 (the tap rebuilt three times while Chrome played; zeros otherwise). No call app was detected and the meeting was started by hand. Those 8 s became two speakers, 6.4 s and 0.6 s (cosine 0.12). Ghost absorption doesn't catch them: it only folds a tiny cluster into another speaker on the same track, and they were the only two.

**Idea, rejected (Robin, 26 Nov 2026):** treat the Mac's sound as no participant when no other app used the microphone (no call). Robin: "In General, these should be speakers. I just didn't remember that there was an ad playing." Sound the Mac plays stays on the transcript as speakers; nothing is built for it. So this meeting correctly has 4 speakers: the two people and the two voices of the ad.

## 4. Why the colleague wasn't recognised

He has no voice profile. He was named in one earlier meeting (24 Nov) without the consent box, so no voice was kept (GDPR Art. 9, see `speaker-names-call-apps-2026-11.md` §4). His voice here vs. that meeting's named speaker: 0.78; the user's profile matched at 0.89–0.90. The threshold for a profile match is 0.6, so once his voice is kept with consent he will be named.

## 5. Short lines in the wrong language

The app's note has 3 short lines (under 2 s) in other languages in the first 30 s (Polish, Czech, Cyrillic); the replay has 3 at the same place.
- Not quiet background: −30.5 to −34.2 dB, louder than the median line (−36.8 dB; lowest −48.9).
- A language hint does nothing with Parakeet Ultra: German and English hints changed 0 of 158 lines (Parakeet reports no detected language either).
- The text of these lines changes between runs (transcribed alone vs. in the pipeline: 106 of 158 lines differ at all; the odd ones come out in different languages each time).
- Not fixed. Options: drop a short line whose script or letters (Cyrillic, ř, ł, ą …) belong to neither meeting language; or transcribe short turns together with their neighbours (R11, `window_s`, off because mixed-language windows broke). Needs more real cases before a rule (CLAUDE.md "As few rules as possible").

## Sources

- Meeting `meeting-20261126-093130-043` (debug copy, delete after 2026-12-10), the app log of 26 Nov 2026, `speakers.json` of 38 meetings.
- `crates/meeting-core/src/stitch.rs`, `pipeline.rs`, `crates/sokki-engine/src/live.rs`, `crates/diarize/src/lib.rs`.
