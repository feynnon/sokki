# Sokki: developer guide

Sokki is a fork of [Handy](https://github.com/cjpais/Handy) (MIT) that adds
meeting notes to its dictation: record a meeting, and get a Markdown note
with speakers, a summary, decisions and action items. Everything runs on
the Mac, unless you choose a cloud language model (see 7). This file covers
the meeting pipeline, code layout and tests; the [README](README.md) is the
user-facing overview.

The app has its own bundle identifier (`org.sokki.app`), so it never shares
settings with an installed upstream app. Data stored under the identifier it
had before (2026-11-23) moves over on the first start
(`src-tauri/src/legacy_data.rs`). Updates come from Sokki's own feed
(`github.com/feynnon/sokki`).

## Look

Own name, V-wave app and menu bar icons, a calm blue on dark grey. Files:
`src-tauri/brands/sokki/` (icon sources and set),
`src-tauri/resources/brands/sokki/` (menu bar icons),
`src-tauri/src/brand.rs`, `src/lib/brand.ts`, `src/styles/theme.css`,
`src/components/icons/SokkiLogo.tsx`. Some screens still carry branches for
the upstream look (`IS_SOKKI` is always true); they go once the main window
work has landed.

Sokki also has a simpler window, after Superwhisper: sidebar Home,
Meetings, Vocabulary | Settings, Sound, Models; descriptions under each
option instead of info icons; no footer bar; no menu bar icon (the meeting
pill shows recording); first start is permissions, then the model. Options
not shown keep their defaults and stay in the config file
(`src/components/settings/sokki/`). The Vocabulary list goes only into the
meeting summary prompt: the direct dictation word swap turned „Lied“ into
„Lead“ and „Mach“ into „Nuxa“ with these words.

Icons are drawn as SVG in
`src-tauri/brands/sokki/src/`; after a change: `rsvg-convert` to PNG, then
`bun run tauri icon src-tauri/brands/sokki/src/icon-1024.png -o src-tauri/brands/sokki/icons`.

## What it does

1. **Dictation** – as in upstream Handy.
2. **Meeting recording** – shortcut ⌥M, tray item, the Meetings page, or
   automatically (see 3). A small pill at the top of the screen shows a
   waveform, the time and a stop button.
   - Two tracks: microphone and system audio (the remote side of a call on
     this Mac). Without system audio (no permission, no call) the meeting
     is mic-only; this is detected automatically.
   - **"Sokki hears nothing"** (`meeting/silence.rs`; switch
     `meetings.silence_hint_enabled`, on): each track also feeds a small
     voice detector (Earshot). After 40 s without speech on the microphone
     (and none on the call either, or a microphone sending only exact
     zeros), a strip above the recording bar names the microphone and
     offers "Switch microphone" (`switch_meeting_microphone`: the meeting
     moves to it within a second). The same strip says "Sokki hears nothing
     from {app}" when a detected call brings no sound at all for 40 s. Each
     at most once per recording; for the microphone also a macOS notification
     ("Sokki hört dich nicht – Nimmst du mit dem richtigen Mikrofon auf?") when the window
     isn't in front. Listening quietly while others speak on the call
     shows nothing. Why: on 2026-11-25 a whole meeting was recorded with a
     muted microphone and only the flat level bars showed it.
   - Recording code from [Mandy](https://github.com/yuri-research-ai/Mandy) (MIT).
3. **Meeting detection** – when Zoom, Teams, Meet in a browser, FaceTime,
   Slack, … start using the mic, a dialog asks whether to record. When the
   call ends, the recording stops after 20 s unless you say otherwise.
   Approach from [Lark](https://github.com/bkoleo/lark) (MIT).
   - **Notice in the meeting chat** (`meeting/chat_notice.rs`, crate
     `meeting-chat`; switch `meetings.chat_notice_enabled`, off by default):
     when a recording runs during a detected call, Sokki posts one line
     into the call's chat, once per call, from the user's account: Zoom
     and Teams apps, Google Meet in Chrome, Arc, Edge and Brave. Through
     Accessibility: find the meeting window (it has a "leave" button),
     find the chat field (opening the chat with its button or shortcut),
     bring the app forward for a moment, set or type the text, Return,
     give the front back. Keys go to the meeting app's process only. Then
     Sokki watches that chat; a message that is just "stop"/"stopp" ends
     the recording and deletes it (`discard_meeting`), Sokki answers in
     the chat and shows a banner. If the line can't be posted within 2 min,
     a banner says so. Why opt-out by chat: § 201 StGB accepts staying in
     a call after a clear notice of recording as consent (GDD "KI-Protokoll"
     2026, § 5–6); under the GDPR the objection must work in practice
     (BayLDA report 2025, 13.1), and Sokki can't take one voice out of the
     mixed call audio, so it deletes all of it.
   - **Notes to everyone** (`meeting/mail.rs`, crate `meeting-mail`,
     calendar bridge `swift/calendar.swift`; button "Send to everyone" on
     the note, switch `meetings.mail_notes_auto`, on by default since
     2026-11-25, schema 8 turns it on once for older installs): the
     note's summary, decisions and action items (no transcript, ending with
     the note's Sokki line) go to the people who were there (below),
     from the user's own mailbox in their name: Gmail when connected
     (Settings › Notes by mail › Connect Gmail: Google's consent page,
     only `gmail.send` + `openid email`, loopback + PKCE, refresh token in
     the Keychain, Disconnect revokes; client id/secret at build time as
     `SOKKI_GOOGLE_CLIENT_ID`/`_SECRET`), else Apple Mail. The event is the one that
     overlaps the recording most, among those with a link for the call app
     (Zoom app → zoom.us, a browser → any web call). Left out: the user
     (calendar "me" and Mail's own addresses), people who declined, rooms,
     and everyone on the objection list (`mail_objections.json`; a reply
     whose first word is "stop"/"stopp" puts the sender on it before the
     next send, BGH VI ZR 134/15). AppleScript makes the message with
     `visible:false` and sends it; a Mail that was closed is quit again once
     the outbox is empty. Automatic only for meetings from the last day,
     once, and never while a meeting records: the calendar and Mail
     (Automation) prompts come up after the first meeting ends, not at
     start. A declined prompt, no event or nobody to send to skips the
     mail with one log line; only a send that failed puts a line on the
     meeting's card. After a Slack huddle (Slack connected) the people come from
     Slack instead of the calendar: everyone else in the huddle's
     `participant_history`, with their Slack addresses (`users.info`,
     scope `users:read.email`), through the same filters
     (`meeting/slack.rs::huddle_event`). Since 2026-11-26 (Robin) only
     people who were there get the notes: the named speakers with an
     address (their own from the name field or the Sprecher page, else the
     matching invitee's, `meeting_mail::recipients::address_for`, then kept
     on their profile) and, after a huddle, everyone in it. Invitees who
     didn't speak get nothing; the person never (their name, their
     addresses; not the calendar's "me" mark, which marks the colleague in a
     shared calendar). Test app: `SOKKI_TEST_CALENDAR=<events.json>` replaces the
     calendar, and nothing is sent unless `SOKKI_TEST_MAIL_TO` names the one
     address every mail then goes to.
   - **Notes into the Slack huddle's thread** (`meeting/slack.rs`, crate
     `sokki-slack`; switch `meetings.slack_post_huddles`, on after
     "Connect Slack" in Settings › Meetings): after a huddle, Sokki posts
     the summary with the Sokki line into the thread of the huddle's own
     message, from the user's account, with the transcript as a `.txt`
     file in the same message. Sign-in: Slack OAuth with PKCE (no client
     secret, no server), redirect `http://localhost:47821/slack`, user
     scopes only; the token sits in `slack_auth.json` (0600) in the data
     folder, refreshed when it runs out (12 h) and once a week at launch
     (a PKCE refresh token lasts 30 days). Which huddle:
     while a detected Slack call records, `users.profile.get` every 15 s
     gives `huddle_state_call_id`; the channel comes from the Slack Mac
     app's own log (`Created huddle_<channel> child window` in
     `webapp-console.log`, only its end is read), because the profile's
     `huddle_state_channel_id` stayed empty in a real huddle and
     `calls.info` fails for huddles (both tried 2026-11-24). Both are kept in the
     meeting folder's `slack.json`. When the note has its summary (also a
     summary added later), one `conversations.history` call (15 messages;
     apps outside the Marketplace get one a minute) finds the message with
     `subtype: huddle_thread` whose `room.id` is the call id; then
     `files.getUploadURLExternal` + `files.completeUploadExternal` with
     `thread_ts` and the summary as `initial_comment` (Markdown turned into
     Slack's mrkdwn). Once per meeting (`slack.json` records the post). A
     banner says when it's posted or why not. Only in builds made with the
     Slack app's client id (`SOKKI_SLACK_CLIENT_ID`, see
     `crates/sokki-slack/SLACK_APP.md`); releases leave it out until the
     app is publicly distributed. Test by hand without Sokki:
     `cargo run --example slack` in `crates/sokki-slack`.
