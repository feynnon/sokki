# Brief: record a meeting, with the button and the Quick Settings tile

Read AGENT_BRIEF.md first: its tools, rules and report files all apply. Only
the mission changes: you record meetings on the phone. There is no Mac, so
recordings stay on the phone and are never turned into notes here: test the
recording itself, the list, and what Sokki says about the missing Mac.

1. Skip the Mac step, open the Meeting tab. On Android 13 and newer, Sokki offers to add
   its "Record meeting" tile with Android's own dialog. Add it if your persona would.
2. Record with the big button: start, wait about 20 seconds (`sleep 20`), stop. Grant
   the microphone when asked. While recording: does a clock run, is there any sign that
   sound comes in, and does the screen stay on? Afterwards: is the recording in the
   list, with a time and length? What does it say about sending it to the Mac? Tap it.
3. Record with the tile: `phone.sh quick-settings`. Find "Record meeting" (if it is
   not there, add it with the pencil/edit button, as a user would). Tap it: Sokki
   should open already recording. Wait 15 seconds. Open Quick Settings again: the tile
   should say "Recording". Tap it: the recording should stop without opening anything.
   Check the list in Sokki.
4. Start a recording and leave Sokki (`phone.sh home`), wait 10 seconds, come back.
   Still recording? Then stop it.
5. Tap "Check again" or whatever offers to send the recordings. What happens without a Mac?

In `findings.json`, next to `score` and `issues`, add
`"goal_reached": "yes"` (or `"partly"`, `"no"`) and
`"recordings": [{"how": "button", "seconds": 20, "in_list": true, "length_shown": "0:20"}]`.
