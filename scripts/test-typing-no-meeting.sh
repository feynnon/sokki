#!/bin/bash
# Long typing, no call, nobody speaking: no meeting may start (2026-11-27,
# "meetings are often detected when I just type for a long time").
# Headless, in the test app "Sokki Test" (scripts/test-auto-meeting.sh),
# with a made-up keyboard sound as the microphone and a made-up known
# voice (macOS voice "Anna") as the user's own voice. Call detection's part
# (a browser holding the mic while nothing plays) is covered by the unit
# tests in crates/meeting-detect/src/watcher.rs.
#
#   scripts/test-dictation.sh --build        # build the test app first
#   scripts/test-typing-no-meeting.sh        # prints PASS or FAIL
set -euo pipefail
cd "$(dirname "$0")/.."
SET="${SOKKI_EVALS:-$HOME/sokki-evals}/typing-noise-made-up"
mkdir -p "$SET"
LOG="$HOME/Library/Logs/${TEST_ID:-org.sokki.test}/sokki.log"

# Two typists: steady, and fast and loud. Same seeds every run.
scripts/make-typing-wav.py "$SET/typing-steady.wav" 90 8 0.3 7
scripts/make-typing-wav.py "$SET/typing-fast-loud.wav" 90 12 0.8 11
[[ -f "$SET/known-anna.wav" ]] || say -v Anna -o "$SET/known-anna.wav" --data-format=LEI16@16000 \
  "Das ist eine kurze Probe meiner eigenen Stimme, damit Sokki sie später wiedererkennt."

fail=0
for clip in typing-steady typing-fast-loud; do
  out="$(KNOWN="$SET/known-anna.wav" scripts/test-auto-meeting.sh "$SET/$clip.wav" 2>&1)"
  echo "$out" | grep -E "Auto meeting:" | sed "s/^/  [$clip] /" | tail -8
  if echo "$out" | grep -q "Auto meeting: started\|Auto meeting: listening in"; then
    echo "FAIL: $clip started or listened in"
    fail=1
  elif ! echo "$out" | grep -q "Auto meeting: listening"; then
    echo "FAIL: $clip: the listener never ran (no known voice? see $LOG)"
    fail=1
  else
    echo "ok: $clip: no meeting"
  fi
done
# Control: the same harness must still start for the known voice talking,
# or "no meeting" above proves nothing.
[[ -f "$SET/known-talk.wav" ]] || say -v Anna -o "$SET/known-talk.wav" --data-format=LEI16@16000 \
  "Guten Morgen zusammen. Ich fasse kurz zusammen, was wir letzte Woche geplant haben. \
Erstens wollten wir die neue Version am Donnerstag testen. Zweitens sollte jemand die \
Anleitung lesen und Fehler sammeln. Drittens treffen wir uns am Freitag wieder. \
Gibt es dazu Fragen? Dann gehen wir jetzt die offenen Punkte einzeln durch, einer nach dem anderen."
out="$(KNOWN="$SET/known-anna.wav" scripts/test-auto-meeting.sh "$SET/known-talk.wav" 2>&1)"
echo "$out" | grep -E "Auto meeting:" | sed "s/^/  [control] /" | tail -4
if echo "$out" | grep -q "Auto meeting: started\|Auto meeting: listening in"; then
  echo "ok: control: the known voice talking starts"
else
  echo "FAIL: control: the known voice talking did not start (the test can't see starts)"
  fail=1
fi
[[ $fail = 0 ]] && echo PASS || { echo FAIL; exit 1; }