4. **During the recording** (`sokki-engine/src/live.rs`): both tracks go,
   sample for sample as written to their files, to one worker at low
   priority. It lines them up, removes the echo from the mic as a stream
   (the same filter as step 5.1, bit-identical), cuts each track into
   8–25 s pieces at the quietest point and transcribes them one at a time
   with word times (one 41-minute call: 338 pieces, 3 min of GPU work over
   the call). Every 5 minutes it also finds the speakers of the last 6
   minutes of each track (Sortformer, as in step 5.2) and joins the
   windows into one set of speakers per track (`meeting-core/src/stitch.rs`):
   a window's speaker is the one it shares the 30 s of committed overlap
   with, else the closest voice (cosine ≥ 0.6), else new; the last 30 s of
   each window are left to the next. Three real calls: 99.2–100 % of the
   speech has the same speaker as diarizing the whole track, 99.8–100 % of
   transcript lines. Once a window's speakers are final, the turns its
   pieces can't cover (a piece across a change of speaker) are transcribed
   too, exactly as step 5.4 will ask for them: the run at the end finds
   them by their audio (`live::TextCache`) and gets the same text. At the
   stop the last pieces and the last speaker window of each track run side
   by side, and the run takes over the echo-free mic instead of filtering
   it again (`live::take_mic`). Speakers run on their own thread, so
   pieces keep flowing while a window is diarized. Between the 5-minute
   windows a provisional window every 2 minutes diarizes as if the call
   ended there; at the stop only the rest is left (at most ~3 minutes of
   audio). Measured on a simulated 13-minute call: the note 6.8 s after
   the stop (was 22 s with live speakers, 41 s without the live part), the
   app's CPU during the call 30 % on average (13 % without provisional
   windows). It keeps both tracks in memory while the
   call runs (~230 MB per hour each), as the run at the end would.
