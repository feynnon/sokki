#!/bin/bash
# End-to-end test of the `sokki` command line against a test app, without a person: made-up
# meetings and dictations (crates/sokki-index fixtures), the app in headless test mode
# (src-tauri/src/test_mic.rs), then the CLI checks the socket API, the index the app keeps current,
# a dictation from SOKKI_TEST_MIC, and a speaker rename through the socket.
#
#   TEST_ID=org.sokki.testcli "TEST_NAME=Sokki Test CLI" scripts/test-dictation.sh --build   # once
#   scripts/test-cli.sh
#
# Its own test app and data folder (TEST_ID, TEST_NAME as for test-dictation.sh). Started only with
# `open -g`; quit by a signal to its own path. Never touches the installed Sokki or its data.
set -uo pipefail
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
W="$(cd "$(dirname "$0")/.." && pwd)"
S="$(mktemp -d)"
trap 'rm -rf "$S"' EXIT
CLI="$W/target/cli/debug/sokki"
(cd "$W/crates/sokki" && CARGO_TARGET_DIR="$W/target/cli" cargo build -q) || { echo "can't build the CLI"; exit 1; }
ID="${TEST_ID:-org.sokki.testcli}"
TESTAPP="$HOME/Applications/${TEST_NAME:-Sokki Test CLI}.app"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
REAL="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
export SOKKI_DATA_DIR="$DATA"
unset SOKKI_SOCKET SOKKI_SCOPE SOKKI_HOOK
fails=0
ok() { echo "  ok    $*"; }
bad() { echo "  FAIL  $*"; fails=$((fails + 1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1"; fi; }
quit_app() {
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || return 0
  for _ in $(seq 1 20); do pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0; sleep 0.5; done
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

[[ -d "$TESTAPP" ]] || { echo "no test app"; exit 1; }
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then echo "tccutil in the app; stop"; exit 1; fi
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc"; then echo "ad hoc signed; stop"; exit 1; fi
quit_app

echo "== made-up data in $DATA"
rm -rf "$DATA"
mkdir -p "$DATA"
(cd "$W/crates/sokki-index" && CARGO_TARGET_DIR="$W/target/cli" cargo run -q --example make_fixture --features fixtures -- "$DATA" transcripts)
# The app's settings: the installed Sokki's, minus everything that reaches outside it (as test-dictation.sh),
# plus agent access and a notes folder inside the test data.
jq --arg notes "$DATA/notes" '.settings.onboarding_completed = true
  | .settings.dictation_cleanup_enabled = false
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.overlay_style = "none"
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.auto_start_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.mail_notes_auto = false
  | .settings.meetings.slack_post_huddles = false
  | .settings.meetings.user_name = "Ich"
  | .settings.meetings.notes_dir = $notes
  | .settings.agent_access = "dictations"' "$REAL" > "$DATA/settings_store.json"
say -v Anna -o "$S/dict.aiff" "Bitte die Reisekostenabrechnung bis Donnerstag an die Buchhaltung schicken."
afconvert -f WAVE -d LEI16@16000 -c 1 "$S/dict.aiff" "$S/dict.wav"

echo "== start (open -g, test mode: dictates dict.wav 3 s after launch)"
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$S/dict.wav" --env SOKKI_TEST_HOLD_S=6 -a "$TESTAPP"
for _ in $(seq 1 60); do [[ -S "$DATA/sokki.sock" ]] && break; sleep 0.5; done
check "the app listens on its socket" '[[ -S "$DATA/sokki.sock" ]]'
check "socket only for this user (0600)" '[[ "$(stat -f %Lp "$DATA/sokki.sock")" == 600 ]]'
"$CLI" events > "$S/events.jsonl" 2>/dev/null &
EVENTS=$!
for _ in $(seq 1 40); do [[ -f "$DATA/index.sqlite" ]] && break; sleep 0.5; done
sleep 1
n=$(sqlite3 "$DATA/index.sqlite" "select count(*) from sources where kind='meeting'" 2>/dev/null || echo 0)
check "the app built the index at start ($n meetings)" '[[ "$n" == 3 ]]'

st=$("$CLI" status --json)
echo "  status: $st" | cut -c1-200
check "status over the socket" 'echo "$st" | jq -e ".ok and .state.meeting.phase == \"idle\"" >/dev/null'
notes=$("$CLI" notes --json)
check "notes over the socket: 3" '[[ "$(echo "$notes" | jq ".notes | length")" == 3 ]]'
sp=$("$CLI" speakers meeting-20261103-100000-001 --json)
check "speakers over the socket" 'echo "$sp" | jq -e ".speakers | map(.name) | index(\"Speaker 3\")" >/dev/null'

echo "== dictation"
for _ in $(seq 1 120); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -qE "Test mode, (not pasted|edited)" && break
  sleep 0.5
done
tail -c +"$FROM" "$LOG" | grep -E "Test mode|Local API|Index" | cut -c1-160
sleep 2
d=$("$CLI" search Reisekostenabrechnung --kind dictation --json 2>&1)
echo "  search: $(echo "$d" | jq -c '[.results[] | {cite, app, text}]' 2>/dev/null | cut -c1-240)"
check "the new dictation is searchable" '[[ "$(echo "$d" | jq ".results | length")" -ge 1 ]]'
check "it has the app it went into" 'echo "$d" | jq -e ".results[0].app != null" >/dev/null'
check "the event stream saw the dictation listen" 'grep -q "\"dictation\":\"listening\"" "$S/events.jsonl"'
check "the event stream got the hook event dictation.done" 'grep -q "\"event\":\"dictation.done\"" "$S/events.jsonl"'

echo "== rename a speaker through the socket; the app rewrites the note, the index follows"
"$CLI" name meeting-20261103-100000-001 "Speaker 3" "Kim Beispiel" --forget && ok "renamed" || bad "rename"
sleep 2
r=$("$CLI" search Datenschutzerklärung --person kim --section transcript --json)
echo "  search: $(echo "$r" | jq -c '[.results[] | {cite, speaker, speaker_id, person_id, channel}]' | cut -c1-240)"
check "found as Kim Beispiel, same speaker id s3" 'echo "$r" | jq -e ".results[0].speaker == \"Kim Beispiel\" and .results[0].speaker_id == \"s3\" and .results[0].person_id == \"kim-beispiel\"" >/dev/null'
check "the note has the new name" 'grep -q "Kim Beispiel" "$DATA"/notes/*Rahmenvertrag*.md'
check "the event stream got meeting.speakers_named" 'grep -q "\"event\":\"meeting.speakers_named\"" "$S/events.jsonl"'

echo "== quit"
kill "$EVENTS" 2>/dev/null
quit_app
sleep 1
[[ -e "$DATA/sokki.sock" ]] && echo "  note  a SIGTERM quit leaves the socket file (no RunEvent::Exit); the next start replaces it"
off=$("$CLI" status 2>&1)
check "status says not running" 'echo "$off" | grep -q "not running"'
g=$("$CLI" get m:meeting-20261103-100000-001@00:05 --window 30s --json)
check "search and get work with the app closed" '[[ "$(echo "$g" | jq ".results | length")" -ge 1 ]]'
echo "== start again over the left socket"
open -g -a "$TESTAPP"
for _ in $(seq 1 60); do "$CLI" status >/dev/null 2>&1 && break; sleep 0.5; done
check "a second start replaces the old socket" '"$CLI" status --json | jq -e .ok >/dev/null'
quit_app
echo "  log of reads: $(wc -l < "$DATA/agent-access.log") lines"
echo
[[ $fails == 0 ]] && echo "ALL PASSED" || { echo "$fails FAILED"; exit 1; }
