# Phone meetings sent to the Mac while they record (27 Nov 2026)

- **Date:** 26–27 Nov 2026.
- **Task:** session keyboard-improve. Robin, 26 Nov 2026: "The meeting transcript, speakers, summary might all be available in very short time after a meeting ends. Sounds worth it to me. Please build it, then let's monitor how this behaves." Until then the phone sent the whole recording after the stop.
- **Result:** the phone sends its recording to the Mac in 5-second pieces while it records. The Mac decodes them as they arrive into the same live transcript as its own recordings. On a 3-minute test meeting, transcript and speakers were ready 2 s after the last piece arrived (7 s after the stop). The note followed 35 s after the stop, 23 s of that the summary, whose model isn't preloaded for phone meetings yet (KNOWN_ISSUES P4). Done afterwards, speakers and transcript took 9.2 s for the same 3 minutes; that part grows with the meeting's length, so longer meetings gain more.
- **Sources:** our own runs (below): Android emulator (Pixel 7 image) against the Mac test app built from this branch, on Robin's Mac (M1 Pro, 32 GB), and against a stand-in Mac (`crates/sokki-link/examples/meeting_mac.rs`). The Mac's own live transcript, measured earlier: `live-pieces-and-summary-preload-2026-11.md`, `MEETINGS.md` (13-minute call: note 6.8 s after the stop with the live part, 41 s without).

## How it works

- Phone (`mobile/src/App.tsx`, `mobile/src-tauri/src/link.rs`): MediaRecorder writes WebM/Opus at 32 kbit/s, a piece a second. Every 5 s the bytes the Mac doesn't have yet go over (`LivePart {id, offset}`), one request on its own connection each, with 12 s at most per request. At the stop, the rest goes over and `LiveEnd {total}` starts processing.
- Mac (`src-tauri/src/meeting/phone_live.rs`): appends the pieces to `mic.phone` and decodes them as they come (`decode_stream`, symphonia + libopus) into `meeting::live::Live`, single track. The meeting shows in the list only at the stop.
- Fallbacks: an older Mac refuses `LiveStart`; a Mac that restarted answers "No such live meeting". In both cases the phone sends the whole recording after the stop (`Upload`), as before. A Mac that is briefly unreachable gets the missing bytes from the byte it reports.

## Streamed decoding gives the file's samples

| File | Pieces | Samples streamed vs whole file |
|---|---|---|
| ffmpeg WebM/Opus, 1:00 | 20,000 and 777 bytes | identical (2,880,648) |
| ffmpeg WebM/Opus, 30:00 | 20,000 bytes | identical (28,800,216) |
| Chromium MediaRecorder WebM (as Android's WebView writes it), 0:20, 5-s pieces | 20,000 and 333 bytes | identical (956,160) |

Unit test with the MediaRecorder file through the live resampler: `a_stream_in_pieces_decodes_like_the_file` (`src-tauri/src/audio_toolkit/audio/decode.rs`).

## Real Mac pipeline, 3:00 made-up meeting (five synthetic voices)

Emulator → Mac test app (Parakeet Ultra, speakers on the GPU, Gemma 4 E4B summary).

| | Live (sent while recording) | Whole file after the stop |
|---|---|---|
| Bytes | 685 KB in 34 pieces, 0 out of order | 657 KB in one upload (1 s) |
| Live transcript | 173 s during the recording, 7 s left at the stop | – |
| Last piece in | 5 s after the stop | – |
| Speakers | 0.0 s (done live) | 5.7 s |
| Transcription | 2.1 s | 3.5 s |
| Summary | 23.2 s (model loaded at the stop) | 16.6 s (model still loaded from the run before) |
| Processing in all | 25.3 s | 25.8 s |
| Note on the phone after the stop | ~39 s | ~29 s plus the upload |

For 3 minutes, the summary model's load eats the gain. The live part saves what grows with length: here 7 s for 3 minutes. Not measured: a 10- or 30-minute meeting from the phone. The test hook's 10-minute file was too large for the command line; `cdp.ts` needs a subcommand that reads the file itself.

## Link behaviour on the emulator

- At 128 kbit/s (the browser's default) one minute is 971 KB. The emulator's relay path carried about 5 KB/s, so the pieces fell behind (260 KB left at the stop). At 32 kbit/s, one minute is 235 KB (~4 KB/s).
- A connection kept open for the whole meeting stopped carrying requests after the first (no answer for minutes, while a new connection answered in 16 ms). Each piece now goes on its own connection, like every other request.
- A freshly paired app sometimes got no answer to its first requests for a minute or more, although the stand-in had answered. With the 12 s limit and retries, the pieces start about 20 s late and catch up: in the first recording after pairing, the stand-in had everything 2 s after the stop (KNOWN_ISSUES P5).
- Stand-in paused for 25 s mid-meeting: the pieces stopped at 238 KB and caught up in one piece to 644 KB. At the stop the Mac had all of it.
- Stand-in restarted mid-meeting: the next piece got "No such live meeting". The phone recorded on and uploaded the whole 235 KB after the stop.
- Stand-in without live (an older Mac): whole upload after the stop.

## Not measured yet

- On Robin's Pixel: the relay and direct paths with a real phone, at home and over mobile data, and battery while sending.
- Real meetings (Robin, 26 Nov 2026: "let's monitor how this behaves"). The Mac logs each one: `Phone link: live meeting <id> complete: <s> s, <KB> in <n> pieces (<n> out of order), over <s> s`, then `Meeting processed in …`.
