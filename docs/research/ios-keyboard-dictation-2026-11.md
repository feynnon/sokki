# iOS keyboard dictation: what a dev podcast says (Nov 2026)

**Date:** 2026-11-26
**Task:** Summarise what the Syntax podcast episode "Syntax Weekly" (late Nov 2026) says about dictation and transcription, for Sokki's keyboard and dictation work.
**Source:** Syntax, "Google Cooked with Neon ⟡ Raycast Claps Back ⟡ Docker Rebranded ⌁ Syntax Weekly", hosts Scott Tolinski, Wes Bos, CJ Reynolds. https://www.youtube.com/watch?v=v9gt7u_7nWQ, from 0:47:24 to 0:52:10. Rebuild the subtitles with `uvx yt-dlp --skip-download --write-subs --sub-langs en "https://www.youtube.com/watch?v=v9gt7u_7nWQ"`.

## Findings (paraphrased)

- **The problem (0:47:24):** third-party keyboards on iOS can't reach the microphone, so dictating means switching to Apple's keyboard. Per Wes, every dictation startup hits this, and LLMs tell them it can't be done.
- **Raycast's workaround (0:49:33):** its iOS keyboard embeds a tiny (2×2 px) web view that gets the microphone through the browser's `getUserMedia`. Dictation in the Raycast keyboard is paid-only.
- **Copied within hours (0:49:33–0:50:48):** per a Raycast thread, another indie iOS keyboard had it after 2.5 h, Aqua Voice and Superwhisper after about 7–8 h, Monologue after 15 h, then Granola.
- **What the hosts still miss (0:50:48–0:52:01):** seeing the text live while speaking; teaching their own terms; good on-device post-processing. They rate Apple's own dictation highly.
- Meeting note-takers are not discussed.

## What it means for Sokki

- The `getUserMedia` web-view route is a possible way for an iOS Sokki keyboard to dictate without switching apps. Before building: check how Raycast does it exactly, and what App Review and "Full Access" allow.
- The three missing pieces (live text, own vocabulary, on-device cleanup) are what Sokki already does locally; useful as launch arguments.