5. **Processing** after the meeting (measured: a 21-minute call is ready
   ~3 minutes after stopping):
   1. decode, align the two tracks; remove the remote voices from the mic
      track with WebRTC echo cancellation (AEC3, pure-Rust `sonora`,
      `meeting-core/src/aec.rs`, ~6 s per 20 min) so they are not found as
      an extra speaker on the mic;
   2. speakers per track: Sortformer on the GPU in a child process
      (`meeting/diarize_worker.rs`, ~20 s per 20 min of audio; CPU fallback);
      (merging voices that sound alike exists but is off, see
      `DiarizeConfig::merge_similarity`);
   3. drop mic turns that are acoustic echo of the system track;
   4. transcribe each speaker turn (Parakeet, the dictation engine, ~20 s); a
      turn that pieces from the recording cover takes its words from them
      instead, when each of those pieces holds that one speaker only
      (`pipeline::texts_from_pieces`; 41-minute call: 180 of 231 turns,
      76 s → 15 s; word errors on call1402's checked passages 7.4 % turn by
      turn, 7.2 % from pieces);
   5. text dedup, merge ghost speakers, names: on an online call the mic
      voice is you; on a single mic nobody is named; remembered voices by
      their embedding;
   6. summary, decisions, tasks with the local LLM (Gemma 4 E4B built in
      since 25.11.2026: llama.cpp's `llama-server` on the GPU, 12–33 s per
      summary on an M1 Pro, 22 s median, without the draft model; stopped
      right after the summary). Since 26.11.2026 it loads a minute into the
      recording when the Mac has the memory (`meeting/preload.rs`: a quarter
      of memory still free after its 5.6 GB, no memory pressure), with
      throttled disk reads, and then only waits (1 % of one core) until the
      summary takes it: the summary starts about 5 s sooner. Under lasting
      memory pressure it is stopped and loads for the summary as before; the
      dictionary is in its prompt, so names come out spelled right. The
      transcript stays as recognised: a separate correction pass cost a
      whole model round and was removed on 2026-11-20 (KNOWN_ISSUES R17);
   7. Markdown note in the notes folder.
