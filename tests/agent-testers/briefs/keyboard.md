# Brief: voice typing with the Sokki keyboard

Read AGENT_BRIEF.md first: its tools, rules and report files all apply. Only
the mission changes: you set up the Sokki keyboard and use it in another app.
The speech model is already on the phone.

1. In Sokki, open the Dictate tab. Use Sokki's own buttons to turn the keyboard on
   (Android's keyboard list) and to pick it (Android's keyboard picker). Android warns
   that a keyboard can read what you type: say what your persona thinks of that, then
   go ahead. Grant the microphone when asked.
2. Open another app with a text field. Good ones on this phone: Contacts (add a
   contact, the name field), or the search field at the top of the Settings app
   (`phone.sh home`, then find the app). Tap the field: the Sokki keyboard should show.
3. Type a few letters by tapping keys, including your language's special letters if it
   has any (ä ö ü ß, é è, ñ, ą ę ł). Do they appear correctly?
4. Tap the microphone at the top right of the keyboard's suggestion strip. Wait about
   10 seconds. Does it show it is listening (red)? Does text appear, piece by piece?
   Tap it again to stop. Compare the text with the "keyboard mic hears" sentence in
   `results/<persona>/speech.txt`.
5. Dictate a second time in the same field. Is the new text added at the cursor, with a
   space, or does it replace or glue onto the old text?
6. Switch back to the previous keyboard and back to Sokki's. Is it easy to find?
7. Turn the phone (`phone.sh landscape`), check the keyboard and the mic, turn it back.

In `findings.json`, next to `score` and `issues`, add
`"goal_reached": "yes"` (or `"partly"`, `"no"`) and
`"dictated": [{"heard": "<sentence from speech.txt>", "typed": "<what appeared>"}]`.
