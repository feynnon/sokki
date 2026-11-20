# Testing Sokki in an Omarchy VM

Real Omarchy (Hyprland, the Omarchy shell, PipeWire, Chromium) in the
"Try Omarchy" VM on an Apple Silicon Mac, driven from the Mac. Audio stays on
virtual devices inside the VM: `share/safe-audio.sh` mutes the Mac's
microphone and speakers in the guest and refuses to start if the test
microphone is not silent.

| File | What it does |
|---|---|
| `start-vm` | Boots the VM (4 GB; for a summary-model test e.g. `OMARCHY_QEMU_GPU_MEMORY_MIB=12288 ./start-vm` for the 9B model) with SSH on `localhost:2222` and `<repo>/vm-share` as `~/vm-share`; never next to a Rust build, and stop it right after the test with `./stop-vm` |
| `vm '<cmd>'` | Runs a command in the guest's desktop session |
| `vmshot out.png` | Screenshot of the guest desktop |
| `qmp.py keys super-ctrl-m` / `qmp.py type 'text'` | Keyboard input through QEMU (arrow keys don't arrive; use `j`/`k`) |
| `share/safe-audio.sh on\|check\|off` | Virtual speakers and a test microphone (with a 40 ms speaker echo, like a laptop) |
| `share/browsercall.sh start\|play\|hangup\|cleanup` | A Meet-style call in Chromium as a web app, on the virtual devices |
| `share/fakemeet/` | The call page (mic open, the other people's voices after a delay) |
| `share/meetcall.sh setup\|join URL a\|b\|play\|hangup\|cleanup` | A real call over a real server: guest A (you, recorded) and guest B (the other side, on its own virtual mic and speakers that Sokki never hears) |
| `qmp.py click X Y` | A mouse click at a screen pixel (1400x1050; see below) |

Once:

1. `./start-vm &`, wait for the desktop.
2. Add an SSH key for the guest user (`~/.ssh/authorized_keys`), e.g. by typing
   a command with `qmp.py type`, and an SSH host `omarchy-vm`
   (`HostName 127.0.0.1`, `Port 2222`, your guest user and key).
3. Copy `share/*` into `<repo>/vm-share/`, and a test call's `mic.wav` (the
   voice alone) into `vm-share/demo/` and its `system.wav` into
   `vm-share/fakemeet/` (`testdata/generate.py demo_en` writes both; use
   `mic_dry.wav` as `mic.wav`).
4. Build `sokki` for aarch64 (`packaging/docker`), copy it and
   `packaging/omarchy/*` into the guest, run `sokki-omarchy-setup`.

A call: `vm 'bash ~/vm-share/browsercall.sh start 22000'`, answer with
`qmp.py keys super-ctrl-m` then `qmp.py keys ret`, then
`vm 'bash ~/vm-share/browsercall.sh play; bash ~/vm-share/browsercall.sh hangup'`.
The note lands in `~/Documents/Sokki` after the 20 s grace period.

A real call, no accounts needed: a public Jitsi server.
`vm 'bash ~/vm-share/meetcall.sh setup'`, set the screen to 1400x1050
(`vm "hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1400x1050@59.98\", position = \"0x0\", scale = 1 })'"`),
then `meetcall.sh join 'https://meet.ffmuc.net/ROOM#config.startAudioOnly=true&config.prejoinConfig.enabled=false&userInfo.displayName=%22A%22' a`
and the same with `b`. Click "Allow" for the microphone in each window
(`qmp.py click`, screenshots with `vmshot`) and unmute with `m`; Sokki asks,
then `meetcall.sh play` speaks both sides and `meetcall.sh hangup` ends the call.

- The VM also gets the Mac's camera (`/dev/video42`, "Mac Camera"). Never
  allow camera access in the guest; `startAudioOnly` asks for the mic only.
- Google Meet turns away anonymous guests from the VM ("You can't join this
  video call"), with or without DevTools or automation flags; a Meet test
  needs a Google sign-in in the guest browser.

Not covered here: the pacman package (the VM user has no passwordless sudo),
x86 (the VM is aarch64; `packaging/docker` builds and tests x86_64 under
emulation), and Google Meet, Zoom or Teams behind a login.

The demo video: `./demo` records a whole run in the VM (call, question,
recording, note, naming a speaker, opening the note, dictation) with
`wf-recorder`, sound from a virtual mix of both sides, and timestamps in
`vm-share/demo/marks.txt`; `./edit-demo` cuts it (the note-writing wait sped
up and labelled) into `vm-share/demo/sokki-omarchy-demo.mp4`.
