#!/bin/bash
# Test audio in the VM that can never reach the Mac: the Mac's microphones
# and speakers (omarchy_host_*) are muted, and a test microphone carries only
# what is played into sokki_test_micfeed.   safe-audio.sh on | off | check
set -euo pipefail
S=/tmp/sokki-vmcall; mkdir -p $S
host_sources() { pactl list short sources | awk '{print $2}' | grep -E '^omarchy_host_input|^alsa_input' || true; }
host_sinks() { pactl list short sinks | awk '{print $2}' | grep -E '^omarchy_host_output|^alsa_output' || true; }
case $1 in
on)
  [ -f $S/old-sink ] || { pactl get-default-sink > $S/old-sink; pactl get-default-source > $S/old-source; }
  for s in $(host_sources); do pactl set-source-mute "$s" 1; done
  for s in $(host_sinks); do pactl set-sink-mute "$s" 1; done
  pactl list short sinks | grep -q sokki_test_speakers || pactl load-module module-null-sink sink_name=sokki_test_speakers >> $S/modules
  pactl list short sinks | grep -q sokki_test_micfeed || pactl load-module module-null-sink sink_name=sokki_test_micfeed >> $S/modules
  # A real (non-monitor) source fed only by sokki_test_micfeed.
  pactl list short sources | grep -q "sokki_test_mic\s" || pactl load-module module-remap-source master=sokki_test_micfeed.monitor source_name=sokki_test_mic source_properties=device.description=Test-microphone >> $S/modules
  # Like a laptop: the microphone also hears the speakers, quieter and
  # about 40 ms later (only the test speakers; never the Mac's).
  if ! pactl list short modules | grep -q "source=sokki_test_speakers.monitor"; then
    pactl load-module module-loopback source=sokki_test_speakers.monitor sink=sokki_test_micfeed latency_msec=40 source_dont_move=true sink_dont_move=true >> $S/modules
    sleep 0.5
    idx=$(pactl -f json list sink-inputs | jq -r '.[] | select(.properties["media.name"]|tostring|test("^loopback"; "i")) | .index' | tail -1)
    [ -n "$idx" ] && pactl set-sink-input-volume "$idx" 15%
  fi
  pactl set-default-sink sokki_test_speakers
  pactl set-default-source sokki_test_mic
  echo "test audio on; Mac devices muted";;
check)
  echo "default source: $(pactl get-default-source)"
  for s in $(host_sources); do echo "$s muted=$(pactl get-source-mute "$s" | awk '{print $2}')"; done
  # Two seconds from the default source with nothing playing must be silent.
  timeout 2 pw-record --raw --format s16 --rate 16000 --channels 1 - 2>/dev/null > $S/check.raw || true
  peak=$(od -An -t d2 -v $S/check.raw | tr -s ' ' '\n' | sed '/^$/d' | awk '{a=$1<0?-$1:$1; if(a>m)m=a} END{print m+0}')
  echo "peak on the test mic with nothing playing: $peak (0 = silent)";;
off)
  pactl set-default-sink "$(cat $S/old-sink)" 2>/dev/null || true
  pactl set-default-source "$(cat $S/old-source)" 2>/dev/null || true
  [ -f $S/modules ] && tac $S/modules | while read -r m; do pactl unload-module "$m" 2>/dev/null || true; done
  rm -f $S/modules $S/old-sink $S/old-source
  for s in $(host_sources); do pactl set-source-mute "$s" 0; done
  for s in $(host_sinks); do pactl set-sink-mute "$s" 0; done
  echo "test audio off; Mac devices back";;
esac
