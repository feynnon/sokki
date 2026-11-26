#!/bin/bash
# Checks in the test app ("Sokki Test", org.sokki.test) that an install from
# 0.2.14 (settings schema 8, "Keep what I write" off, word learning off) comes
# up with the journal on and word learning on (settings migrations 9 and 11).
#
#   scripts/test-dictation.sh --build   # once per code change
#   scripts/test-journal-default.sh
#
# Safety as in test-dictation.sh: `open` only, headless (SOKKI_TEST_MIC),
# nothing touches Robin's Sokki.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_DATA="$HOME/Library/Application Support/org.sokki.app"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$REAL_DATA/settings_store.json}"

quit_test_app() {
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || return 0
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
# Another session's test build with the same id would take our settings and
# our run (single instance): stop rather than disturb it.
others="$(pgrep -fl 'Sokki Test[^/]*[.]app/Contents/MacOS/' | grep -vF "$TESTAPP/Contents/MacOS/" || true)"
if [[ -n "$others" ]]; then
  echo "Another Sokki Test build is running; not touching it:" >&2
  echo "$others" >&2
  exit 3
fi
WORK="$(mktemp -d)"
trap 'quit_test_app; rm -rf "$WORK"' EXIT
say -v Anna -o "$WORK/say.aiff" "Kurzer Test."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"

quit_test_app
mkdir -p "$DATA"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.settings_schema_version = 8
  | .settings.keep_what_i_write = false
  | .settings.learn_words = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" -a "$TESTAPP"
for _ in $(seq 1 120); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "Keep what I write: on" && break
  sleep 0.5
done
sleep 2
quit_test_app
sleep 1

echo "--- settings"
jq -c '.settings | {settings_schema_version, keep_what_i_write, learn_words}' "$DATA/settings_store.json"
echo "--- checks"
fail=0
S="$(jq -c '.settings' "$DATA/settings_store.json")"
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
check "the journal runs after the update" 'tail -c +"$FROM" "$LOG" | grep -q "Keep what I write: on"'
check "the setting is saved as on" 'echo "$S" | jq -e ".keep_what_i_write == true" >/dev/null'
check "word learning is on" 'echo "$S" | jq -e ".learn_words == true" >/dev/null'
check "the store is at schema 11" 'echo "$S" | jq -e ".settings_schema_version == 11" >/dev/null'
exit $fail
