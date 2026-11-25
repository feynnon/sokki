#!/bin/bash
# End-to-end test of "Help improve Sokki" (src-tauri/src/usage.rs) without a
# person. A test app built against a local counter replays setup as a fresh
# install, dictates a sentence and panics on a side thread. A minute after
# start it sends its daily report. The local counter (counter/, `wrangler dev`)
# passes the report on to a stand-in PostHog, and this prints what PostHog
# would get.
#
# Once:
#   (cd counter && npx wrangler d1 execute verbatim-counter --local --file schema.sql)
#   python3 scripts/mock-posthog.py OUT &
#   (cd counter && npx wrangler dev --local --var POSTHOG_HOST:http://127.0.0.1:8799 \
#      --var POSTHOG_API:http://127.0.0.1:8799) &
#   SOKKI_COUNTER_URL=http://localhost:8787 TEST_ID=org.sokki.test.usage \
#     TEST_NAME="Sokki Test Usage" scripts/test-dictation.sh --build
# Then:
#   TEST_ID=org.sokki.test.usage TEST_NAME="Sokki Test Usage" scripts/test-usage.sh OUT
#   AGAIN=1 scripts/test-usage.sh OUT   # a later report: no setup replay, no panic;
#                                       # checks that nothing is sent twice
#
# Like test-dictation.sh: started only with `open`, never Robin's Sokki.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test.usage}"
NAME="${TEST_NAME:-Sokki Test Usage}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
OUT="${1:?usage: $0 OUT (the folder mock-posthog.py writes to)}"
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

[[ -d "$TESTAPP" ]] || { echo "No test app at $TESTAPP" >&2; exit 1; }
curl -sf localhost:8787/total >/dev/null || { echo "No local counter on :8787" >&2; exit 1; }
strings "$TESTAPP/Contents/MacOS/sokki" | grep -c "http://localhost:8787" >/dev/null \
  || { echo "The test app wasn't built with SOKKI_COUNTER_URL=http://localhost:8787" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
say -v Anna -o "$WORK/say.aiff" "Hallo, das ist ein kurzer Test für Sokki."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"

mkdir -p "$DATA/voice-data"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
# "Help improve Sokki" on, report due now.
CONFIG="$DATA/voice-data/config.json"
[[ -f "$CONFIG" ]] || echo '{}' > "$CONFIG"
RAND="$(openssl rand -hex 16)"
jq --arg id "$RAND" '.improve = true | .last_usage = 0
  | .install_id = (if (.install_id // "") == "" then $id else .install_id end)' "$CONFIG" > "$WORK/c.json"
mv "$WORK/c.json" "$CONFIG"

quit_test_app
sleep 1
BEFORE=$(ls "$OUT" | wc -l)
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
EXTRA=(--env SOKKI_TEST_SETUP=1 --env SOKKI_TEST_PANIC=1)
[[ -n "${AGAIN:-}" ]] && EXTRA=()
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_HOLD_S=4 \
  ${EXTRA[@]+"${EXTRA[@]}"} -a "$TESTAPP"
echo "Waiting for the dictation and the daily report (about 70 s)…"
for _ in $(seq 1 200); do
  [[ $(ls "$OUT" | wc -l) -gt $BEFORE ]] && break
  sleep 1
done
sleep 2
quit_test_app
tail -c +"$FROM" "$LOG" | grep -E "Test mode, not pasted|daily report|counter not reached|panicked" || true
BATCH="$(ls -t "$OUT"/*post_batch_* 2>/dev/null | head -1)"
[[ -n "$BATCH" && $(ls "$OUT" | wc -l) -gt $BEFORE ]] || { echo "FAIL: nothing reached PostHog" >&2; exit 1; }
AGAIN="${AGAIN:-}" python3 - "$BATCH" <<'PY'
import json, sys
b = json.load(open(sys.argv[1]))
for e in b["batch"]:
    p = {k: v for k, v in e["properties"].items() if k not in ("$lib", "$geoip_disable", "$set", "$set_once")}
    print(f'{e["event"]:16} {e["timestamp"]}  {json.dumps(p, ensure_ascii=False)[:400]}')
import os
names = [e["event"] for e in b["batch"]]
if os.environ.get("AGAIN"):
    twice = [n for n in names if n in ("setup_step", "first_dictation", "first_start", "crash")]
    print("OK" if names[:1] == ["app_daily"] and not twice else f"FAIL: sent again: {twice}")
    sys.exit(1 if twice else 0)
need = ["app_daily", "setup_step", "first_dictation", "crash"]
missing = [n for n in need if n not in names]
print("OK" if not missing else f"FAIL: missing {missing}")
sys.exit(1 if missing else 0)
PY