6. **Name and list row** (`meeting/title.rs`, `meetings/meetingDates.ts`):
   the summary's title names the meeting. Built but switched off
   (`title::DURING_RECORDING`, on only in the test app): five minutes into
   the recording the language model names it once from what the live
   transcript has heard (built-in model in macOS's background band,
   `llm::open_with`), until the summary replaces that guess. Off because a
   model load during a call is not safe until the system-audio capture
   recovers from a dead tap (24.11.: 13 minutes of call audio lost under
   load). A title typed in the reader (click it) is kept for good and goes
   into the note's heading. A list row reads "24.11. · 10:00 · Google Meet ·
   34 Min." (date and time in the Mac's region, `date_format_locale`) and,
   on the right, who spoke besides you: initials of up to two named people,
   then how many more, every name in a tooltip.
   The place is the call app; for a browser the call service from its tab
   titles (`meeting-detect/src/macos.rs`, needs Screen Recording, else the
   browser's name), else "Online"/"In person" from the processed tracks.
   Test: `TITLE_AFTER=60 scripts/test-meeting.sh mic.wav system.wav 90`.
7. **Speakers** – rename "Speaker 2" in the app; with *Remember voice* the
   voice is recognised in later meetings (sherpa-onnx embeddings).
8. **Language model** – summary (5.6) and the provisional title (6). Chosen under
   Models → *Language model*, a list like the speech models: Gemma 4 E4B on
   this Mac, built in (the page downloads the model and its draft model,
   5.1 GB, checksum verified; dictation cleanup uses the same files) or
   through Ollama (started only by hand), or Claude / GPT through Anthropic, OpenAI or OpenRouter with
   an API key (the transcript then goes to that provider; the key is shared
   with dictation post-processing). No temperature is sent: every model runs
   at its own default. Other models can be set in the config file
   (`meetings.llm_provider`, `llm_model`, `cloud_model`). Request shapes are
   covered by tests against a fake server (`summary.rs`); a real-request
   check: `MEETING_CLOUD=anthropic MEETING_CLOUD_MODEL=claude-opus-5-5 MEETING_CLOUD_KEY=… cargo test -p meeting-core cloud_summary_smoke -- --ignored --nocapture`.
9. **Safety** – a memory guard stops the app above 6 GB while a meeting is
   processed (`meeting/memguard.rs`); recordings are closed at quit and
   processed at the next launch.
9. **Sokki line** – every summary ends with one short line that is both a
   small ad and the AI label: "Summarized by AI. Local, free, open source.
   Dictation app and meeting notetaker: sokki.org" (German note: „Von KI
   zusammengefasst. Lokal, kostenlos, Open Source. Diktier-App und
   Meeting-Notetaker: sokki.org“; the same on every computer; a cloud
   model's summary drops "local, free", it is neither; no summary, no
   line). It is the
   note's last line, and `split_note` puts it under the summary, so the
   app, the phone, a copy and the web page carry it. Only a supporter (at
   least €20 paid) can switch it off: `meetings.supporter_code` holds a code
   signed with Sokki's Ed25519 key, checked offline
   (`sokki_engine::supporter`); `scripts/supporter-code.mjs mint` makes one.
   Older notes keep their "Transcribed on my Mac with sokki.org".
   **Mails and chat posts Sokki sends to other people** carry the same line
   (Robin, 2026-11-24, accepting the risk: in Germany an ad in an e-mail
   needs the recipient's prior consent, UWG §7 Abs. 2 Nr. 2, and the product
   counts as the advertiser, BGH I ZR 208/12). They take it from
   `Settings::sokki_line_text`, and whoever objects gets no more mail
   (BGH VI ZR 134/15).

