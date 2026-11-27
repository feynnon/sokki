#!/usr/bin/env bash
# Night test: an email address typed in a meeting's speaker card is kept
# (fix 05c97e7c, Robin's bug of 27 Nov 2026). Clicks through the real
# window of the test app "Sokki Test" (org.sokki.test, its own data folder):
# open a made-up meeting, open the card of speaker "Jonas Weber", type an
# example.com address, click outside, reopen, and check the field and the
# test app's voice-profiles.json. Prints PASS or the first step that failed.
#
# Never touches /Applications/Sokki.app or Robin's data: the test app's
# meetings, settings and voice profiles are backed up first and put back
# afterwards. Moves the mouse and types into the front window: run ONLY
# when nobody uses the Mac (the 03:00 run). It refuses while the Mac is in
# use (SOKKI_NIGHT_FORCE=1 overrides) and while Robin's Sokki records or
# processes a meeting. Build the test app first: scripts/test-dictation.sh --build
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
ID=org.sokki.test
NAME="Sokki Test"
TESTAPP="$HOME/Applications/$NAME.app"
DATA="$HOME/Library/Application Support/$ID"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
REAL_DATA="$HOME/Library/Application Support/$REAL_ID"
REAL_INDEX="$REAL_DATA/meetings/index.json"
ax="${TMPDIR:-/tmp}/sokki-night-ax"
MEETING=meeting-20261127-030000-001
TITLE="Planung Januar (Test)"
PERSON="Jonas Weber"
ADDRESS="jonas.test@example.com"

fail() { echo "FAIL: $*"; exit 1; }

# --- refusals -------------------------------------------------------------
[ "$ID" != "$REAL_ID" ] || fail "the test app's id is Robin's ($REAL_ID)"
[ "$(cd "$REAL_DATA" 2>/dev/null && pwd -P)" != "$(mkdir -p "$DATA" && cd "$DATA" && pwd -P)" ] \
  || fail "the test app's data folder is Robin's"
if grep -q '"kind": *"\(recording\|processing\|queued\)"' "$REAL_INDEX" 2>/dev/null; then
  fail "Robin's Sokki records or processes a meeting; try later"
