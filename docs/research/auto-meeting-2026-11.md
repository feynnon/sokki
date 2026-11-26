# Meetings that start by themselves when people talk (Nov 2026)

**Date:** 2026-11-26. **Task:** build "start a meeting by itself when speaking is detected" on the Mac, off by default and on only on Robin's Mac, without being annoying like Wispr Flow (the reason Robin uninstalled it). Research first: how other apps start and stop by themselves and what users complain about. Board card `auto-meeting`. Code: `src-tauri/src/meeting/auto.rs`.

## How others do it

No desktop app we found starts recording because people talk. They start on the calendar, or when another app opens the microphone, and most show a prompt first. Only wearables listen for speech itself, and those collect the complaints about TV and songs.

| Product | Trigger | How it shows | Stop | Undo a false start |
|---|---|---|---|---|
| Wispr Flow dictation | shortcut only (hands-free mode) | pill | shortcut | press again within 0.5 s cancels |
| Wispr Flow Notetaker (beta 2026) | a call in a supported app (mic use alone isn't enough) | "Meeting detected" card; auto-start (off by default) records after ~10 s unless closed | call ends, silence, max length (2 h) | close the card |
| Granola | calendar (1 min before, 2+ attendees) or another app on the mic | its own pop-up, never records without a click | call app lets go of the mic, 15 min without audio, Mac sleeps | — |
| Otter desktop (Aug 2026) | mic becomes active | per app: off / remind (default) / record automatically | unverified | unverified |
| Circleback | any app opens the mic | countdown panel | unverified | stop in the countdown |
| MacWhisper | list of watched apps | notification, gone after 10 s | by hand | — |
| Notion AI notes | joining a call | reminder | meeting app gone and audio inactive | — |
| Plaud desktop | prompt by default | prompt | — | a stopped meeting doesn't restart |
| Krisp | its virtual mic used in a call app | — | call ends | exclude apps |
| Limitless, Bee (wearables) | always listening, speech | none | — | delete afterwards |
| Jamie, Bluedot, tl;dv, Fireflies, Superwhisper | call detection, details not documented (unverified) | | | |

## What users complain about (most often first)

1. Recording without the others knowing: Otter's bot joining every call led to a class action (Brewer v. Otter.ai, Aug 2025); press calls the wearables creepy.
2. False starts: "Meeting detected" for a quick video recording or a voice chat (Granola, Notion); Bee takes TV dialogue and songs for conversations.
3. Never stops, or two calls merged into one note (Granola when Zoom keeps the mic open).
4. Too many pop-ups.
5. Background music in the transcript.
6. CPU and memory (Wispr Flow).
7. AirPods sound muffled whenever an app opens their mic (Bluetooth call profile).

## What this means for Sokki (the design built)

- **On for everyone, but only with the user's own voice known** (since 2026-11-26 evening; first off for everyone and on only on Robin's Mac). Until a voice profile named like the user (`meetings.user_name`) has a voice, the microphone stays closed and nothing starts.
- **Start only when the user is in it:** at least 12 s of speech in the last 30 s, and at least 2 of its 3 s speech pieces match the user's own voice (cosine ≥ 0.5 to an embedding of that profile). Other people's voices, even known ones, don't count (Robin, 2026-11-26: "Only activate it if the owner is present"). The user alone is enough ("when only I am speaking"). First built as "two different voices", then "any known voice"; both changed the same day.
- **Listen in first, show later** (Robin, 2026-11-26 evening: "we don't bother showing a meeting recording unless we think: Yes, this is a meeting"): once a candidate passes, Sokki records the conversation into memory only, with nothing on screen and nothing on disk. After `meetings.auto_start_show_after_s` (60 s; Robin: "Show at 60 s"), if there are still 12 s of speech in the last 30 s and the user's voice is in it again, it becomes a meeting with everything so far as its first minutes. No speech for 30 s before that: dropped, nothing kept. A shown meeting whose conversation lasted less than `meetings.auto_start_min_talk_s` (2 min, from the listening-in) is deleted when it stops. Both durations can be set (`sokki config`).
- **Not on media:** no start while any app plays sound (Core Audio `kAudioProcessPropertyIsRunningOutput`, per app, Sokki left out). Sound plus speech on the mic is most likely a video, music or podcast. With headphones nothing reaches the mic, and the check doesn't matter.
- **Not on calls:** a call app on the microphone keeps the existing click-to-start prompt; the listener stays out.
- **Not during a dictation.**
- **Nothing lost at the start:** the last 35 s are kept in memory only and become the meeting's first seconds.
- **Stop by itself** after 2 minutes without speech on either track (Granola: 15 min; ours is shorter because there's no call to end it).
- **The card (since 2026-11-26 evening, canvas M2.2):** one line like a macOS notification, shown for 20 s: Sokki's mark, "Von selbst aufgenommen", an Auto-Start switch (switches the setting off; the recording goes on) and a round X over the top left corner that shows with the mouse and only closes the card. Buttons show the hand cursor. The pill shows that it records. First built with three buttons (Discard, Keep, "Turn off auto start"); after the first real start Robin found that too much and the text button didn't look like one, and picked M2.2 from 17 drafts.
- **False start cheap:** stopping an auto-started meeting in the pill means it wasn't wanted: no auto start for 15 minutes, so the same conversation doesn't start it again.
- **No Bluetooth mic:** the listener uses the built-in microphone, so AirPods keep their sound quality.
- **Logged for measuring:** every start, stop, discard and rejected candidate in `sokki.log` with reason and timing ("Auto meeting: …"), never what was said. Counts `auto_meeting_started` / `auto_meeting_stopped_by_user` / `auto_meeting_turned_off` in the daily usage report.
- Known cost: while on and idle, the microphone is open (macOS shows the orange dot all the time).

Not built (ideas for later, unverified): comparing system output with the mic (echo correlation) to tell a TV in the room from people; Now Playing (closed to third-party apps since macOS 15.4).

## Headless test (2026-11-26, Sokki Test, `scripts/test-auto-meeting.sh`)

Made-up audio only (macOS voices and synthesised music), played into the test microphone, never through the speakers. Known voice: one 8 s sample of the voice "Anna" (a sentence not in any test clip), the test's stand-in for the user's own voice. Build d53aca7e plus the test fixes.

| Case | Expected | Result | Detail |
|---|---|---|---|
| 1 Two people talk (Anna + Eddy), until it stops | start, stop | started, stopped | start after 16 s of listening, 2 of 3 pieces known, best similarity 0.92, check 896 ms, recording 76 ms later, 14.8 s from before kept; stopped after 120 s without speech (181 s recorded) |
| 2 One known person thinking aloud (Anna), then Discard | start, discard | started, discarded | 3 of 3 pieces known, 0.91; discarded 3 s after the start; the next 5 candidates refused ("discarded … ago") |
| 3 Video with two strangers (Daniel + Flo) | no start | no start | 0 known pieces in 5 checks, best similarity 0.16–0.29 |
| 4 Same video, an app plays sound | no start | no start | refused as "sound plays" 6× |
| 5 Known talk, an app plays sound | no start | no start | refused as "sound plays" 6× |
| 6 Music, no voice | no start | no start | the voice detector took 28–30 s of 30 as speech; best similarity −0.04–0.00 |
| 7 Song (sung voice over music) | no start | no start | best similarity 0.02–0.08 |
| 8 Noise only | no start | no start | never reached 12 s of speech |
| 9 Known talk, but no known voices stored | no start | no start | "0 known voice samples" |
| 10 Known talk, then "Turn off auto start" | start, off | started, turned off | setting off 3 s after the start, recording went on |

**False starts: 0 of 7 no-start cases. Missed starts: 0 of 3 start cases.** A check (speaker embeddings of 3–9 pieces) takes 0.5–2.4 s on a background thread, every 15 s at most while there is speech.

Findings:
- Earshot (the voice detector) calls synthesised music speech almost all the time, so speech length alone would have started on music. The known-voice check is what holds.
- Known vs. strange voices are far apart on these voices (0.91–0.92 vs. at most 0.29), so the 0.5 bar has room both ways. Not yet measured: the user's real voice on the built-in mic against a profile enrolled from a meeting. Robin's Mac has one stored sample of his voice; the log line "best similarity" on his Mac answers this.
- Not covered by the test: a real Bluetooth headset, a real app playing sound (the test pretends with `PLAYING`), a TV in the room with a known voice in it.

## Headless test of the one-line card (2026-11-26 evening, Sokki Test Auto)

Same made-up audio as above. The card itself was checked in headless Chromium (`tests/main-window.spec.ts`, "started by itself"): fits its 380 × 92 window in German and English, X hidden until the mouse is on the card, hand cursor on switch and X, the switch goes off at once and answers 0.7 s later.

| Case | Expected | Result |
|---|---|---|
| Known talk, then the Auto-Start switch | start, off | started (2 of 3 pieces known, 0.92, check 914 ms, recording 88 ms later); off 3 s later, recording went on |
| Known talk, then the X | start, card closed | started (0.92); card closed 3 s later, recording went on |
| Video with two strangers | no start | no start, best similarity 0.16–0.30 |

Not tested headless: stopping an auto-started meeting in the pill (then 15 minutes without auto start), and how the card looks in the real window over the desktop.

## Headless test: only the user's own voice, on by default (2026-11-26, 20:38)

| Case | Expected | Result |
|---|---|---|
| The user talks (the test's own voice) | start | started, 2 of 3 pieces the user's, 0.92 |
| No own voice known | no microphone | "waiting for the user's own voice; not listening" |
| Video with two strangers | no start | no start, best similarity 0.16–0.30 |

On Robin's Mac the voice profile named like `meetings.user_name` exists with one sample, so it keeps working there.

## Headless test: listening in, shown at 60 s (2026-11-26, 21:22)

| Case (made-up voices, the test's "own" voice) | Expected | Result |
|---|---|---|
| 40 s chat | dropped, nothing kept | listening in after 15 s of listening; dropped 54 s later (no speech for 30 s) |
| 78 s chat | nothing kept | shown 61 s into listening in (76 s kept from before); deleted at the stop: 62 s of conversation, under 120 s. The pill showed for about 2 min and went away |
| 4 min conversation | shown at 60 s, kept | shown 61 s in (76 s kept); stopped 120 s after the last speech; note done, 351 s of audio |
| Video with two strangers | never listens in | similarity 0.16–0.29, no listening in |

Seen: a conversation between about 60 s and 2 min shows the pill and then deletes the meeting. That is the trade-off of showing at 60 s while keeping only 2 min and longer.

## First real use (2026-11-26, Robin's Mac, 0.2.17)

Auto start on from 11:50 UTC, built-in mic, 2 stored voice samples (the user's and one person who agreed), no app playing sound.

- 11:54–12:01: 21 checks with 12–21 s of speech each, all refused. Best similarity 0.23–0.47 in 19 of them; twice 0.67–0.70, but only 1 piece of 3–5 matched (2 needed).
- 12:01:56: started on 13.4 s of speech, 2 of 4 pieces known, best similarity 0.64, check 579 ms, pill 220 ms later, 35 s from before kept (listening for 663 s).
- The meeting's speakers: the user 14.8 s, one unknown person 5.1 s, both on the mic (a conversation in the room).
- 11.8 s after the start the user pressed "Turn off auto start", then stopped the recording at 15 s. The 49 s note (35 s pre-roll + 14 s) was kept, not discarded. The user's reason isn't known yet.

Findings:
- The trigger did what it was built for (a known voice in a conversation in the room), and the user still didn't want the recording. The rule is: a known voice talking is not the same as a conversation worth recording.
- The user's real voice on the built-in mic scores 0.64–0.70 against one stored sample, against 0.91–0.92 for the synthesised test voices. Unknown speech scored at most 0.47. The 0.5 bar sits close to both.
- The user turned auto start off. It stays off on that Mac until the user decides otherwise.

## Sources

- Wispr Flow: https://docs.wisprflow.ai/articles/6391241694-use-flow-hands-free ; https://docs.wisprflow.ai/articles/9319084321-notetaker-settings-explained-beta ; https://docs.wisprflow.ai/articles/8533503284-knwon-audio-playback-airpod-issues-ios-macos ; https://www.producthunt.com/posts/wispr-flow/reviews
- Granola: https://docs.granola.ai/help-center/taking-notes/notifications.md ; https://docs.granola.ai/help-center/taking-notes/transcription.md ; https://www.producthunt.com/p/shadow-4/still-getting-meeting-detected-when-you-re-not-in-a-meeting
- Otter: https://otter.ai/blog/never-miss-the-conversation-introducing-automatic-recording-in-otter-desktop ; https://www.npr.org/2025/08/15/g-s1-83087/otter-ai-transcription-class-action-lawsuit
- Krisp: https://krisp.ai/speech-to-text-meeting-note-taker/
- Circleback: https://circleback.ai/releases/automatically-start-recordings-on-microphone-use
- MacWhisper: https://docs.macwhisper.com/article/30-record-meetings
- Superwhisper: https://superwhisper.com/meeting-transcription
- Notion: https://notion.com/help/ai-meeting-notes
- Plaud: https://support.plaud.ai/hc/en-us/articles/53793036250521-Automatic-Recording
- Limitless: https://feedback.limitless.ai/board/disregaurd-background-talking-from-tv-radio-crowds
- Bee: https://techcrunch.com/2026/05/24/i-tried-amazons-bee-wearable-and-am-both-intrigued-and-slightly-creeped-out/
- Core Audio: https://developer.apple.com/documentation/coreaudio/kaudioprocesspropertyisrunninginput
- Now Playing closed in 15.4: https://github.com/ungive/mediaremote-adapter
- Bluetooth call profile: https://forums.macrumors.com/threads/airpods-3-sound-muffled-when-mic-is-being-used.2332154
