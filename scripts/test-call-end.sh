#!/bin/bash
# A recording ends with its call (src-tauri/src/meeting/detect.rs), tested
# without a person or a real call in a test app, which never touches
# Robin's Sokki. The app simulates a Zoom call that starts and, 8 s after
# the recording, ends; the "call ended" window is built hidden.
#
#   TEST_ID=org.sokki.test.prompt TEST_NAME="Sokki Test Prompt" scripts/test-dictation.sh --build
#   TEST_ID=org.sokki.test.prompt TEST_NAME="Sokki Test Prompt" scripts/test-call-end.sh [modes]
#
# Modes (default all three): "hand" (started in Sokki during the call),
# "prompt" (started from the call prompt), "keep" (started by hand, "Keep
# recording" clicked when the call ends). Expected: hand and prompt stop
# about 20 s after the call ended, keep still records. Exit code 0 when all
# match. Made-up silence only; nothing reaches a chat or a call app.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
MODES="${*:-hand prompt keep}"

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
ffmpeg -loglevel error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 1 -c:a pcm_s16le "$WORK/silence.wav"
mkdir -p "$DATA" "$(dirname "$LOG")"
jq --arg notes "$WORK/notes" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.auto_start_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.summary_enabled = false
  | .settings.meetings.mail_notes_auto = false
  | .settings.meetings.chat_notice_enabled = false
  | .settings.meetings.call_names_enabled = false
  | .settings.meetings.slack_post_huddles = false
  | .settings.meetings.notes_dir = $notes' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
touch "$LOG"
TAILPID=""
trap '{ [[ -n "$TAILPID" ]] && kill $TAILPID; wait $TAILPID; } 2>/dev/null || true; quit_test_app; rm -rf "$WORK"' EXIT

FAILED=0
for MODE in $MODES; do
  quit_test_app
  : > "$WORK/log.txt"
  tail -F -n0 "$LOG" >> "$WORK/log.txt" 2>/dev/null &
  TAILPID=$!
  sleep 1
  open -g --env SOKKI_TEST_MIC="$WORK/silence.wav" --env SOKKI_TEST_MEETING=1 \
    --env SOKKI_TEST_SYSTEM="$WORK/silence.wav" --env SOKKI_TEST_CALL_ENDS="$MODE" -a "$TESTAPP"
  END=$(( $(date +%s) + 70 ))
  while (( $(date +%s) < END )); do
    sleep 1
    grep -qE "Test mode: call end \($MODE\): (stopped|still recording|nothing)" "$WORK/log.txt" && break
  done
  sleep 1
  grep -E "Test mode: call end|records the .* call|call ended|kept recording|Meeting stop via call" "$WORK/log.txt" || true
  { kill $TAILPID; wait $TAILPID; } 2>/dev/null || true
  TAILPID=""
  case "$MODE" in
    keep) WANT="still recording" ;;
    *) WANT="stopped" ;;
  esac
  if grep -qE "Test mode: call end \($MODE\): $WANT" "$WORK/log.txt"; then
    echo "== $MODE: ok ($WANT)"
  else
    echo "== $MODE: FAILED (wanted: $WANT)"
    FAILED=1
  fi
done
exit $FAILED
