# Own notes during a meeting (2026-11-26)

**Task:** Robin: "what I liked in Granola is that you could also use it as a
notetaker. And that it could enhance the notes." Find out how Granola,
Notion AI Meeting Notes, Apple Notes, Otter, Fathom and tl;dv let you type
your own notes during a meeting and merge them with the transcript, and
what users love and hate about it. Basis for Sokki's design (canvas "Sokki
app", page Offen, "Eigene Notizen im Meeting, vier Versionen").

Web research by an agent, 2026-11-26. Otter and tl;dv rest on secondary
sources; the user-opinion part is mostly second-hand (blogs, reviews, some
written by competitors). Treat versions as possibly out of date after 2026.

## How the others do it

**Granola** (sources 1–7)
- The note itself is a plain notepad; you type while it records mic and
  system audio (no bot in the call). Transcription starts when you start a
  note by hand.
- "Enhance notes" (automatic when the call ends, or by button) merges your
  typed lines, the transcript, the calendar event and your profile. Cloud
  models (OpenAI, Anthropic).
- Your words black, AI additions grey. A magnifier per line shows the
  transcript quotes behind it. (Older docs: editing a grey line turns it
  black; uncertain for today's version.)
- Templates set the structure (29 built in); "recipes" are saved prompts
  run afterwards. Regenerate, switch template, or ask the chat to revise.
- Typing nothing still works, but the result is a generic summary; your
  notes are what steer the focus.
- No documented notes-only mode without a recording (not verified).

**Notion AI Meeting Notes** (8, 9; launched 2025-05-13 against Granola)
- `/meet` inside a page; a "Notes" section to write in before or during.
- Summary from transcript plus your notes; summary lines link to the
  transcript. Format: Auto or a custom template; "Retry summary".
- Desktop captures audio locally, no bot. Business/Enterprise only.

**Apple Notes** (macOS 15.1+, iOS 18.1+; 10, 11)
- A recording lives inside a note; you can type in the note while it
  records. Afterwards: summary (Apple Intelligence) and "Add transcript
  to note". Your typed text and the recording stay separate; nothing says
  the summary uses your typing.

**Otter, Fathom, tl;dv** (12–16)
- Otter: highlight and comment on the live transcript, add photos of
  slides. Joins as a bot.
- Fathom: a Highlight button marks and summarizes the moment, with tags.
- tl;dv: manual timestamped notes during the call. Whether they feed the
  AI summary could not be confirmed.

## What users love
- No bot in the call; it feels like a normal notepad (17, 4).
- My notes steer the result, and I can see which lines are mine (2, 4).
- Every AI line can be checked against the transcript (1).

## What users hate
- Privacy and consent: others aren't told; GDPR worries; a lawsuit
  (*Chamberlain v. Granola*); shared links public by default (18, 19).
- Silent recording failures; "Me"/"Them" as the only speakers; no audio
  to check what was really said (18).
- Free plan cut to 30 days of history after the Feb 2026 redesign (19).
- Accuracy: people re-read the transcript for important meetings; too
  many action items (20, 21).

## What this means for Sokki
1. The note is the notepad: one plain page during the meeting, no tabs.
   The enhanced note is written into the same page.
2. The user's own words stay word for word and look different from
   Sokki's (Granola's black/grey is the one users name).
3. Sokki's lines can link back to the transcript and the audio (Sokki is
   local and keeps both).
4. An empty notepad still gives today's summary; typed lines are the
   backbone when present.
5. Sokki already answers the complaints: no bot, nothing leaves the Mac,
   a notice in the meeting chat, a strip when the mic hears nothing.

## What Sokki built (2026-11-26)

Robin's picks on the canvas, after two rounds of four versions each:
- During the meeting, "1 · Nebeneinander": notes on the left, live text on the right. He rejected round 1 because he wants to see the live transcript.
- After the meeting, all three views so he can find out which he uses: Notizen (as typed), Kombiniert (Granola's mix), and AI-Zusammenfassung (free, unchanged), plus Transkript.
- "Transkript kopieren" and "Zusammenfassung kopieren" stay where they were: "Eher wie heute lassen". The buttons are redesigned as "C · Gefüllt".

**Measurement, end to end in the test app** (`OWN_NOTES=… TITLE_AFTER=600 scripts/test-meeting.sh`, the 2-minute made-up English sample call, built-in Gemma 4 E4B Q4_K_M, its default temperature):
- 5 typed lines, 5 with additions, 15 additions (the cap of 3 per line each time).
- The extra request took 7.4 s; the summary step took 16.8 s in all, the extra request included. Processing finished 46.8 s after the stop.
- No line came back reworded (code copies the lines), no reply needed a retry, and the front matter got `id:` and `own_notes: true`.
- Problems seen:
  - Additions name speakers "Speaker 1/2/3" until they are named.
  - Two lines got the same fact ("without Apple Pay") because the additions overlap across lines.
  - Every line got the full 3 additions; whether 3 is too many needs real meetings.

## Sources
1. https://docs.granola.ai/help-center/taking-notes/ai-enhanced-notes
2. https://techcrunch.com/2024/05/22/granola-debuts-an-ai-notepad-for-meetings
3. https://docs.granola.ai/help-center/ios/taking-notes.md
4. https://zapier.com/blog/granola-ai/
5. https://www.granola.ai/blog/meeting-recipes-repeatable-formats
6. https://docs.granola.ai/help-center/getting-more-from-your-notes/recipes
7. https://granola.ai/llms.txt
8. https://notion.com/help/ai-meeting-notes
9. https://techcrunch.com/2025/05/13/notion-takes-on-ai-notetakers-like-granola-with-its-own-transcription-feature
10. https://support.apple.com/guide/notes/apdb5106e334
11. https://support.apple.com/en-ca/guide/mac-help/summarize-an-audio-recording-in-notes-mchl2102c2ae/15.0/mac/15.0
12. https://siliconangle.com/2023/02/14/otter-ai-launches-ai-assistant-generate-meeting-notes-capture-images-summaries/
13. https://provost.virginia.edu/subsite/coaa/services-tools/otterzoom
14. https://zapier.com/blog/fathom-features
15. https://www.producthunt.com/p/fathom/fathom-3-0
16. https://intercom.help/tldv/en/articles/5946332-manually-add-timestamped-notes-to-meetings (404 when fetched)
17. https://www.remio.ai/post/reddit-users-are-turning-granola-into-the-new-meeting-attention-debate
18. https://anarlog.so/blog/granola-ai-complaints/ (written by a competitor)
19. https://www.hedy.ai/post/granola-redesign-alternative-hedy/ (written by a competitor)
20. https://medium.com/@trelvek/granola-review-2026-a-limitations-first-look-at-ai-meeting-notes-0bcd97d68bc4
21. https://www.granola.ai/blog/ai-notetaker-myths-facts-sales
