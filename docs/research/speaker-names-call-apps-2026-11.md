# Speaker names: voices, calendar and call apps (Nov 2026)

How Sokki puts real names on meeting speakers. The code lives in `crates/sokki-engine/src/voices.rs`, `src-tauri/src/meeting/voices.rs`, `src-tauri/src/meeting/call_speakers.rs` and `crates/meeting-chat/src/mac.rs`. Each entry below gives its date, the question asked, the result and the sources. Re-check an entry once it is about six months old: call apps change their pages often.

## 1. Voices across Robin's meetings (25 Nov 2026)

**Question:** How many speakers are unnamed, and do voices group reliably across meetings?
**Method:** ERes2Net 192-dim embeddings from `speakers.json`, average-link grouping at a cosine threshold of 0.6. Speakers of the same meeting, and speakers already named differently, are never grouped.
**Result:**
- 46 meetings, 94 speakers, 74 of them still generic ("Speaker N").
- Robin's own voice landed in one group in 16 of 17 meetings.
- Two recurring unnamed voices showed up in 13 and 12 meetings, so naming each of them once names about 25 speakers.

## 2. Reading who speaks from the call app, via macOS Accessibility (25 Nov 2026)

**Question:** Without a bot or browser extension, can Sokki see who is speaking?
**Method:** Live solo calls, read through the Accessibility API from another process. Chrome needs `AXEnhancedUserInterface`.
**Result:**
- **Zoom (app):** each tile is an `AXTabGroup` labelled "Name, Computer audio unmuted[, Active speaker]". "Active speaker" only appears with two or more people.
- **Google Meet (Chrome):** the caption region is `[role=region][aria-label="Untertitel"|"Captions"]`. Each line is an avatar image, then the name, then the text, and the user shows as "Ich"/"You". It only exists while captions are on. Sokki presses the captions button once; only the user sees the captions.
- **Not confirmed:** a call with two real people. Meet refused a guest because of a Workspace policy, and Zoom's web client blocked the second guest with a reCAPTCHA. Both are still open in `KNOWN_ISSUES.md` (P6–P10).

## 3. Does Meet need captions on to show the speaker? (26 Nov 2026)

**Task given to the research agent:** "Can Sokki learn who is speaking in Google Meet without turning captions on, still only through macOS Accessibility from a separate process (no extension, no injected JS, no bot)? Check the tile speaking indicator in the AX tree, how open-source recorders detect speakers, and the official Meet APIs." This was desk research only; nothing was tested live.

**Result: captions are needed.** Nothing else is reliable from outside the browser.
- Meet marks the speaking tile only with obfuscated CSS classes on `[data-participant-id]` tiles (`Oaajhc`, `HX2H7`, `wEsLMd`, `OgVli` in Vexa's code). There is no ARIA label or live region. Vexa calls this "a known-bad foundation" and says it needs 3+ people and gets worse during screen sharing.
- Chrome on macOS does expose classes as `AXDOMClassList`, so Sokki could read them in principle. Whether the tiles appear in the AX tree at all is untested.
- Attendee and Recall.ai use JS injected into the page or per-participant audio streams. Neither works from outside the browser.
- Meet's REST API gives transcript entries (participant, start and end time) only after the call. It also needs Meet's own transcription and Business Standard or higher.
- Meet's Media API needs every participant in the Developer Preview, and the app joins the call as its own client, like a bot.

**Decision (Robin, 26 Nov 2026):** keep captions. Sokki turns them on while it records. This is described under Meeting settings › "Namen aus dem Call", where it can be switched off.

**Sources:**
- https://github.com/Vexa-ai/vexa/blob/main/core/meetings/modules/gmeet-capture/src/gmeet-speakers.ts
- https://docs.vexa.ai/speaker-identification
- https://github.com/attendee-labs/attendee/blob/main/bots/google_meet_bot_adapter/google_meet_chromedriver_payload.js
- https://chromium.googlesource.com/chromium/src/+/HEAD/content/browser/accessibility/ax_tree_formatter_mac_browsertest.mm
- https://docs.recall.ai/docs/google-meet-faq
- https://developers.google.com/workspace/meet/api/reference/rest/v2/conferenceRecords.transcripts.entries
- https://support.google.com/meet/answer/12849897
- https://developers.google.com/meet/media-api/guides/overview

## 4. Law on stored voices (25 Nov 2026)

**Question:** What does storing a voice need?
**Result:**
- A voice stored to recognise a person is biometric data under GDPR Art. 9. It needs explicit consent, and the date of consent is kept (Art. 7(1)).
- The consent box starts unticked (CJEU C-673/17, Planet49).
- Voices not heard for a year are deleted, in line with the time limits in BIPA and CUBI.
- Names taken from the call app or the calendar are not biometric.
- Open for a lawyer: whether the EU AI Act, Annex III(1)(a), covers this.
