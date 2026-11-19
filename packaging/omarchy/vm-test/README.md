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

Not covered here: the pacman package (the VM user has no passwordless sudo),
x86 (the VM is aarch64; `packaging/docker` builds and tests x86_64 under
emulation), and real call apps behind a login.

The demo video: `./demo` records a whole run in the VM (call, question,
recording, note, naming a speaker, opening the note, dictation) with
`wf-recorder`, sound from a virtual mix of both sides, and timestamps in
`vm-share/demo/marks.txt`; `./edit-demo` cuts it (the note-writing wait sped
up and labelled) into `vm-share/demo/sokki-omarchy-demo.mp4`.
