#!/bin/bash
# Meetings that start by themselves (src-tauri/src/meeting/auto.rs), tested
# without a person in the test app ("Sokki Test", org.sokki.test), which
# never touches Robin's Sokki. The microphone plays one WAV, then silence;
# the listener decides by itself whether to start, and stops after two
# minutes without speech. Prints the "Auto meeting:" lines of the log.
#
#   scripts/test-dictation.sh --build              # build the test app first
#   scripts/test-auto-meeting.sh talk.wav [wait_s] # 16 kHz mono WAV; wait in s (default: length + 30)
#   PLAYING=Safari scripts/test-auto-meeting.sh …  # pretend an app plays sound (a video on the speakers)
#   KNOWN=me.wav scripts/test-auto-meeting.sh …    # that voice is the one known voice (else none:
#                                                  # nothing may start)
#   DISCARD=off scripts/test-auto-meeting.sh …     # flip the card's Auto-Start switch 3 s after a start
#   DISCARD=close scripts/test-auto-meeting.sh …   # click the card's X instead
#   UNTIL_STOP=1 scripts/test-auto-meeting.sh …    # wait for the auto stop and processing (+3 min)
#
# Exit code 0 always; read the output: "Auto meeting: started" is a start.
# Made-up audio only. Nothing plays through the speakers.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
APP_ID="$(defaults read "$TESTAPP/Contents/Info" CFBundleIdentifier 2>/dev/null || echo "$ID")"

quit_test_app() {
  osascript -e "tell application id \"$APP_ID\" to quit" 2>/dev/null || true
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" || true
  sleep 2
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
# Test builds share one data folder; never run next to another one.
# (ps, not pgrep: a pgrep's own command line holds the pattern and matched.)
if ps -axo command= | grep -E "^/.*/Applications/$NAME[^/]*\.app/Contents/MacOS/" \
    | grep -v -F "$TESTAPP/Contents/MacOS/" >/dev/null; then
  echo "Another $NAME build is running (another session's test); try again later." >&2
  exit 1
fi
[[ $# -ge 1 ]] || { echo "usage: $0 mic.wav [wait_s]" >&2; exit 1; }
abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
MIC="$(abs "$1")"
LENGTH=$(afinfo "$MIC" | awk '/estimated duration/ {print $3}')
WAIT="${2:-$(echo "$LENGTH + 30" | bc)}"
[[ -z "${UNTIL_STOP:-}" ]] || WAIT=$(echo "$WAIT + 180" | bc)

REAL_INDEX="$HOME/Library/Application Support/$REAL_ID/meetings/index.json"
if [[ -f "$REAL_INDEX" ]] && jq -e '[(if type == "array" then . else (.meetings // []) end)[]
    | select(.status.kind == "recording" or .status.kind == "processing")] | length > 0' \
    "$REAL_INDEX" >/dev/null 2>&1; then
  echo "A meeting of Robin's is recording or processing; try again later." >&2
  exit 1
fi
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi

WORK="$(mktemp -d)"
# A silent call side, so the test never records this Mac's real sound.
ffmpeg -loglevel error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 1 -c:a pcm_s16le "$WORK/system.wav"
mkdir -p "$DATA"
jq --arg notes "$WORK/notes" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.auto_start_enabled = true
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.correction_enabled = false
  | .settings.meetings.summary_enabled = false
  | .settings.meetings.mail_notes_auto = false
  | .settings.meetings.chat_notice_enabled = false
  | .settings.meetings.slack_post_huddles = false
  | .settings.meetings.notes_dir = $notes' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

quit_test_app
sleep 1
rm -rf "$DATA/meetings"
tail -F -n0 "$LOG" > "$WORK/log.txt" 2>/dev/null &
TAILPID=$!
# `|| true`: under set -e, the killed tail's status ended this trap before
# quit_test_app, which left the test app running.
trap '{ kill $TAILPID; wait $TAILPID; } 2>/dev/null || true; quit_test_app; rm -rf "$WORK"' EXIT
sleep 1
echo "Playing $(basename "$MIC") (${LENGTH}s) into the test microphone, waiting ${WAIT}s…"
open -g --env SOKKI_TEST_MIC="$MIC" --env SOKKI_TEST_AUTO=1 --env SOKKI_TEST_MIC_CONTINUE=1 \
  --env SOKKI_TEST_MEETING=1 --env SOKKI_TEST_SYSTEM="$WORK/system.wav" \
  ${PLAYING:+--env SOKKI_TEST_AUTO_PLAYING="$PLAYING"} ${DISCARD:+--env SOKKI_TEST_AUTO_DISCARD="$DISCARD"} ${KNOWN:+--env SOKKI_TEST_AUTO_KNOWN="$(abs "$KNOWN")"} \
  -a "$TESTAPP"
END=$(( $(date +%s) + ${WAIT%.*} ))
while (( $(date +%s) < END )); do
  sleep 5
  if [[ -n "${UNTIL_STOP:-}" ]] && grep -qE "Auto meeting: stopped" "$WORK/log.txt" \
     && { jq -e '[(if type == "array" then . else (.meetings // []) end)[]
            | select(.status.kind == "recording" or .status.kind == "processing")] | length == 0' \
            "$DATA/meetings/index.json" >/dev/null 2>&1; }; then
    sleep 5
    break
  fi
done
grep -E "Auto meeting|Test mode: playing|Meeting .* recording .* ms after|ERROR" "$WORK/log.txt" || true
echo "Meetings left in the test app: $(jq -c '[(if type == "array" then . else (.meetings // []) end)[] | {status: .status.kind, duration_s}]' "$DATA/meetings/index.json" 2>/dev/null || echo "[]")"
