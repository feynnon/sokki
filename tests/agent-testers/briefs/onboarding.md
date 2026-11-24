# Brief: first launch to the first dictated sentence

Read AGENT_BRIEF.md first: its tools, rules and report files all apply. Only
the mission changes: you follow one flow end to end, as a brand-new user, and
time it. On this run the speech model is NOT on the phone yet: its download is
part of the flow.

1. Sokki is open on its first screen. Read it as your persona would. What does it
   ask you to do first? Is it clear that a Mac is optional for dictation?
2. Get past the Mac step without a Mac (skip, later, back). Note how many taps it took.
3. Go to the Dictate tab. Start the speech model download. Screenshot the progress
   every 20-30 seconds (`sleep 20`). Does it show how big it is, how far it is, and
   what happens if you leave the screen? Leave the app once (`phone.sh home`), come
   back, and check the download is still going or finished.
4. When the model is ready, dictate: start, wait about 15 seconds while the test audio
   plays, stop. Answer Android's microphone dialog as your persona would.
5. Compare the text with `results/<persona>/speech.txt`. Can you copy the text? Is it
   clear what to do with it next?
6. Note the total: taps from the first screen to the first text, and minutes.

In `findings.json`, next to `score` and `issues`, add
`"goal_reached": "yes"` (or `"partly"`, `"no"`), `"actions": 23` (taps from first
screen to first text) and `"minutes": 6`. Start `report.md` with the same three lines.
