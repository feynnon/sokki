#!/bin/bash
# The "record this call?" window (src-tauri/src/meeting/prompt.rs), tested
# without a person in a test app, which never touches Robin's Sokki. The app
# asks N times in a row, as call detection does for one call after another
# (Slack huddle, Google Meet, Zoom, Teams), each answered "not now" after a
# second. The window is built hidden: nothing shows on the screen.
#
#   TEST_ID=org.sokki.test.prompt TEST_NAME="Sokki Test Prompt" scripts/test-dictation.sh --build
#   TEST_ID=org.sokki.test.prompt TEST_NAME="Sokki Test Prompt" scripts/test-meeting-prompt.sh [n]
#
# Exit code 0 when every prompt came up, 1 otherwise. Until 2026-11-27 only
# the first prompt after a launch did (KNOWN_ISSUES.md).
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
N="${1:-4}"

quit_test_app() {
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || return 0
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi

WORK="$(mktemp -d)"
ffmpeg -loglevel error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 1 -c:a pcm_s16le "$WORK/mic.wav"
mkdir -p "$DATA"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.auto_start_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.mail_notes_auto = false
  | .settings.meetings.chat_notice_enabled = false
  | .settings.meetings.slack_post_huddles = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

quit_test_app
mkdir -p "$(dirname "$LOG")"
touch "$LOG"
tail -F -n0 "$LOG" > "$WORK/log.txt" 2>/dev/null &
TAILPID=$!
trap '{ kill $TAILPID; wait $TAILPID; } 2>/dev/null || true; quit_test_app; rm -rf "$WORK"' EXIT
sleep 1
open -g --env SOKKI_TEST_MIC="$WORK/mic.wav" --env SOKKI_TEST_PROMPTS="$N" -a "$TESTAPP"
END=$(( $(date +%s) + 30 + N * 4 ))
while (( $(date +%s) < END )); do
  sleep 1
  grep -q "Test mode: prompts done" "$WORK/log.txt" && break
done
grep -E "Test mode: prompt|Meeting prompt" "$WORK/log.txt" || true
grep -qE "Test mode: prompts done, $N/$N shown" "$WORK/log.txt"