fi
idle_s=$(( $(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {v=$NF} END {print v+0}') / 1000000000 ))
if [ "$idle_s" -lt 900 ] && [ -z "${SOKKI_NIGHT_FORCE:-}" ]; then
  fail "the Mac was used ${idle_s} s ago; this test moves the mouse, run it at night"
fi
[ -d "$TESTAPP" ] || fail "no test app; build it: scripts/test-dictation.sh --build"
bin_strings="$(strings "$TESTAPP/Contents/MacOS/sokki")"
if grep -q /usr/bin/tccutil <<<"$bin_strings"; then
  fail "the test app still contains tccutil; rebuild it from current meetings"
fi
grep -qE '(^|[^_])person_emails' <<<"$bin_strings" \
  || fail "the test app is older than the fix; rebuild it: scripts/test-dictation.sh --build"
swiftc -O -o "$ax" "$here/ax.swift" 2>/dev/null || fail "could not build the click helper"

# --- the test app, and only it -------------------------------------------
test_pids() { pgrep -f "^$TESTAPP/Contents/MacOS/" || true; }
quit_test_app() {
  local pids
  pids="$(test_pids)"
  [ -n "$pids" ] || return 0
  kill -TERM $pids 2>/dev/null || true
  for _ in $(seq 1 10); do
    [ -z "$(test_pids)" ] && return 0
    sleep 0.5
  done
  kill -KILL $pids 2>/dev/null || true
}

# --- a made-up meeting in the test app's own data folder -----------------
backup="$(mktemp -d)"
for f in settings_store.json meetings/index.json voice-profiles.json; do
  if [ -f "$DATA/$f" ]; then mkdir -p "$backup/$(dirname "$f")"; cp -p "$DATA/$f" "$backup/$f"; fi
done
restore() {
  quit_test_app
  for f in settings_store.json meetings/index.json voice-profiles.json; do
    if [ -f "$backup/$f" ]; then cp -p "$backup/$f" "$DATA/$f"; else rm -f "$DATA/$f"; fi
  done
  rm -rf "$DATA/meetings/$MEETING" "$DATA/night-notes" "$backup"
}
trap restore EXIT
quit_test_app

mkdir -p "$DATA/meetings/$MEETING" "$DATA/night-notes"
note="$DATA/night-notes/$MEETING.md"
cat > "$note" <<EOF
# $TITLE

## Summary
- $PERSON bucht den Raum.

## Transcript
[00:04] $PERSON: Ich buche den Raum.
[00:09] Mara Lind: Danke.
EOF
python3 - "$DATA" "$MEETING" "$TITLE" "$note" "$PERSON" <<'EOF'
import json, sys, datetime
data, mid, title, note, person = sys.argv[1:]
path = f"{data}/meetings/index.json"
try:
    index = json.load(open(path))
except FileNotFoundError:
    index = []
index = [r for r in index if r.get("id") != mid]
index.insert(0, {
    "id": mid,
    "started_at": datetime.datetime.now().astimezone().isoformat(),
    "duration_s": 600.0,
    "status": {"kind": "done"},
    "tracks": [],
    "source_app": "Zoom",
    "title": title,
    "note_path": note,
    "speakers": ["Mara Lind", person],
    "warnings": [],
    "summary_missing": None,
    "audio_deleted": True,
    "online": True,
})
json.dump(index, open(path, "w"))
# No profile for the person yet: the address must come from the card.
vp = f"{data}/voice-profiles.json"
try:
    store = json.load(open(vp))
    store["profiles"] = [p for p in store.get("profiles", []) if p.get("name") != person]
    json.dump(store, open(vp, "w"))
except FileNotFoundError:
    pass
EOF

# Settings: onboarding done, English, the window shows, nothing reaches
# outside the app (no mail, no detection, no updates, no tray).
jq '.settings.onboarding_completed = true
  | .settings.app_language = "en"
  | .settings.start_hidden = false
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.user_name = "Mara Lind"
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.auto_start_enabled = false
  | .settings.meetings.auto_start_set = true
  | .settings.meetings.mail_notes_auto = false
  | .settings.meetings.mail_notes_auto_set = true
  | .settings.meetings.slack_post_huddles = false
  | .settings.meetings.phone_link_enabled = false' \
  "${backup}/settings_store.json" > "$DATA/settings_store.json" 2>/dev/null \
  || fail "the test app has no settings yet; run scripts/test-dictation.sh once"

# Test mode (no shortcuts, no permission resets) with nothing driven: a
# silent "microphone" and SOKKI_TEST_AUTO with detection off.
silence="$backup/silence.wav"
python3 - "$silence" <<'EOF'
import sys, wave
w = wave.open(sys.argv[1], "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
w.writeframes(b"\0\0" * 1600); w.close()
EOF
open --env SOKKI_TEST_MIC="$silence" --env SOKKI_TEST_AUTO=1 -a "$TESTAPP"
for _ in $(seq 1 40); do [ -n "$(test_pids)" ] && break; sleep 0.5; done
pid="$(test_pids | head -1)"
[ -n "$pid" ] || fail "Sokki Test did not start"

# --- the clicks -----------------------------------------------------------
step() { echo "ok: $*"; }
"$ax" "$pid" click "Meetings" 20 || fail "step 1: no Meetings section in the window"
"$ax" "$pid" click "$TITLE" 10 || fail "step 2: the made-up meeting is not in the list"
step "the made-up meeting is open"
"$ax" "$pid" click "$PERSON" 10 || fail "step 3: no speaker chip \"$PERSON\""
"$ax" "$pid" wait "$PERSON is …" 5 || fail "step 3: the speaker card did not open"
"$ax" "$pid" click "Email addresses" 5 || fail "step 4: no email field in the card"
"$ax" type "$ADDRESS"
sleep 0.5
step "typed an address"
"$ax" "$pid" click "$TITLE" 5 || fail "step 5: nothing to click outside the card"
"$ax" "$pid" gone "$PERSON is …" 5 || fail "step 5: a click outside did not close the card"
step "closed the card with a click outside"
sleep 1
grep -q "$ADDRESS" "$DATA/voice-profiles.json" 2>/dev/null \
  || fail "step 6: the address is not in the test app's voice-profiles.json"
step "the address is saved on the person"
"$ax" "$pid" click "$PERSON" 5 || fail "step 7: no speaker chip to reopen"
"$ax" "$pid" wait "$PERSON is …" 5 || fail "step 7: the card did not reopen"
sleep 1
shown="$("$ax" "$pid" value "Email addresses")"
[ "$shown" = "$ADDRESS" ] || fail "step 7: the reopened card shows \"$shown\", not the address"
step "the reopened card shows the address"
"$ax" key escape
echo "PASS"
