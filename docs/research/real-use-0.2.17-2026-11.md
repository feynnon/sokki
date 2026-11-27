# Real-use review of Sokki 0.2.17 and later (27–29 Nov 2026)

**Task.** Release manager, 26 Nov 2026: a light, read-only review of Sokki in Robin's daily use on 27, 28 and 29 Nov at about 18:00 (installed version, dictation cleanup, stop-to-paste wait, summary preload and times, call audio, instant paste). Numbers only, no transcript text. Sources: `~/Library/Logs/org.sokki.app/sokki.log` and its rotated files (UTC), `history.db` table `transcription_history`, `meetings/index.json`, `ffmpeg volumedetect` on the meeting tracks.

Baseline (26 Nov, 0.2.17 installed 12:50): schema 10, cleanup on, Gemma 4 E4B + MTP; last dictation 25 Nov 22:57; last summary 26 Nov 09:31 without preload, 17.3 s. Lab expectation for the preload (`live-pieces-and-summary-preload-2026-11.md`): summary step 8.4–9.5 s on the 2-minute sample call. Note: Robin's Mac has 34 GB (log), so the preload needs 13.6 GB free and no memory pressure, not the 9.6 GB of a 16 GB Mac.

## Day 1 — 27 Nov 2026, 18:00 (window 26 Nov 12:50 – 27 Nov 18:00 CET)

**Version.** 0.2.40 installed 27 Nov 17:53. 23 releases since 0.2.17 (0.2.18 on 26 Nov 15:40 brought instant paste). Settings schema 10 → 11 (26 Nov 18:02) → 12 (26 Nov 20:56).

**Dictations.** 1 new (26 Nov 17:52, 83 s audio, 223 words, before 0.2.18, so no instant paste).
- Cleanup ran: 8 live steps + 1 at the stop (9 words left); one live rollback ("live text changed; 31 words are cleaned again").
- Stop → pasted: 4.0 s (transcription 1.77 s, last cleanup step 1.58 s, paste 0.24 s).
- Changes in meaning: 1 sentence dropped (a hedge of 8 words), 1 nuance word dropped ("doch"). No list lost. 1 correct word fix. `target_app` empty.

**Instant paste.** 0 instant pastes in the log (no dictation since 0.2.18). Nothing to judge yet.

**Meetings.** 8 new in the index, all status done; 2 more were dropped as "no speech" (122 s, 44 s, 6 s: 3 dropped in total), 1 (26 Nov 22:33, 2.5 min) processed and mailed but no longer in the index.

| Start (CET) | Length | Online | Preload | Tokens | Summary | Processing |
|---|---|---|---|---|---|---|
| 26 Nov 13:01 | 0.8 min | no | – | 170 | 6.1 s | 11.8 s |
| 26 Nov 13:44 | 12.1 min | no | loaded, 5.1 s | 3,798 | 19.8 s | 23.1 s |
| 26 Nov 22:33 | 2.5 min | yes | loaded, 5.3 s | 610 | 11.7 s | 15.3 s |
| 27 Nov 06:31 | 0.7 min | no | – | ~8 | failed (no JSON title), added 30 min later | 7.6 s |
| 27 Nov 10:08 | 16.8 min | yes | **never** (pressure 2 at 12–17 GB free, 31 tries) | 5,124 | 24.4 s | 29.0 s |
| 27 Nov 11:00 | 39.2 min | yes | after 5.5 min (pressure 2 before) | 9,733 | 37.7 s | 38.9 s |
| 27 Nov 13:53 | 4.9 min | yes | loaded, 5.3 s | 35 | 4.0 s | 4.2 s |
| 27 Nov 14:00 | 65.3 min | yes | loaded, 5.6 s | 21,803 (4 parts) | 126.2 s | 127.2 s |
| 27 Nov 16:26 | 42.5 min | yes | loaded, 5.6 s | 9,225 | 41.7 s | 42.3 s |

Summary time follows length (~3.8–5 s per 1,000 tokens with preload); the 8–10 s of the lab holds only for meetings of a few minutes. Memory warnings during preload: 7 (15–185 s each), none stopped the model.

**Call audio** (mean / max dB, system track): 10:08 −29.7/−0.1; 11:00 −20.9/0.0; 13:53 −37.3/−4.5; 14:00 −20.3/0.0; 16:26 −26.5/0.0. None silent. But the tap delivered only zeros and was rebuilt: 13:53 call **~281 of 295 s** (192.4 s + 88.9 s while Chrome played; summary input 35 tokens), 11:00 call ~93 s (5 rebuilds), 10:08 call ~9 s, 16:26 call ~17 s (5 rebuilds while Slack played), 26 Nov 22:33 call ~11 s.

**Errors and warnings** (no ERROR lines, no panics): summary JSON failure 1; tap rebuilds 21; "falling behind" live transcript 1; 1 xrun; SecureInput fallback 2 (26 Nov 13:30); meeting prompt not shown 9× until 27 Nov 11:00 (X17, fixed in a release that day; none since).
