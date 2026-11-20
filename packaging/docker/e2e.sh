#!/bin/bash
# End-to-end test of the Linux service in a container, no hardware needed:
# a fake "Zoom" opens the microphone, Sokki asks, we say yes, the synthetic
# call plays (your voice into a virtual mic, the others through Zoom's own
# playback stream), Zoom hangs up, Sokki stops by itself and writes the note.
#
#   docker run --rm -v <repo>:/src -v sokki-target:/target -v sokki-models:/models \
#     sokki-build:arch bash /src/packaging/docker/e2e.sh /target/release/sokki <fixture-dir>
set -euo pipefail

SOKKI=${1:?path to the sokki binary}
FIXTURE=${2:?folder with mic.wav and system.wav}
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg
rm -rf "$HOME" "$XDG_RUNTIME_DIR"
mkdir -p "$HOME/.local/share/sokki" "$XDG_RUNTIME_DIR" "$HOME/.config/sokki"
chmod 700 "$XDG_RUNTIME_DIR"
ln -s /models "$HOME/.local/share/sokki/models"

step() { printf '\n== %s\n' "$*"; }
state() { "$SOKKI" status --json; }
wait_for() { # jq-free: grep the state JSON
  local what=$1 secs=$2
  for _ in $(seq "$secs"); do
    if state 2>/dev/null | grep -q "$what"; then return 0; fi
    sleep 1
  done
  echo "timed out waiting for $what"; state || true
  echo "--- service log"; grep -v cpuid /tmp/sokki.log | tail -20
  echo "--- capture streams"; pw-dump | grep -E '"(media.class|application.name|application.process.binary)"' | sort | uniq -c | head -20
  return 1
}

step "PipeWire (headless)"
pipewire &>/tmp/pipewire.log &
sleep 1
wireplumber &>/tmp/wireplumber.log &
pipewire-pulse &>/tmp/pipewire-pulse.log &
sleep 2
# The computer's speakers, and a virtual microphone fed from a sink.
pactl load-module module-null-sink sink_name=speakers >/dev/null
pactl load-module module-null-sink sink_name=micfeed >/dev/null
pactl set-default-sink speakers
pactl set-default-source micfeed.monitor
pactl info | grep -E "Default (Sink|Source)"

step "Sokki"
cat >"$HOME/.config/sokki/sokki.toml" <<'EOF'
[meetings]
user_name = "Me"
notes_dir = "/tmp/home/Notes"
ask_seconds = 60
summary = false
EOF
"$SOKKI" daemon &>/tmp/sokki.log &
sleep 1
"$SOKKI" models install
"$SOKKI" doctor || true

step "A fake Zoom opens the microphone"
mkdir -p /tmp/zoom && cp "$(command -v pw-cat)" /tmp/zoom/zoom
/tmp/zoom/zoom --record -P application.name=Zoom /dev/null &
ZOOM_MIC=$!
if [[ ${MANUAL:-0} == 1 ]]; then
  echo "(MANUAL=1: no detection; recording by hand)"
else
  wait_for '"phase":"asking"' "${ASK_WAIT:-20}"
  state
fi

step "Yes, record"
"$SOKKI" meeting record
wait_for '"phase":"recording"' 5
state

step "The call plays (29 s)"
/tmp/zoom/zoom --playback -P application.name=Zoom --target speakers "$FIXTURE/system.wav" &
PLAY_THEM=$!
pw-play --target micfeed "$FIXTURE/mic.wav" &
PLAY_ME=$!
wait $PLAY_THEM $PLAY_ME

step "Zoom hangs up; Sokki stops by itself after the grace period"
kill $ZOOM_MIC
if [[ ${MANUAL:-0} == 1 ]]; then "$SOKKI" meeting stop; fi
wait_for '"phase":"idle"' 40
step "Writing the note"
for _ in $(seq 600); do
  if ! state | grep -q '"processing"' && ! state | grep -q '"queued":[1-9]'; then break; fi
  sleep 1
done
"$SOKKI" notes
echo
cat "$HOME"/Notes/*.md
echo
grep -E "Recording|call stream|all output|Call detected|Call ended|ERROR|WARN" /tmp/sokki.log || true
