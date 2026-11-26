#!/bin/bash
# Word learning on a copy of the user's own journal, in the test app
# ("Sokki Test", org.sokki.test), for a person to review: which typed words
# the local model would add, which it would not, and how the speech model
# hears them. Nothing is added to the real Sokki; the copy is deleted after.
#
#   scripts/test-dictation.sh --build   # once per code change
#   scripts/learn-review.sh <report.json>
#
# The report holds real typed words and sentences: keep it on this Mac
# (Robin's: the vault's reports/Sokki eval data/), never in the repo.
# Heavy job: loads the built-in language model (~6 GB) for a few minutes.
set -euo pipefail

OUT="${1:?usage: scripts/learn-review.sh <report.json>}"
ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_DATA="$HOME/Library/Application Support/org.sokki.app"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$REAL_DATA/settings_store.json}"
JOURNAL="${JOURNAL:-$REAL_DATA/typing_log}"

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
[[ -d "$JOURNAL" ]] || { echo "No journal at $JOURNAL" >&2; exit 1; }
# The built-in model: Gemma 4 E4B and its draft model.
LLM_FILES=(gemma-4-E4B-it-Q4_K_M.gguf mtp-gemma-4-E4B-it.gguf)
for f in "${LLM_FILES[@]}"; do
  [[ -f "$REAL_DATA/meeting-models/llm/$f" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }
done

while python3 - "$REAL_DATA/meetings/index.json" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(1)
items = d if isinstance(d, list) else d.get("meetings", d)
items = items if isinstance(items, list) else list(items.values())
sys.exit(0 if any((m.get("status") or {}).get("kind") in ("recording", "processing") for m in items) else 1)
PY
do echo "A meeting records or is processed; waiting…"; sleep 30; done

WORK="$(mktemp -d)"
cleanup() { quit_test_app; rm -rf "$WORK" "$DATA/typing_log"; }
trap cleanup EXIT
say -v Anna -o "$WORK/say.aiff" "Kurzer Test."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"

quit_test_app
mkdir -p "$DATA/meeting-models/llm"
for f in "${LLM_FILES[@]}"; do
  [[ -f "$DATA/meeting-models/llm/$f" ]] || ln "$REAL_DATA/meeting-models/llm/$f" "$DATA/meeting-models/llm/"
done
# The user's own words, languages and model; learning starts from nothing.
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.keep_what_i_write = true
  | .settings.learn_words = true
  | .settings.vocabulary_learned = []
  | .settings.vocabulary_heard_as = []
  | .settings.learn_words_last_run = 0
  | .settings.meetings.llm_provider = "builtin"
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
rm -rf "$DATA/typing_log"
cp -R "$JOURNAL" "$DATA/typing_log"
chmod -R go-rwx "$DATA/typing_log"

rm -f "$OUT"
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_LEARN=1 \
  --env SOKKI_TEST_LEARN_REPORT="$OUT" -a "$TESTAPP"
for _ in $(seq 1 1800); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -qE "Word learning: [0-9]+ candidates|Word learning stopped" && break
  sleep 0.5
done
quit_test_app
tail -c +"$FROM" "$LOG" | grep -E "Word learning|language model|ERROR" | grep -v "model says" || true
[[ -f "$OUT" ]] && chmod 600 "$OUT" && echo "report: $OUT ($(jq '.words | length' "$OUT") words)"