## First meeting note

So that the first meeting gets a summary without anyone finding Models →
Language model:

- **Setup** (`src/components/onboarding/screens/Summary.tsx`): when Meetings
  is picked, a screen after the speech model offers the built-in summary
  model with its size, the memory it needs (5.6 GB resident, measured with
  Gemma 4 E4B and its draft model in Sokki's llama-server) against the Mac's (`summary_model_setup`), and when notes arrive
  (about 3 minutes after hanging up). It downloads in the background while
  setup goes on; one download at a time (`meeting/pull.rs`), which the
  Models page joins. A Mac under 12 GB (8 GB in practice) is told it's too
  small and pointed to Ollama or an own key.
- **Gmail in setup** (`screens/Permissions.tsx`, `onboarding/useGmail.ts`;
  Robin, 2026-11-24): with Meetings, Permissions also offers Gmail,
  optional, after the macOS permissions, so Sokki can send everyone the
  notes after the meeting. The sign-in asks for sending only
  (`gmail_connect`: `gmail.send`, `openid email`); Google's notice beside
  Connect says exactly that. The mail before a meeting, its
  `gmail.metadata` sign-in and the § 201 notice on going on without Gmail
  were dropped (Robin, 2026-11-25: "too spammy"). A build without
  `gmail_status` has no Gmail row.
- **Sample meeting** (`meeting/sample.rs`, `meetings/SampleMeeting.tsx`):
  on an empty Meetings page, a two-minute call between three invented
  people (`resources/sample-meeting/call.flac`, from
  `testdata/sample_meeting/`, spoken by Kokoro-82M, Apache-2.0; macOS's
  voices may not be redistributed). It plays out loud and runs through the
  real pipeline in real time: the call in place of the system audio, a
  silent stand-in microphone, the live transcript on screen, then speakers
  and summary. Marked as a sample: not in "Your data" or the milestones,
  no chat notice, and a real meeting started meanwhile replaces it.
  Headless test: `scripts/test-sample.sh`.

## Other people's voices (Robin's decision, 2026-11-24)

How Sokki treats the voices of the other people in a meeting. Robin decided
this after reading up on German law (§ 201 StGB, GDPR). It is not legal
advice, and the second point is not built yet.

1. **Sokki tells people.** In every online meeting format, the others learn
   that Sokki records:
   - a line in the meeting chat when recording starts; anyone who types
     "stop" stops Sokki and deletes the recording (branch `chat-notice`,
     Sokki Board card "Hinweis im Meeting-Chat");
   - dropped (Robin, 2026-11-25, "too spammy"): a mail to outside guests
     before the meeting.
   - In a room, the app asks the user to tell everyone.
2. **Only your own voice is kept.** During the meeting Sokki saves both
   tracks (`mic.flac`, `system.flac`), because transcription and speaker
   detection need the whole audio (live transcription measured worse, F3).
   Once the note is written:
   - the system track (everyone else on the call) is deleted;
   - only the microphone track, your own voice, may stay
     (`meetings.keep_audio_days`).
   - Today `keep_audio_days` keeps or deletes both tracks together
     (`process.rs`, `apply_retention`).
3. **Others' voices only with their OK.** Keeping someone else's voice
   needs that person's OK. Sokki can't yet tell who said OK, or take one
   voice out of the call sound, so for now it keeps no one else's voice.
4. **Phone calls: on the Mac yes, on Android not possible.**
   - Mac: an iPhone call taken on the Mac (Continuity) runs through
     FaceTime's call service (`avconferenced`), so Sokki asks to record it
     like a FaceTime call (Robin, 2026-11-25). macOS doesn't tell other
     apps which kind of call it is (tested 2026-11-25: `callservicesd`
     gives call state only to Apple's own apps).
   - A phone call has no chat for Sokki's notice: you tell the other side
     yourself.
   - Android gives no app the call audio (KNOWN_ISSUES X10), so a call
     can't be recorded on the phone. The ways around it (rooting the
     phone, the Mac posing as the phone's Bluetooth headset) were dropped
     (Robin, 2026-11-24). Details: `mobile/README.md`.

Why this is enough for Robin:
- § 201 StGB punishes saving someone's non-public words without their OK.
  It is prosecuted only if that person files a complaint.
- Deleting the file after the meeting doesn't undo the saving. The notice
  is what covers it: someone who stays and talks after a clear notice that
  they are being recorded agrees by staying. Lawyers write this; no court
  has ruled on it.
- So the notice should say "recording", not only "taking notes".
- GDPR applies to work meetings in any case, so people are told even
  though Sokki keeps none of their voice.
- Robin weighs the remaining risk as small against what Sokki is useful
  for.

Open, not covered by this decision:
- On a call through speakers, the mic track also holds the others' voices
  (echo). Keep it only after echo removal (`meeting-core/src/aec.rs`). In a
  room, the microphone hears everyone.
- A meeting with screen keeps the call sound in its `.mp4`.
- Remembered voices (`voice-profiles.json`) store other people's voice
  embeddings: only after the user ticked that the person agrees (GDPR
  Art. 9(2)(a), Art. 7(1): the date is kept), and dropped after a year
  unused. See "Speakers with real names".

## Speakers with real names (board card speaker-names, 2026-11-25)

Goal (Robin): every speaker named in transcripts and summaries.

- **Sprecher page** (sidebar, design C): every voice across meetings,
  grouped by voice (`sokki_engine::voices::group`, average-link at the
  voice-profile threshold 0.6, never two speakers of one meeting, never two
  names). On Robin's 46 meetings the user's own voice landed 16 of 17 times
  in one group. A voice heard in one meeting only is named there. Each
  voice: what it said (three lines, each playable), a suggestion (the
  invitee who was in most of its meetings), name, email addresses, consent.
- **In a meeting**: unnamed speakers show what Sokki proposes ("Lena oder
  Jonas?"): the name the same voice has elsewhere, then who was invited
  (`invitees.json`, from the calendar). One click names it; when that
  leaves one unnamed speaker and one invitee, Sokki names that one too
  (`voices::propose`, `left`). Also right after processing, before the
  note is mailed (a 1:1 call is named without a click).
  The chip opens a small card hanging from it, over the note: the
  proposals, "Jemand anderes …" for a name and email addresses, and
  "Benennen" at the bottom right; Enter names, a click outside or Esc
  closes it (Robin chose this, 26 Nov 2026).
- **Keeping voices** (in a meeting): naming asks nothing about the voice.
  Once every speaker has a name, Sokki asks once "Künftig von selbst
  erkennen?" with an unticked box per person whose voice it doesn't know
  yet, also the one it named by itself (`voices::voice_offer`). "Merken"
  keeps the ticked voices with their consent date; "Nicht jetzt" or
  "Merken" ends the question for that meeting (`voice_offer_done` file in
  its folder). Robin chose this over a box next to the names (26 Nov 2026).
- **Naming** changes the note (transcript, summary, tasks: generic
  "Speaker N" is replaced everywhere), `speakers.json` and the list. Sent
  mails and Slack posts stay as they were.
- **People** (`voice-profiles.json`): a name, several email addresses
  (invitees are named after the known person with that address), and a
  voice only with consent. Names from the guest list alone keep no voice.
- **Privacy**: voice groups are computed when asked and never stored; a
  voice is kept only with consent; nothing of this leaves the Mac or goes
  into "Help improve Sokki". Legal research and sources: side-session notes
  `speaker-names.md`.
- **From the call app** (`meeting/call_speakers.rs`, `meeting_chat::SpeakerWatch`):
  while a call is recorded, Sokki reads who is speaking twice a second:
  Meet's captions (Sokki turns them on once with their own button; each
  line starts with the name, "Ich"/"You" for the user) and Zoom's video
  tile ("Name, …, Active speaker"; Zoom marks nobody while only one person
  is in the call). Spans go to `call_speakers.json`; after processing,
  `voices::names_from_call` gives an unnamed speaker the name that covers
  more than half of its time (at least 3 s), first of all naming steps.
  No voice is kept for these names. Live in a two-person call: not yet
  checked (KNOWN_ISSUES P6).
- **Not yet**: Teams, names said in the meeting, Slack huddle members. See
  KNOWN_ISSUES P6–P8.
- Tests: `cargo test voices` (sokki-engine), `cargo test --lib profiles`
  (diarize), `tests/speakers.spec.ts` (Playwright), and a Sokki Test run
  with made-up voices: `CALENDAR=events.json NAME_SPEAKER="Speaker 2=…"
  KEEP_VOICES=all KEEP_MEETINGS=1 scripts/test-meeting.sh mic.wav system.wav`.

## Meeting with screen

A meeting can also record its screen: "Meeting with screen" under the arrow
next to Start meeting, or "Record screen too" in the call window
(`meeting/prompt.rs`, `src/meeting-prompt/`; it replaced macOS's own dialog,
which could not hold a switch and was German only). The screen is recorded
in full with ScreenCaptureKit (`src-tauri/swift/video_capture.swift`, Rust
side `src-tauri/src/video/`); Sokki's own windows stay out of it. After Stop,
the screen video and the meeting's sound become one `.mp4` in
`~/Movies/Sokki/<date time>/`; the meeting's row shows a screen button that
reveals it in Finder. Deleting the meeting moves that folder to the Trash.

Without Screen Recording permission, macOS is asked and the meeting is
recorded without the screen (from the call window) or not started (from the
Meetings page, which then says how to allow it).

Tests: `SOKKI_VIDEO_FIXTURES=<dir with screen.mov, mic.flac> cargo test --lib
video::` renders the screen video with sound from sample files.
`SOKKI_TEST_SCREEN=<video file>` skips capture and copies that file in at Stop.

The Videos page (camera, recording bar, looks) was removed on 2026-11-23
(Robin: "Videos raus, Bildschirm bleibt"); its last state is on branch
`video` (commit 71e0eea6).

## Layout

| Path | What |
|---|---|
| `crates/sokki-engine` | One meeting in, one note out (`process::run`, shared by the Mac app and the Linux service); the meeting lifecycle state machine; language-model settings, status, the built-in model server, Ollama |
| `crates/sokki` | The Linux/Omarchy host: `sokki daemon` (PipeWire recording, call detection, dictation), the CLI and the processing worker. See ARCHITECTURE.md |
| `crates/meeting-core` | Platform-independent pipeline: types, pipeline, acoustic echo filter, text dedup, LLM summary (Ollama / OpenAI-compatible, map-reduce for long meetings), Markdown notes, speaker rename |
| `crates/diarize` | Diarization: Sortformer (default) and sherpa-onnx backends, speaker embeddings, voice profiles |
| `crates/meeting-detect` | Which processes use the mic (Core Audio), meeting-app matching, debounce watcher |
| `crates/meeting-chat` | Posting one line into a meeting's chat and watching it for "stop" (macOS Accessibility); `cargo run --example chat -- dump zoom` prints an app's tree |
| `crates/meeting-e2e` | Headless end-to-end harness: real models on synthetic fixtures, scored against ground truth; examples for real-call evals (`raw_transcript`, `summary_eval`, `diarize_once`; `live_eval`: transcript during the call vs after it, time and word difference) |
| `testdata/generate.py` | Builds the synthetic fixtures (macOS voices + ffmpeg) into `testdata/audio/` (gitignored) |
| `~/sokki-evals/real-calls/` | Real-call eval data (outside the repo, private; `docs/evals.md`): uncorrected transcripts, hand labels (34 misheard words, 37 words that must stay), dictionary |
| `scripts/correction_eval.py` | Scores speech models' transcripts (and, from the removed correction step, correction outputs) against the real-calls labels |
| `src-tauri/src/meeting/` | App side: capture (`mic.rs`, `system_tap.rs`, `flac.rs`), `process.rs`, `store.rs` (meeting index), `detect.rs` |
| `src-tauri/src/commands/meeting.rs` | Tauri commands and the `MeetingChanged` event |
| `src/components/settings/meetings/` | Meetings page |

Data on disk:
- meeting audio + index: `~/Library/Application Support/org.sokki.app/meetings/`
- voice profiles: `…/org.sokki.app/voice-profiles.json`
- diarization models: `…/org.sokki.app/meeting-models/` (downloaded on first use)
- notes: the folder set on the Meetings page (default `~/Documents/Sokki`)

## Tests

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" CMAKE_POLICY_VERSION_MINIMUM=3.5
python3 -I testdata/generate.py                      # once: synthetic fixtures

(cd crates/meeting-core   && CARGO_TARGET_DIR=/tmp/hm-target-core   cargo test)
(cd crates/meeting-detect && CARGO_TARGET_DIR=/tmp/hm-target-detect cargo test)
(cd crates/sokki-engine   && CARGO_TARGET_DIR=/tmp/hm-target-engine cargo test)
(cd crates/diarize        && CARGO_TARGET_DIR=/tmp/hm-target-diarize cargo test --release)
(cd crates/meeting-e2e    && CARGO_TARGET_DIR=/tmp/hm-target-e2e    cargo test --release -- --nocapture --test-threads=1)

# with live summaries from local Ollama
(cd crates/meeting-e2e && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-e2e cargo test --release -- --nocapture --test-threads=1)
(cd crates/meeting-core && MEETING_LIVE_LLM=1 CARGO_TARGET_DIR=/tmp/hm-target-core cargo test --test live_llm -- --nocapture --test-threads=1)
```

The e2e harness needs `~/.cache/sokki/models/parakeet-tdt-0.6b-v3-Q8_0.gguf`
(see `crates/meeting-e2e/src/lib.rs`). It prints, per scenario, word error
rate, speakers found, speaker attribution and leftover echoes, and writes the
notes to `$TMPDIR/hm-e2e-notes/`.

To add a real recording as a test case: put `mic.wav` (and `system.wav`)
as 16 kHz mono WAV plus a `truth.json` (see an existing scenario) into a new
folder under `testdata/audio/`, and add its name to the scenario lists in
`crates/meeting-e2e/tests/pipeline_e2e.rs`.

### Real-call evaluation (transcripts)

```sh
# transcript of a recording (same pipeline as the app)
(cd crates/meeting-e2e && cargo run --release --example raw_transcript -- mic.wav system.wav ~/sokki-evals/real-calls/data/<call>/raw.json)
```

Count only first runs for timing: Ollama reuses cached prompt parts on repeats.

### Transcript during the call

```sh
# one recorded meeting: time after the stop and word difference, today vs live
# (prints numbers only; transcripts go to the out dir, keep it outside the repo)
(cd crates/meeting-e2e && MEETING_ASR_MODEL=<parakeet-ultra-Q8_0.gguf> cargo run --release --example live_eval -- <meeting dir> /tmp/live-eval)
# a simulated call in the test app, in real time, with CPU and memory (NO_LIVE=1 for the A/B)
ffmpeg -i <meeting dir>/mic.flac -ar 16000 -ac 1 /tmp/mic.wav     # likewise system.flac
scripts/test-meeting.sh /tmp/mic.wav /tmp/system.wav
```

## Building and installing

```sh
scripts/build-and-install.sh          # build, then install when no meeting is busy
scripts/build-and-install.sh --no-build
```

The script signs with the code-signing certificate **"Sokki Dev"**
from the login keychain when it exists, so macOS keeps the granted
permissions across installs. Create it once (Keychain Access → Certificate
Assistant → Create a Certificate…; Name "Sokki Dev", Identity Type
"Self-Signed Root", Certificate Type "Code Signing"). Without it, builds are
signed ad hoc and every install asks for all permissions again.

Manual build:

```sh
bun install
bun run tauri build --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'
# → src-tauri/target/release/bundle/macos/Sokki.app
```

Needs Xcode Command Line Tools 26.x (Swift for the system-audio tap). With
Command Line Tools only, Apple Intelligence is built as a stub; meeting
summaries use Ollama instead.

## Keeping up with upstream

```sh
git fetch upstream
git merge upstream/main     # or a release tag
```

The meeting code lives mostly in new files, so conflicts should be rare
(expect them in `lib.rs`, `settings.rs`, `tray.rs`, `bindings.ts` and the
translation files).
