# Brief: settings, permissions and the way back from "no"

Read AGENT_BRIEF.md first: its tools, rules and report files all apply. Only
the mission changes: you look at every place where the user sets something
up, and at what happens when they say no. The speech model is already on the
phone.

1. Skip the Mac step. Then open it again from the main screen ("Mac" at the top):
   is it clear what it does, that it is optional, and how to leave?
2. Dictate tab: start dictating and DENY the microphone. What does Sokki say? Can you
   fix it from inside Sokki (a button, a hint)? Fix it the way the app suggests, or
   through Android's app settings if it suggests nothing, and dictate once.
3. Turn the Sokki keyboard on, then open its settings (the link in the Dictate tab, or
   the keyboard's own settings). Look at languages and layouts: is your language
   there, with the right layout? Change one setting your persona cares about and
   check it in a text field.
4. Notifications: if Sokki asks to send notifications, deny it, then record a short
   meeting. Does anything break?
5. Dark mode and text size are set on this phone: look for text that is hard to read
   or cut, on every Sokki screen you visited.

In `findings.json`, next to `score` and `issues`, add
`"denied_and_recovered": [{"what": "microphone", "recovered": true, "how": "button in Sokki"}]`.
