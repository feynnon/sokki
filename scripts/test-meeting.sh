#!/bin/bash
# A simulated call without a person, in the test app ("Sokki Test",
# org.sokki.test) next to Robin's Sokki, which it never touches.
# Test mode (src-tauri/src/test_mic.rs): the microphone plays one WAV, the
# system audio another, both in real time; the app records one meeting,
# stops it, processes it and logs how long that took after the stop. No
# recording pill, no notification, no voice data, no language model (the
# note goes to a temporary folder). CPU and memory of the app are sampled
# every 5 s while it runs.
#
#   scripts/test-dictation.sh --build                 # build the test app first
#   scripts/test-meeting.sh mic.wav system.wav [hold] # 16 kHz mono WAVs; hold in s
#   NO_LIVE=1 scripts/test-meeting.sh ...             # without the live transcript (A/B)
#   SYSTEM_DELAY=12 scripts/test-meeting.sh ...        # system audio starts 12 s late
#   SECOND_S=30 GAP_S=2 scripts/test-meeting.sh ...   # a second meeting while the first is processed
#   NO_YIELD=1 SECOND_S=30 scripts/test-meeting.sh ... # same, processing not yielding (A/B)
#   TITLE_AFTER=60 scripts/test-meeting.sh ...        # provisional title after 60 s, with the
#                                                     # language model and the summary on
#   TESTAPP=/path/Sokki\ Test.app scripts/test-meeting.sh ... # another build of the test app
#
# Real recordings stay outside the repository: convert them to WAV in a
# temporary folder (ffmpeg -i mic.flac -ar 16000 -ac 1 mic.wav).
#
# Safety, after the 2026-11-19 input freeze: the app is started only with
# `open`, never its binary from a shell; nothing here runs tccutil or touches
# Robin's Sokki, its settings or its data.
set -euo pipefail

# TEST_ID/TEST_NAME: a second test app, so two sessions can test side by side.
ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
# Robin's settings: under the installed Sokki's bundle id.
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
quit_test_app() {
  osascript -e "tell application id \"$ID\" to quit" 2>/dev/null || true
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
[[ $# -ge 2 ]] || { echo "usage: $0 mic.wav system.wav [hold_s]" >&2; exit 1; }
abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
MIC="$(abs "$1")"
SYSTEM="$(abs "$2")"
LENGTH=$(afinfo "$MIC" | awk '/estimated duration/ {print $3}')
HOLD="${3:-$(echo "$LENGTH + 1" | bc)}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DATA"
# Robin's settings, minus everything that reaches outside the test app.
jq --arg notes "$WORK/notes" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.correction_enabled = false
  | .settings.meetings.summary_enabled = ($title != "")
  | .settings.meetings.notes_dir = $notes' --arg title "${TITLE_AFTER:-}" "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
if [[ -n "${TITLE_AFTER:-}" ]]; then
  # Sokki's downloaded language model, linked, not copied.
  MODEL="$HOME/Library/Application Support/$REAL_ID/meeting-models/llm/Qwen3.5-9B-Q4_K_M.gguf"
  [[ -f "$MODEL" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }
  mkdir -p "$DATA/meeting-models/llm"
  [[ -f "$DATA/meeting-models/llm/$(basename "$MODEL")" ]] || ln "$MODEL" "$DATA/meeting-models/llm/"
fi

if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc" \
  && security find-identity -p codesigning 2>/dev/null | grep -q '"Sokki Dev"'; then
  echo "The test app is signed ad hoc; rebuild it: scripts/test-dictation.sh --build" >&2
  exit 1
fi

quit_test_app
sleep 1
# Only the test app's own meetings: a run cut short by quitting would
# otherwise be processed at this launch, next to the new one.
rm -rf "$DATA/meetings"
# Follow the log across rotation (a long meeting fills it several times).
EVENTS="$WORK/events.txt"
PATTERN="Test mode|Live transcript|live transcript|Live speakers|Live microphone|Live system|Meeting step|Aligned tracks|recording .* ms after|Diarized|summary later|Meeting title|dates follow|anguage model|ERROR|WARN"
# One process, so the trap ends it and nothing outlives the script.
tail -F -n0 "$LOG" > "$WORK/log.txt" 2>/dev/null &
TAILPID=$!
trap '{ kill $TAILPID; wait $TAILPID; } 2>/dev/null; rm -rf "$WORK"' EXIT
sleep 1
echo "Recording a ${HOLD}s meeting (live transcript ${NO_LIVE:+off}${NO_LIVE:-on})…"
open -g --env SOKKI_TEST_MIC="$MIC" --env SOKKI_TEST_SYSTEM="$SYSTEM" \
  --env SOKKI_TEST_MEETING=1 --env SOKKI_TEST_HOLD_S="$HOLD" \
  ${NO_LIVE:+--env SOKKI_TEST_NO_LIVE=1} ${SYSTEM_DELAY:+--env SOKKI_TEST_SYSTEM_DELAY_S=$SYSTEM_DELAY} \
  ${SECOND_S:+--env SOKKI_TEST_SECOND_S="$SECOND_S"} ${GAP_S:+--env SOKKI_TEST_GAP_S="$GAP_S"} \
  ${NO_YIELD:+--env SOKKI_TEST_NO_YIELD=1} ${TITLE_AFTER:+--env SOKKI_TEST_TITLE_AFTER_S="$TITLE_AFTER"} \
  -a "$TESTAPP"

# Sample the app (and its model workers) every 5 s until processing is done.
SAMPLES="$WORK/samples.txt"
STARTED=""
LIMIT=$(echo "($HOLD + ${SECOND_S:-0} + 900) / 5" | bc)
for _ in $(seq 1 "$LIMIT"); do
  sleep 5
  grep -E "$PATTERN" "$WORK/log.txt" | grep -v "Loaded settings" > "$EVENTS" || true
  [[ -n "$STARTED" ]] || ! grep -q "Test mode: meeting starts" "$EVENTS" || STARTED=$(date +%s)
  grep -qE "Test mode: (meeting processed|the meeting failed|the meeting didn)" "$EVENTS" && break
  PIDS=$(pgrep -f "^$TESTAPP/Contents/MacOS/" | paste -sd, - || true)
  [[ -n "$PIDS" ]] || break
  # %cpu summed over the app's processes, resident memory in MB.
  { ps -o %cpu=,rss= -p "$PIDS" || true; } \
    | awk -v t="$(date +%s)" '{c+=$1; m+=$2} END {printf "%s %.0f %.0f\n", t, c, m/1024}' >> "$SAMPLES"
done
sleep 1
grep -E "$PATTERN" "$WORK/log.txt" | grep -v "Loaded settings" || true
if [[ -s "$SAMPLES" && -n "$STARTED" ]]; then
  awk -v stop="$(echo "$STARTED + $HOLD" | bc)" '{
      if ($1 < stop) { n++; c+=$2; if ($2>cmax) cmax=$2; if ($3>mmax) mmax=$3 }
      else { pn++; pc+=$2; if ($2>pcmax) pcmax=$2; if ($3>pmmax) pmmax=$3 } }
    END {
      if (n) printf "During the call: CPU mean %.0f%%, max %.0f%%; memory max %d MB (%d samples)\n", c/n, cmax, mmax, n;
      if (pn) printf "After the stop:  CPU mean %.0f%%, max %.0f%%; memory max %d MB (%d samples)\n", pc/pn, pcmax, pmmax, pn }' "$SAMPLES"
fi
quit_test_app
