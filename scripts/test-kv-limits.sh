#!/bin/bash
# End-to-end test of the counter's daily KV limits (counter/, KNOWN_ISSUES
# "Cloudflare-Gratisgrenzen") without a person: a test app built against a
# local counter shares five recordings into a write budget of 3, so two get
# "try later"; then "Delete what was shared" runs against a delete budget of
# 2, so the app hears "being deleted"; the local counter's hourly runs then
# delete the rest.
#
# Once:
#   SOKKI_COUNTER_URL=http://localhost:8787 TEST_ID=org.sokki.test.kv \
#     TEST_NAME="Sokki Test KV" scripts/test-dictation.sh --build
# Then:
#   scripts/test-kv-limits.sh
#
# It starts its own `wrangler dev` on :8787 with local storage in a temp
# folder (never the live counter). Like test-dictation.sh: the app is started
# only with `open`, never Robin's Sokki.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ID="${TEST_ID:-org.sokki.test.kv}"
NAME="${TEST_NAME:-Sokki Test KV}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
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
grep -a -q "http://localhost:8787" "$TESTAPP/Contents/MacOS/sokki" \
  || { echo "The test app wasn't built with SOKKI_COUNTER_URL=http://localhost:8787" >&2; exit 1; }
if lsof -iTCP:8787 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "Something already listens on :8787 (another session's counter?)" >&2
  exit 1
fi

WORK="$(mktemp -d)"
DEV_PID=""
cleanup() {
  quit_test_app
  [[ -n "$DEV_PID" ]] && kill "$DEV_PID" 2>/dev/null || true
  pkill -f "wrangler dev --local --persist-to $WORK" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

# The local counter: write budget 3, delete budget 2.
(cd "$REPO/counter" && npx wrangler d1 execute verbatim-counter --local --persist-to "$WORK/wd" \
  --file schema.sql >/dev/null 2>&1)
(cd "$REPO/counter" && exec npx wrangler dev --local --persist-to "$WORK/wd" --port 8787 \
  --test-scheduled --var KV_WRITE_BUDGET:3 --var KV_DELETE_BUDGET:2 >"$WORK/dev.log" 2>&1) &
DEV_PID=$!
for _ in $(seq 1 60); do curl -sf localhost:8787/total >/dev/null && break; sleep 1; done
curl -sf localhost:8787/total >/dev/null || { cat "$WORK/dev.log"; exit 1; }
q() {
  (cd "$REPO/counter" && npx wrangler d1 execute verbatim-counter --local --persist-to "$WORK/wd" \
    --json --command "$1" 2>/dev/null) | jq -c '.[0].results'
}
kv_files() {
  (cd "$REPO/counter" && npx wrangler kv key list --binding AUDIO --local --persist-to "$WORK/wd" \
    2>/dev/null) | jq length
}

# The test app: Robin's settings minus everything that reaches outside, and
# five recordings to share under a fresh id.
quit_test_app
rm -rf "$DATA/voice-data"
mkdir -p "$DATA/voice-data/clips"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
say -v Anna -o "$WORK/say.aiff" "Hallo, das ist ein kurzer Test."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"
NOW=$(date +%s)
for i in 1 2 3 4 5; do
  STEM="dictation-20261125-20000000$i-ab1$i"
  cp "$WORK/say.wav" "$DATA/voice-data/clips/$STEM.wav"
  jq -nc --arg f "clips/$STEM.wav" --argjson t "$NOW" \
    '{file: $f, text: "Hallo, das ist ein kurzer Test.", language: "de", seconds: 2.0, source: "dictation", created: $t}' \
    >> "$DATA/voice-data/manifest.jsonl"
done
INSTALL="$(openssl rand -hex 16)"
jq -n --arg id "$INSTALL" '{support: true, improve: false, install_id: $id}' > "$DATA/voice-data/config.json"

# 1. Sharing: the app reports and uploads a minute after start.
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_HOLD_S=3 -a "$TESTAPP"
echo "Waiting for the uploads (about 70 s)…"
for _ in $(seq 1 120); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "uploads for today are used up" && break
  sleep 1
done
quit_test_app
echo "--- app log"
tail -c +"$FROM" "$LOG" | grep -E "Your data" || true
echo "--- counter: $(q "SELECT COUNT(*) AS donations FROM donations"), KV files $(kv_files), $(q "SELECT kind, n FROM kv_ops")"
PAUSED=$(jq '.uploads_paused_until // 0' "$DATA/voice-data/config.json")
echo "--- uploads paused until $(date -u -r "$PAUSED" 2>/dev/null || echo never)"

# 2. "Delete what was shared", the app's own code path (test mode).
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_FORGET=current -a "$TESTAPP"
for _ in $(seq 1 60); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "Test mode: forget" && break
  sleep 1
done
quit_test_app
echo "--- app log"
tail -c +"$FROM" "$LOG" | grep -E "Your data|Test mode: forget" || true
echo "--- counter right after: $(q "SELECT (SELECT COUNT(*) FROM donations) AS donations, (SELECT COUNT(*) FROM installs) AS installs, (SELECT COUNT(*) FROM forget_audio) AS queued"), KV files $(kv_files)"

# 3. The next day: the hourly run deletes the rest.
q "DELETE FROM kv_ops" >/dev/null
curl -s "localhost:8787/__scheduled?cron=23+*+*+*+*" >/dev/null
echo "--- next day, after one hourly run: $(q "SELECT COUNT(*) AS queued FROM forget_audio"), KV files $(kv_files)"

OK=1
grep -q "uploads for today are used up" "$LOG" || OK=0
tail -c +"$FROM" "$LOG" | grep -q "Test mode: forget Ok(true); sharing off" || OK=0
[[ "$(q "SELECT COUNT(*) AS n FROM forget_audio" | jq '.[0].n')" == 0 ]] || OK=0
[[ "$(kv_files)" == 0 ]] || OK=0
[[ "$PAUSED" -gt "$NOW" ]] || OK=0
[[ $OK == 1 ]] && echo OK || { echo FAIL; exit 1; }
