#!/bin/bash
# End-to-end test of "Keep what I write" (sokki-journal) without a person, in
# the separate test app ("Sokki Test", org.sokki.test). A helper app
# (scripts/journal-probe) holds a text area and a password field in a window
# far off screen; the test app reads its focused field through Accessibility
# (SOKKI_TEST_JOURNAL_PID) while a test dictation runs. Then the script
# checks the journal files.
#
#   scripts/test-dictation.sh --build   # once per code change
#   scripts/test-journal.sh
#
# Safety, as in test-dictation.sh: the app is started only with `open`,
# never its binary from a shell; nothing runs tccutil or touches Robin's
# Sokki, its settings or its data. Nothing appears on screen.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
JOURNAL="$DATA/typing_log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"

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
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc" \
  && security find-identity -p codesigning 2>/dev/null | grep -q '"Sokki Dev"'; then
  echo "The test app is signed ad hoc; rebuild it: scripts/test-dictation.sh --build" >&2
  exit 1
fi

WORK="$(mktemp -d)"
PROBE_PID=""
cleanup() {
  [[ -n "$PROBE_PID" ]] && kill "$PROBE_PID" 2>/dev/null || true
  exec 3>&- 2>/dev/null || true
  quit_test_app
  rm -rf "$WORK"
}
trap cleanup EXIT

# The helper app, built as a bundle so it has a bundle id.
PROBE="$WORK/JournalProbe.app"
mkdir -p "$PROBE/Contents/MacOS"
swiftc -O -suppress-warnings "$REPO/scripts/journal-probe/JournalProbe.swift" -o "$PROBE/Contents/MacOS/JournalProbe"
cat > "$PROBE/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>org.sokki.journal-probe</string>
<key>CFBundleExecutable</key><string>JournalProbe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
</dict></plist>
EOF

# A dictation the test app runs 3 s after launch, made-up words only.
say -v Anna -o "$WORK/say.aiff" "Das neue Dashboard läuft jetzt auf Vercel."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"
HOLD=$(echo "$(afinfo "$WORK/say.wav" | awk '/estimated duration/ {print $3}') + 1" | bc)

mkdir -p "$DATA"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.keep_what_i_write = false
  | .settings.journal_excluded_apps = []
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
rm -rf "$JOURNAL"

mkfifo "$WORK/in"
PROBE_KEYS="$WORK/keys" "$PROBE/Contents/MacOS/JournalProbe" < "$WORK/in" > "$WORK/probe.out" &
PROBE_PID=$!
exec 3>"$WORK/in"
send() { echo "$*" >&3; }
sleep 1

quit_test_app
sleep 1
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_HOLD_S="$HOLD" \
  --env SOKKI_TEST_JOURNAL_PID="$PROBE_PID" --env SOKKI_TEST_JOURNAL_KEYS="$WORK/keys" -a "$TESTAPP"
for _ in $(seq 1 60); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "journal reads process" && break
  sleep 0.5
done
sleep 2

# 1. The dictation (it starts 3 s after launch): the text area has focus,
#    the "paste" arrives, a word gets fixed, the message is sent.
send text ""
for _ in $(seq 1 120); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "Test mode, not pasted" && break
  sleep 0.5
done
PASTED="$(tail -c +"$FROM" "$LOG" | grep "Test mode, not pasted" | tail -1 | sed 's/.*the stop): //')"
send paste "$PASTED"; sleep 1.5
# Fix the last word by hand: backspace over it, type the right one.
LAST="${PASTED##* }"
send back "${#LAST}"; sleep 1
send type "Vercel. Danke"; sleep 1.5
send clear; sleep 1.5
# 2. A message typed and sent: one entry with the last text.
send type "Hallo Jorinde"; sleep 1
send paste " (Kelvaro Mendtz)"; sleep 1
send type ", bis morgen"; sleep 1.5
send clear; sleep 1.5
# 3. A password field: never kept. Leaving the text area for it keeps the draft.
send type "Rechnung an Brannwick"; sleep 1.5
send secure "geheim-Probe-4711"; sleep 1.5
# 4. A hung app: reads time out, nothing breaks, nothing is split.
send text ""; sleep 1
send type "Vor dem Hänger"; sleep 1.5
send hang 2; sleep 0.5
send type " und danach"; sleep 1.5
send clear; sleep 1.5
# 5. An excluded app: nothing more is kept.
jq '.settings.journal_excluded_apps = ["org.sokki.journal-probe"]' "$DATA/settings_store.json" > "$WORK/s.json" \
  && cp "$WORK/s.json" "$DATA/settings_store.json"
sleep 2.5
send type "Nicht behalten"; sleep 1.5
send clear; sleep 1.5
send quit

echo "--- log"
tail -c +"$FROM" "$LOG" | grep -iE "journal|Keep what I write|not pasted|ERROR" | grep -v "Loaded settings" || true
echo "--- journal"
cat "$JOURNAL"/journal-*.jsonl 2>/dev/null || echo "(no journal file)"
echo "--- checks"
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
ALL="$(cat "$JOURNAL"/journal-*.jsonl 2>/dev/null || true)"
check "a sent message is one entry" '[[ "$(echo "$ALL" | jq -r "select(.text == \"Hallo Jorinde (Kelvaro Mendtz), bis morgen\") | .app")" == org.sokki.journal-probe ]]'
# The typed parts of that message, put together: the pasted name is not among them.
TYPED_PARTS="$(echo "$ALL" | jq -r 'select(.text | startswith("Hallo Jorinde")) | . as $e | [.typed[] | $e.text[.[0]:.[1]]] | join("|")')"
check "typed and pasted are told apart" '[[ "$TYPED_PARTS" == "Hallo Jorinde|, bis morgen" ]]'
check "a dictation is not typed, the typed fix is" '[[ "$(echo "$ALL" | jq -r "select(.text | endswith(\"Danke\")) | . as \$e | [.typed[] | \$e.text[.[0]:.[1]]] | join(\"|\")")" == "Vercel. Danke" ]]'
check "leaving for a password field keeps the draft" 'echo "$ALL" | jq -e "select(.text == \"Rechnung an Brannwick\")" >/dev/null'
check "a hung app doesn't split a message" '[[ "$(echo "$ALL" | jq -r "select(.text | startswith(\"Vor dem\")) | .text")" == "Vor dem Hänger und danach" ]]'
check "the password is never kept" '! grep -rq "geheim-Probe" "$JOURNAL" 2>/dev/null'
check "the dictation is attached" 'echo "$ALL" | jq -e "select(.dictated | length > 0) | select(.text | endswith(\"Danke\"))" >/dev/null'
check "an excluded app is not kept" '! grep -rq "Nicht behalten" "$JOURNAL" 2>/dev/null'
check "files are private" '[[ "$(stat -f %Lp "$JOURNAL"/journal-*.jsonl | sort -u)" == 600 ]]'
exit $fail
