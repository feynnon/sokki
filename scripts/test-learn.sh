#!/bin/bash
# End-to-end test of word learning (src-tauri/src/learn_words.rs) in the
# separate test app ("Sokki Test", org.sokki.test): a journal of made-up
# messages goes into the test app's data, the app runs word learning once
# (SOKKI_TEST_LEARN=1: spell checker, the built-in language model, the hear
# test with Parakeet), and the script checks what landed in its settings.
#
#   scripts/test-dictation.sh --build   # once per code change
#   scripts/test-learn.sh
#
# Heavy job: it loads the built-in language model (~6 GB) for a minute.
# It waits while one of Robin's meetings records or is processed.
# Safety as in test-dictation.sh: `open` only, nothing touches Robin's Sokki.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
REAL_DATA="$HOME/Library/Application Support/$REAL_ID"
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
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi
MODEL="$REAL_DATA/meeting-models/llm/Qwen3.5-9B-Q4_K_M.gguf"
[[ -f "$MODEL" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }

# Robin's meetings come first.
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
trap 'quit_test_app; rm -rf "$WORK"' EXIT
say -v Anna -o "$WORK/say.aiff" "Kurzer Test."
afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WORK/say.wav"

mkdir -p "$DATA/meeting-models/llm"
[[ -f "$DATA/meeting-models/llm/$(basename "$MODEL")" ]] || ln "$MODEL" "$DATA/meeting-models/llm/"
jq '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.keep_what_i_write = true
  | .settings.learn_words = true
  | .settings.custom_words = ["Sokki"]
  | .settings.vocabulary_learned = []
  | .settings.vocabulary_heard_as = []
  | .settings.vocabulary_rejected = []
  | .settings.learn_words_last_run = 0
  | .settings.spoken_languages = ["de", "en"]
  | .settings.meetings.glossary = []
  | .settings.meetings.llm_provider = "builtin"
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

# A made-up journal: names typed twice, a typo twice, code, a fixed dictation.
J="$DATA/typing_log"
rm -rf "$J"; mkdir -p "$J"
NOW=$(($(date +%s) * 1000))
# typed: the whole text was typed key by key.
line() { jq -cn --arg t "$1" --argjson w "$NOW" '{t:"entry",opened:$w,written:$w,app:"com.example.chat",hint:"",prefill:"",text:$t,typed:[[0,($t|length)]]}'; }
# pasted: nothing was typed (copied text, another dictation tool).
pasted() { jq -cn --arg t "$1" --argjson w "$NOW" '{t:"entry",opened:$w,written:$w,app:"com.example.chat",hint:"",prefill:"",text:$t,typed:[]}'; }
{
  line "Die Rechnung an Brannwick geht morgen raus."
  line "Hat Brannwick schon geantwortet?"
  line "Tarnisio läuft jetzt auf der neuen Domain."
  line "Kannst du Tarnisio heute noch testen?"
  line "Send me the Quorbit deck please."
  line "The Quorbit numbers look good."
  line "Bis Freiatg dann."
  line "Passt, Freiatg geht."
  line "The useEffect hook fires twice."
  line "Remove the useEffect again."
  pasted "Kelvaro Mendtz hat zugesagt."
  pasted "Gruß an Kelvaro Mendtz"
  jq -cn --argjson w "$NOW" '{t:"entry",opened:$w,written:$w,app:"com.example.chat",hint:"",prefill:"",
    text:"Das neue Dashboard läuft auf Vercel. Danke",
    dictated:[{raw:"Das neue Dashboard läuft auf Fassel.",at:$w}], typed:[[29,42]]}'
} > "$J/journal-$(date +%Y-%m-%d).jsonl"

quit_test_app
sleep 1
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
open -g --env SOKKI_TEST_MIC="$WORK/say.wav" --env SOKKI_TEST_LEARN=1 -a "$TESTAPP"
for _ in $(seq 1 600); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -qE "Word learning: [0-9]+ candidates|Word learning stopped" && break
  sleep 0.5
done
quit_test_app
sleep 2

echo "--- log"
tail -c +"$FROM" "$LOG" | grep -E "Word learning|language model|ERROR" | grep -v "Loaded settings" || true
echo "--- learned"
jq -c '.settings | {vocabulary_learned, vocabulary_heard_as}' "$DATA/settings_store.json"
echo "--- checks"
fail=0
S="$(jq -c '.settings' "$DATA/settings_store.json")"
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
learned() { echo "$S" | jq -e --arg w "$1" '.vocabulary_learned | index($w)' >/dev/null; }
check "names typed twice are learned" 'learned Brannwick && learned Tarnisio && learned Quorbit'
check "a typo is not learned" '! learned Freiatg'
check "code is not learned" '! learned useEffect'
check "pasted names are not learned" '! learned Kelvaro && ! learned Mendtz'
check "a fixed dictation is learned as a pair" 'echo "$S" | jq -e ".vocabulary_heard_as[] | select(.heard == \"Fassel\" and .word == \"Vercel\" and .source == \"correction\")" >/dev/null'
check "the language model is gone" '! pgrep -f "^$TESTAPP/Contents/Resources/resources/llm/llama-server" >/dev/null'
exit $fail
