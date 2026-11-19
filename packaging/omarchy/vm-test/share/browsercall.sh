#!/bin/bash
# A browser call in Chromium, as an Omarchy web app (--app), on virtual
# devices only.  browsercall.sh start [delay_ms] | play | hangup
set -euo pipefail
S=/tmp/sokki-vmcall; mkdir -p $S
case $1 in
start)
  bash ~/vm-share/safe-audio.sh on >/dev/null
  # Refuse to start unless the test mic is silent (nothing from the Mac).
  peak=$(bash ~/vm-share/safe-audio.sh check | sed -n 's/.*nothing playing: \([0-9]*\).*/\1/p')
  [ "$peak" = 0 ] || { echo "test mic is not silent ($peak); stopping"; exit 1; }
  setsid chromium --user-data-dir=/tmp/sokki-chrome --no-first-run --test-type --use-fake-ui-for-media-stream \
    --autoplay-policy=no-user-gesture-required --allow-file-access-from-files \
    --app="file://$HOME/vm-share/fakemeet/index.html?delay=${2:-20000}" >/dev/null 2>&1 &
  echo "browser call opened";;
play)
  pw-play --target sokki_test_micfeed ~/vm-share/${SCENARIO:-demo}/mic.wav; echo "my side done";;
hangup)
  pkill -f "user-data-dir=/tmp/sokki-chrome" || true; echo "left the call";;
cleanup)
  bash ~/vm-share/safe-audio.sh off;;
esac
