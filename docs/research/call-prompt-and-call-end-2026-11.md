# Call prompt and call end (2026-11-27)

**Task** (Robin, 2026-11-27): in a Slack huddle Sokki did not offer to
record; and after leaving a Google Meet the recording kept running. Find
why from the logs, fix both, test without a person.

## Prompt missing

- Source: `~/Library/Logs/org.sokki.app/` from 2026-11-24 to 2026-11-27 (0.2.30).
- After every launch only the first detected call got the prompt (24.11.
  Slack, 25.11. Zoom, 27.11. Chrome 09:37); every later one logged
  `Meeting prompt could not be shown: a webview with label meeting_prompt
  already exists`: 12 calls missed (1 on 24.11., 3 on 25.11., 1 on 26.11.,
  7 on 27.11.).
- Cause: the app-wide close handler (`lib.rs`) turns a close into a hide;
  the prompt's hidden window kept its label. Fix: destroy it (`meeting/prompt.rs`).
- Test: `scripts/test-meeting-prompt.sh` (4 prompts in a row, hidden
  window, test app): 1/4 shown before, 4/4 after.

## Recording runs on after the call

- The call-end stop existed only for recordings started from the prompt.
  Because of the missing prompt, both of today's calls were started by hand
  in Sokki and ran on:

  | Call (2026-11-27, local time) | Watcher "Ended" | Stopped by hand | Late by |
  |---|---|---|---|
  | Slack huddle, 10:08–10:25 | 10:24:59 | 10:25:48 | 49 s |
  | Google Meet in Chrome, 11:00–11:39 | 11:38:32 | 11:39:55 | 83 s |

- "Ended" comes 20 s after the app lets go of the mic (`end_after`,
  `crates/meeting-detect/src/watcher.rs`), so a reconnect or a Meet tab
  switch doesn't end the call. Then the "call ended" window (Keep recording /
  Stop) waits 20 s and stops without an answer: about 40 s from leaving to
  stopped, one click keeps it. Not tuned on real reconnects yet: today's
  logs hold no reconnect within a call (gaps between Chrome calls were 32 s
  and more, and each was a separate call).
- Fix: any recording that starts while a call is detected belongs to that
  call (`meeting/detect.rs`, `recording_started`). One started before the
  call keeps running (it may be an in-person meeting).
- Test: `scripts/test-call-end.sh` (simulated Zoom call, test app). Before:
  started by hand, still recording 30 s after the end. After: hand and
  prompt stop 20.2 s after "Ended"; "Keep recording" keeps it.
