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

- **Off for everyone**, no switch in the window: `meetings.auto_start_enabled` set by hand, on Robin's Mac only.
- **Start only on a known voice:** at least 12 s of speech in the last 30 s, and at least 2 of its 3 s speech pieces match a voice Sokki knows (cosine ≥ 0.5 to an embedding in `voice-profiles.json`: the user's own, or a person who agreed). The user alone counts (Robin, 2026-11-26: record "when only I am speaking"). Strangers' voices (a video, a podcast, a TV in the room) don't start it. First built as "two different voices"; Robin changed it the same day.
- **Not on media:** no start while any app plays sound (Core Audio `kAudioProcessPropertyIsRunningOutput`, per app, Sokki left out). Sound plus speech on the mic is most likely a video, music or podcast. With headphones nothing reaches the mic, and the check doesn't matter.
- **Not on calls:** a call app on the microphone keeps the existing click-to-start prompt; the listener stays out.
- **Not during a dictation.**
- **Nothing lost at the start:** the last 35 s are kept in memory only and become the meeting's first seconds.
- **Stop by itself** after 2 minutes without speech on either track (Granola: 15 min; ours is shorter because there's no call to end it).
- **False start cheap:** a card (canvas version 1, Robin's pick) shows for 20 s: Discard (deletes audio and record, nothing is learned from it, then 15 minutes without auto start), Keep, and "Turn off auto start" (switches the setting off; the recording goes on).
- **No Bluetooth mic:** the listener uses the built-in microphone, so AirPods keep their sound quality.
- **Logged for measuring:** every start, stop, discard and rejected candidate in `sokki.log` with reason and timing ("Auto meeting: …"), never what was said. Counts `auto_meeting_started` / `auto_meeting_discarded` / `auto_meeting_turned_off` in the daily usage report.
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
