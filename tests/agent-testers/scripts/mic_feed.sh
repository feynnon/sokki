#!/usr/bin/env bash
# CI only (Linux runner): gives the emulator a microphone that hears the test
# audio. PulseAudio gets a silent virtual sink whose monitor is the default
# source; the emulator (started with -allow-host-audio) records from it, and
# a loop plays the test WAV into the sink.
#   bash scripts/mic_feed.sh setup          before the emulator starts
#   bash scripts/mic_feed.sh play <wav>     once the emulator is up (background loop)
set -euo pipefail
case "${1:-}" in
  setup)
    pulseaudio --check 2>/dev/null || pulseaudio -D --exit-idle-time=-1
    pactl load-module module-null-sink sink_name=sokkimic sink_properties=device.description=SokkiTestMic >/dev/null
    pactl set-default-sink sokkimic
    pactl set-default-source sokkimic.monitor
    echo "virtual microphone ready" ;;
  play)
    WAV=$2
    ( while true; do paplay -d sokkimic "$WAV"; sleep 2; done ) >/dev/null 2>&1 &
    echo "test audio plays into the microphone (pid $!)" ;;
  *) sed -n '2,8p' "$0"; exit 2 ;;
esac
