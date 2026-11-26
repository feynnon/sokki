# Meeting transcript on the phone: time and battery (Pixel 7, 26 Nov 2026)

- **Date:** 26 Nov 2026.
- **Task:** session keyboard-improve, for the Play Store launch (card
  android-play-store). Option A of the phone app (no Mac: the phone writes
  the transcript itself, `mobile/src/phoneTranscript.ts`, removed the same
  day) on a real phone.
  How long does a long recording take, and what does it cost in battery?
- **Result:** 30 minutes take 12.6 minutes (2.4× real time) and about 7 % of
  the battery (296 mAh), with 3 % word errors on clean voices. The
  transcript only runs while Sokki is on screen (KNOWN_ISSUES P1).
- **Decision:** Robin, 26 Nov 2026: "meeting transcription never happens on
  the phone - just too battery intensive". The phone records; without a Mac
  it says "Connect your Mac for a transcript". Streaming to the Mac is not
  needed either.
- **Sources:** our own measurement on Robin's Pixel 7 (below). Battery
  capacity 4,355 mAh: Google's Pixel 7 spec sheet.

## Setup

- Phone: Pixel 7 (Tensor G2), Android, unplugged, screen on (kept awake by an
  injected Shift key every 45 s), Wi-Fi on, nothing else in front.
- App: debug build of `meetings` 56345989 as `org.sokki.app` (Rust and C++
  dependencies at opt-level 3), installed next to the everyday app.
- Model: Nemotron Streaming 3.5 0.6B Q8_0 (751 MB), the Dictate tab's model.
- Audio: 30:00 of a made-up meeting (five macOS voices, 16 turns repeated,
  0.6 s pauses), WebM/Opus 32 kbit/s, added with the debug hook
  `window.__verbatim.add` instead of the microphone.

## Result

| What | Value |
|---|---|
| Model load (once per app start) | 7.5 s |
| 30:00 recording to "Transcript ready" | 754 s (12:34), 2.4× faster than real time |
| Battery | 296 mAh (charge counter 692 → 396 mAh), 17 % → 8 % |
| Battery temperature | 20.6 °C → 32.4 °C |
| Words written | 5,510 |
| Word error rate against the script | 3.0 % (lower case, no punctuation) |

- Battery: 296 mAh is about 7 % of the Pixel 7's 4,355 mAh. The percentage
  shown dropped more (9 points) because the phone started at 17 %, where the
  gauge reads less linearly. The screen being on is part of the cost.
- The word error rate is with clean synthetic voices; real rooms will be
  worse. It shows the long-recording path loses nothing on the way (no
  dropped or doubled 5-second blocks).
- The run decodes the whole file first (WebM → 16 kHz in the WebView), then
  pushes 5-second blocks to the streaming engine.

## App not on screen

Same app, a 3:00 clip of the same audio; Home pressed 5 s after adding it.

| What | Value |
|---|---|
| In the background (launcher in front, screen on) | 0 words after 6 min; Android reports the process `isFrozen=true` |
| Sokki opened again | done 59 s later (2,866 characters) |

Locking the phone has the same effect (the first 30-minute attempt stopped
when the screen went off after 10 minutes). Fix idea in KNOWN_ISSUES P1.
