#!/bin/bash
# A real Google Meet call inside the VM, with two guests on virtual devices:
#   A = the Sokki user (default test mic + test speakers, recorded by Sokki)
#   B = the other side (its own mic and speakers, which Sokki never records)
# Nothing reaches the Mac: safe-audio.sh mutes the Mac's devices first.
#   meetcall.sh setup | join <url> | play [scenario] | hangup | cleanup
set -euo pipefail
S=/tmp/sokki-vmcall; mkdir -p $S
case $1 in
setup)
  bash ~/vm-share/safe-audio.sh on >/dev/null
  pactl list short sinks | grep -q sokki_b_speakers || pactl load-module module-null-sink sink_name=sokki_b_speakers >> $S/modules
  pactl list short sinks | grep -q sokki_b_micfeed || pactl load-module module-null-sink sink_name=sokki_b_micfeed >> $S/modules
  pactl list short sources | grep -q "sokki_b_mic\s" || pactl load-module module-remap-source master=sokki_b_micfeed.monitor source_name=sokki_b_mic >> $S/modules
  peak=$(bash ~/vm-share/safe-audio.sh check | sed -n 's/.*nothing playing: \([0-9]*\).*/\1/p')
  [ "$peak" = 0 ] || { echo "test mic is not silent ($peak); stopping"; exit 1; }
  echo "audio ready";;
join)
  # Plain browsers (no automation flags: Meet turns those away as bots).
  # join <url> a|b opens one guest; without a|b both.
  flags=(--no-first-run --no-default-browser-check)
  if [ "${3:-a}" = a ]; then rm -rf /tmp/sokki-meet-a
    setsid chromium --user-data-dir=/tmp/sokki-meet-a "${flags[@]}" --app="$2" >/dev/null 2>&1 &
  fi
  if [ "${3:-b}" = b ]; then rm -rf /tmp/sokki-meet-b
    PULSE_SINK=sokki_b_speakers PULSE_SOURCE=sokki_b_mic \
      setsid chromium --user-data-dir=/tmp/sokki-meet-b "${flags[@]}" --app="$2" >/dev/null 2>&1 &
  fi
  echo "guest(s) opened";;
streams)
  # Which browser stream uses which device (B must use only sokki_b_*).
  pactl -f json list source-outputs | jq -r '.[] | "in  \(.properties["application.process.id"]) \(.properties["application.name"]) -> source \(.source)"'
  pactl -f json list sink-inputs | jq -r '.[] | "out \(.properties["application.process.id"]) \(.properties["application.name"]) -> sink \(.sink)"'
  pactl list short sources | awk '{print "source", $1, $2}'; pactl list short sinks | awk '{print "sink", $1, $2}';;
play)
  sc=${2:-demo}
  pw-play --target sokki_test_micfeed ~/vm-share/$sc/mic.wav &
  pw-play --target sokki_b_micfeed ~/vm-share/$sc/system.wav
  wait; echo "conversation done";;
hangup)
  pkill -f "user-data-dir=/tmp/sokki-meet-" || true; echo "left the call";;
cleanup)
  bash ~/vm-share/safe-audio.sh off;;
esac
