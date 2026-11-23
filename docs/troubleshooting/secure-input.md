# Shortcuts stopped working on macOS (Secure Input)

macOS turns on **Secure Input** while you type into a password field, so no
other app can read those keystrokes. While it is on, Sokki's keyboard listener
gets no key presses either: shortcuts with a letter or Space (like ⌥ Space)
stop working, while modifier-only shortcuts keep working. Sokki shows a
warning and, when it can, names the app that holds Secure Input.

Secure Input normally ends as soon as you leave the password field. When it
stays on:

1. Switch to the app Sokki names and leave its password field, or quit the app.
2. In Terminal, turn off **Terminal → Secure Keyboard Entry**; iTerm2 has the
   same option in its app menu.
3. If no app is named, lock the screen (⌃ ⌘ Q) and unlock it again. A stuck
   login window often holds Secure Input after waking from sleep.
4. If it is still on, log out and back in.

To see which process holds it, run in Terminal:

```sh
ioreg -l -w 0 | grep SecureInput
```

The `kCGSSessionSecureInputPID` value is the process ID; `ps -p <PID>` shows
its name.
