#!/bin/bash
# The sample meeting end to end, without a person, in a test app next to
# Robin's Sokki, which it never touches. Test mode (src-tauri/src/test_mic.rs,
# SOKKI_TEST_SAMPLE): the app starts the sample as its button does
# (src-tauri/src/meeting/sample.rs), silently; the call plays into the
# recording in real time, it stops itself, and is processed with the live
# transcript, the speakers and the summary of the built-in language model.
# The script prints the log, the note, and CPU and memory every 5 s.
#
#   TEST_ID=org.sokki.test.sample TEST_NAME="Sokki Test Sample" scripts/test-dictation.sh --build
#   TEST_ID=org.sokki.test.sample TEST_NAME="Sokki Test Sample" scripts/test-sample.sh
#
# Needs Sokki's downloaded language model (linked, not copied). A heavy run:
# never while one of Robin's meetings records or processes.
#
# Safety, after the 2026-11-19 input freeze: the app is started only with
# `open`, never its binary from a shell; nothing here runs tccutil or touches
# Robin's Sokki, its settings or its data.
set -euo pipefail

ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
REAL="$HOME/Library/Application Support/$REAL_ID"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$REAL/settings_store.json}"

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it." >&2
  exit 1
fi
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc"; then
  echo "The test app is signed ad hoc; rebuild it with the \"Sokki Dev\" certificate." >&2
  exit 1
fi
INDEX="$REAL/meetings/index.json"
if [[ -f "$INDEX" ]] && jq -e '[(if type == "array" then . else (.meetings // []) end)[]
    | select(.status.kind == "recording" or .status.kind == "processing")] | length > 0' \
    "$INDEX" >/dev/null 2>&1; then
  echo "A meeting of Robin's is recording or processing; try again later." >&2
  exit 1
fi

# The test app's own processes, by PID.
app_pids() { pgrep -f "^$TESTAPP/Contents/MacOS/" || true; }
quit_test_app() {
  local pids
  pids="$(app_pids)"
  [[ -n "$pids" ]] || return 0
  kill -TERM $pids 2>/dev/null || true
  for _ in $(seq 1 10); do
    [[ -n "$(app_pids)" ]] || return 0
    sleep 0.5
  done
  pids="$(app_pids)"
  [[ -z "$pids" ]] || kill -KILL $pids 2>/dev/null || true
}

WORK="$(mktemp -d)"
mkdir -p "$DATA/meeting-models/llm"
# Robin's settings, minus everything that reaches outside the test app.
jq --arg notes "$WORK/notes" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.chat_notice_enabled = false
  | .settings.meetings.summary_enabled = true
  | .settings.meetings.llm_provider = "builtin"
  | .settings.meetings.notes_dir = $notes' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
# The models Sokki downloaded, linked, not copied.
for f in "$REAL"/meeting-models/*.* "$REAL"/meeting-models/llm/*.gguf; do
  [[ -f "$f" ]] || continue
  rel="${f#"$REAL/meeting-models/"}"
  [[ -e "$DATA/meeting-models/$rel" ]] || ln "$f" "$DATA/meeting-models/$rel"
done
[[ -f "$DATA/meeting-models/llm/Qwen3.5-9B-Q4_K_M.gguf" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }
if [[ -d "$REAL/models" ]]; then
  mkdir -p "$DATA/models"
  for f in "$REAL"/models/*; do
    [[ -e "$DATA/models/$(basename "$f")" ]] || cp -Rl "$f" "$DATA/models/" 2>/dev/null || true
  done
fi
# The microphone file test mode needs; the sample plays its own call.
python3 -I -c 'import sys, wave
w = wave.open(sys.argv[1], "wb"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(16000)
w.writeframes(bytes(32000)); w.close()' "$WORK/silence.wav"

quit_test_app
sleep 1
rm -rf "$DATA/meetings"
tail -F -n0 "$LOG" > "$WORK/log.txt" 2>/dev/null &
TAILPID=$!
# `wait` on the stopped tail returns 143; with `set -e` that would end the
# trap before the test app is quit, so it must not count as a failure.
trap '{ kill $TAILPID; wait $TAILPID; } 2>/dev/null || true; quit_test_app; rm -rf "$WORK"' EXIT
sleep 1
PATTERN="Test mode|Sample meeting|Live transcript|live transcript|Meeting step|Diarized|anguage model|summary|ERROR|WARN"
echo "Playing the sample meeting (about 2 minutes, silent)…"
open -g --env SOKKI_TEST_MIC="$WORK/silence.wav" --env SOKKI_TEST_SAMPLE=1 -a "$TESTAPP"

SAMPLES="$WORK/samples.txt"
for _ in $(seq 1 240); do
  sleep 5
  grep -qE "Test mode: (sample processed|the sample failed)" "$WORK/log.txt" && break
  PIDS=$(app_pids | paste -sd, -)
  [[ -n "$PIDS" ]] || { echo "The test app quit." >&2; break; }
  { ps -o %cpu=,rss= -p "$PIDS" || true; } \
    | awk -v t="$(date +%s)" '{c+=$1; m+=$2} END {printf "%s %.0f %.0f\n", t, c, m/1024}' >> "$SAMPLES"
  # The language model runs as its own process (llama-server).
  LLM=$(pgrep -f "llama-server.*$ID" | paste -sd, - || true)
  [[ -z "$LLM" ]] || ps -o rss= -p "$LLM" | awk '{m+=$1} END {printf "llama-server %.0f MB\n", m/1024}' >> "$WORK/llm.txt"
done
sleep 1
grep -E "$PATTERN" "$WORK/log.txt" | grep -v "Loaded settings" || true
if [[ -s "$SAMPLES" ]]; then
  awk '{n++; c+=$2; if ($2>cmax) cmax=$2; if ($3>mmax) mmax=$3}
    END {printf "App: CPU mean %.0f%%, max %.0f%%; memory max %d MB (%d samples)\n", c/n, cmax, mmax, n}' "$SAMPLES"
fi
[[ ! -s "$WORK/llm.txt" ]] || sort -k2 -n "$WORK/llm.txt" | tail -1 | sed 's/^/Most of /'
quit_test_app
NOTE="$(ls "$WORK"/notes/*.md 2>/dev/null | head -1 || true)"
if [[ -n "$NOTE" ]]; then
  echo "---------- $(basename "$NOTE")"
  cat "$NOTE"
  [[ -z "${KEEP_NOTE:-}" ]] || cp "$NOTE" "$KEEP_NOTE"
fi
