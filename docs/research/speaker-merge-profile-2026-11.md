# Speaker merge: a speaker given to a known person, and an extra voice sample (Nov 2026)

Numbers only; the recordings and names are private and stay out of the repo.

## 1. The real case (27 Nov 2026)

**Task given:** in the meeting of 26 Nov 2026, 13:44 (in the room, mic only), "Speaker 2" is, as Robin remembered it, Robin himself further from the microphone. Measure how close that speaker is to Robin's voice profile today, what adding it as an extra sample would do, and whether the profile still rejects other voices.

**Data:** the app's `speakers.json` of 38 processed meetings (one embedding per speaker, 3D-Speaker ERes2Net, 192 dimensions) and `voice-profiles.json`, copied read-only to a debug folder at 06:07 and 06:15. Robin's profile has 1 embedding. Threshold for a profile match: 0.6 (`VoiceProfiles::DEFAULT_THRESHOLD`). Script: `sims.py` in the debug folder (cosine similarity, best over a profile's embeddings, as `VoiceProfiles::scores`).

**The meeting's speakers:** 4 on the mic. Robin 213.9 s (profile 0.90), a colleague 225.1 s, Speaker 2 17.8 s, Speaker 3 20.6 s. Speaker 3 is the colleague split off (0.77 to him).

| Speaker 2 against | Cosine |
|---|---|
| Robin's profile | 0.24 |
| Robin in the same meeting | 0.21 |
| Robin's 22 named speakers in other meetings | median 0.24, max 0.34 |
| another person, named in 5 other meetings | 0.69, 0.75, 0.76, 0.85, 0.85 |
| all 82 other speakers | 6 at ≥ 0.5 (those 5 and one unnamed at 0.52), 27 at ≥ 0.3 |

**At 06:15 on 27 Nov** that voice was given the other person's name across 6 meetings at once, this one included (the Sprecher page; `speakers.json` of all six changed in the same second). So the voice data and that naming agree: Speaker 2 is most likely not Robin.

## 2. What the extra sample would have done

Speaker 2's embedding added to Robin's profile (2 embeddings, best match), checked against the 104 speakers of the other 37 meetings:

| Profile | Robin's speakers named Robin (≥ 0.6) | Other speakers named Robin |
|---|---|---|
| today (1 embedding) | 19 / 22 | 2 / 82 (max 0.92) |
| + Speaker 2 | 19 / 22 | **7 / 82** |

The 5 new wrong ones are all the other person. A wrong sample doesn't stay in one meeting: with "best over the embeddings", every later meeting of that person would be named Robin. (The 2 "other" hits today, at 0.6–0.92, are speakers named differently but likely Robin's own voice in a call; not checked by ear.)

## 3. What this means for the design

- A sample is only added after a check: Sokki compares the speaker with the person's profile and with the other known voices. When it is close to someone else (here 0.85, against 0.24 to the person), Sokki says so before learning ("klingt eher wie N.") and learns nothing unless the user confirms. A low score to the person alone is no reason to refuse: Robin's own named speakers score as low as 0.15 (below).
- A speaker can still be merged into the person in the note (the user knows who spoke); merging and learning are separate steps.
- Every sample keeps where it came from (meeting, speaker), so removing it undoes exactly that sample.
- Too short a sample (under ~5 s of speech) is not learned; 17.8 s is enough by length.

**Where an extra sample would help:** Robin's 22 named speakers against his one-embedding profile: 0.15, 0.32, 0.43, then 0.68–1.00. Those 3 are named Robin but would not be recognised (under 0.6); they are the "same voice, other conditions" case Robin means. Each added alone as a second embedding:

| Added sample (score before) | The other two low ones | Robin's speakers named Robin | Other speakers named Robin |
|---|---|---|---|
| 17 Nov (0.43) | 0.15, 0.40 | 20 / 22 | 2 / 82 |
| 19 Nov (0.15) | 0.43, 0.32 | 20 / 22 | **33 / 82** |
| 20 Nov (0.32) | 0.43, 0.26 | 20 / 22 | 2 / 82 |

- A sample fixes its own meeting and helps nothing else here: the three low ones are not like each other (0.26–0.40).
- One of them (0.15) is a voice that looks like many others: added, it would name 33 of 82 other speakers Robin. Probably a mixed cluster (several people, or noise) that carries his name.

**Rule this gives (planned, built with the picked version):** before a sample is added, Sokki scores it against every other speaker it knows (other people's named speakers in all meetings and their profiles). If adding it would make any of them match the person (≥ 0.6), it isn't learned, and the card says why. On this data the rule refuses Speaker 2 of 26 Nov (5 speakers of the other person) and the 19 Nov sample (31 more), and accepts the other two.

## Sources

- Meeting `meeting-20261126-134413-057` and the 37 other processed meetings' `speakers.json`, `voice-profiles.json` (debug copies under `~/sokki-debug/speaker-merge/`, delete after 2026-12-11).
- `crates/diarize/src/profiles.rs` (`scores`, `identify`, `MAX_EMBEDDINGS_PER_NAME` = 16), `src-tauri/src/meeting/voices.rs` (`name_voice`).
- `docs/research/speaker-count-2026-11.md` (same-meeting splits at 0.76–0.98; two named people at most 0.59).
