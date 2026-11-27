#!/bin/bash
# End-to-end test of the one dictionary (src-tauri/src/dictionary.rs) in a test
# app: with `autocorrect_add_words` on, the Wörterbuch's words are what Sokki
# teaches the Mac's spell checker; a word taken out is taken back; with the
# switch off, everything Sokki taught is taken back. Test mode never touches the
# real Mac's dictionary, so the script checks the list Sokki keeps
# (spelling_taught_words.txt) and the log, and that the real spell checker
# still doesn't know the made-up words. The pill itself is tested headless in
# tests/word-pill.spec.ts; the typed word that brings it up needs a keyboard
# (autocorrect is off in test mode).
#
#   TEST_ID=org.sokki.test.words TEST_NAME="Sokki Test Words" scripts/test-dictation.sh --build
#   scripts/test-word-pill.sh
#
# Safety as in test-dictation.sh: `open` only, nothing touches Robin's Sokki.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test.words}"
NAME="${TEST_NAME:-Sokki Test Words}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
TAUGHT="$DATA/spelling_taught_words.txt"

quit_test_app() {
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || return 0
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: TEST_ID=$ID TEST_NAME=\"$NAME\" scripts/test-dictation.sh --build" >&2; exit 1; }
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'quit_test_app; rm -rf "$WORK"' EXIT
say -v Anna -o "$WORK/say.aiff" "Kurzer Test."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"

# The real spell checker, asked from a separate process (never changed here).
cat > "$WORK/known.swift" <<'SWIFT'
import AppKit
let w = CommandLine.arguments[1]
print(NSSpellChecker.shared.hasLearnedWord(w) ? "learned" : "not learned")
SWIFT
swiftc -O "$WORK/known.swift" -o "$WORK/known" 2>/dev/null

mkdir -p "$DATA"
rm -f "$TAUGHT"
write_settings() {
  jq --argjson g "$1" --argjson on "$2" '.settings.onboarding_completed = true
    | .settings.start_hidden = true
    | .settings.autostart_enabled = false
    | .settings.update_checks_enabled = false
    | .settings.show_tray_icon = false
    | .settings.autocorrect_enabled = true
    | .settings.autocorrect_add_words = $on
    | .settings.meetings.glossary = $g
    | .settings.meetings.detection_enabled = false
    | .settings.meetings.phone_link_enabled = false' "$3" > "$DATA/settings_store.json.new"
  mv "$DATA/settings_store.json.new" "$DATA/settings_store.json"
}
since() { tail -c +"$FROM" "$LOG" 2>/dev/null; }
wait_log() {
  for _ in $(seq 1 60); do since | grep -qF "$1" && return 0; sleep 0.5; done
  return 1
}

quit_test_app
sleep 1
write_settings '["Brannwick", "Kelvaro Mendtz", "R2D2"]' true "$SOKKI_SETTINGS"
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
START=$FROM
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" -a "$TESTAPP"

fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

wait_log "(3 words to teach, 0 to take back)" || true
check "the switch on: the Wörterbuch's three words are taught" \
  'since | grep -qF "(3 words to teach, 0 to take back)"'
check "only words of letters, each name word on its own" \
  '[[ "$(tr "\n" " " < "$TAUGHT" 2>/dev/null)" == "Brannwick Kelvaro Mendtz " ]]'

# A word taken out of the Wörterbuch (the app applies an edit within a second).
FROM=$(($(stat -f %z "$LOG") + 1))
write_settings '["Kelvaro Mendtz", "R2D2"]' true "$DATA/settings_store.json"
wait_log "(0 words to teach, 1 to take back)" || true
check "a word taken out is taken back" \
  'since | grep -qF "(0 words to teach, 1 to take back)" && ! grep -qx Brannwick "$TAUGHT"'

# The switch off: everything Sokki taught goes back.
FROM=$(($(stat -f %z "$LOG") + 1))
write_settings '["Kelvaro Mendtz", "R2D2"]' false "$DATA/settings_store.json"
wait_log "(0 words to teach, 2 to take back)" || true
check "the switch off takes back everything Sokki taught" \
  'since | grep -qF "(0 words to teach, 2 to take back)" && [[ ! -s "$TAUGHT" ]]'

check "test mode left the real Mac's dictionary alone" \
  '[[ "$("$WORK/known" Brannwick)" == "not learned" && "$("$WORK/known" Mendtz)" == "not learned" ]]'
check "the log never names the words" '! tail -c +"$START" "$LOG" | grep -qE "Brannwick|Kelvaro|Mendtz"'
exit $fail
