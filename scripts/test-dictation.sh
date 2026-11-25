#!/bin/bash
# End-to-end dictation test without a person, in a separate test app
# ("Sokki Test", org.sokki.test) next to Robin's Sokki, which it never
# touches. Test mode (src-tauri/src/test_mic.rs): the microphone plays a WAV,
# nothing is pasted, no shortcuts, no permission resets. The script starts a
# dictation, stops it, and prints what the log says (live text, timings, the
# final text).
#
#   scripts/test-dictation.sh --build          # build the test app (once per code change)
#   PROFILE=release scripts/test-dictation.sh --build  # … with the shipping profile
#   scripts/test-dictation.sh                  # a German sentence spoken by `say` (to a file, silent)
#   scripts/test-dictation.sh audio.wav [12]   # your own 16 kHz mono WAV, stop after 12 s
#   OVERLAY=none scripts/test-dictation.sh ...  # the same run without live text (A/B)
#   CLEANUP=1 scripts/test-dictation.sh ...     # with the dictation cleanup on
#   (needs Sokki's cleanup model; the "not pasted" line then shows the
#   cleaned text, "live text" the transcript)
#   SOKKI_TEST_LLM=1 scripts/test-dictation.sh  # also start the built-in language
#                                               # model once (bundled server, Sokki's
#                                               # downloaded model, linked, not copied)
#
# Safety, after the 2026-11-19 input freeze: the app is started only with
# `open`, never its binary from a shell; nothing here runs tccutil or touches
# Robin's Sokki, its settings or its data.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
# TEST_ID/TEST_NAME: a second test app, so two sessions can test side by side.
ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
# Robin's settings: under the installed Sokki's bundle id.
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
# Quits the test app if it runs, and only the test app (matched by its own
# path). No AppleScript: on 2026-11-19 a Sokki Test without test mode showed
# setup and macOS's permission window 4 s after this script sent `tell
# application … to quit` to the stopped app. Not proven to be the cause, but
# a signal reaches only a running process.
quit_test_app() {
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || return 0
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -KILL -f "^$TESTAPP/Contents/MacOS/" 2>/dev/null || true
}

if [[ "${1:-}" == "--build" ]]; then
  export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5 SOKKI_DEV_BUILD=1
  # Sign like the real app: an ad hoc signature changes with every build, so
  # macOS treats each build as a new app and would ask for permissions again.
  # The release certificate when this Mac has it, else the self-made one.
  IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' | head -1)"
  IDENTITY="${IDENTITY:-Sokki Dev}"
  if security find-identity -p codesigning 2>/dev/null | grep -qF "\"$IDENTITY\""; then
    export APPLE_SIGNING_IDENTITY="$IDENTITY"
  else
    export APPLE_SIGNING_IDENTITY="-"
  fi
  source "$REPO/scripts/build-lock.sh"
  build_lock_acquire
  "$REPO/scripts/build-llama-server.sh"
  # PROFILE: fastrelease (default; no LTO, see src-tauri/Cargo.toml) or
  # release (exactly what scripts/release.sh ships, several times slower).
  PROFILE="${PROFILE:-fastrelease}"
  (cd "$REPO" && bun run tauri build --bundles app \
    --config src-tauri/brands/sokki/tauri.conf.json \
    --config "{\"identifier\":\"$ID\",\"productName\":\"$NAME\",\"bundle\":{\"createUpdaterArtifacts\":false}}" \
    -- --profile "$PROFILE")
  build_lock_release
  quit_test_app
  mkdir -p "$HOME/Applications"
  rm -rf "$TESTAPP"
  cp -R "$REPO/src-tauri/target/$PROFILE/bundle/macos/$NAME.app" "$TESTAPP"
  echo "Built $TESTAPP"
  exit 0
fi

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
WAV="${1:-}"
if [[ -z "$WAV" ]]; then
  WAV="$WORK/say.wav"
  say -v Anna -o "$WORK/say.aiff" "Hallo, das ist ein Test für Sokki. Ich spreche jetzt ein paar Sätze, damit man sieht, ob der Text schon beim Sprechen erscheint. Danach lasse ich die Taste los, und der ganze Text wird eingefügt."
  afconvert -f WAVE -d LEI16@16000 -c 1 "$WORK/say.aiff" "$WAV"
fi
WAV="$(cd "$(dirname "$WAV")" && pwd)/$(basename "$WAV")"
LENGTH=$(afinfo "$WAV" | awk '/estimated duration/ {print $3}')
HOLD="${2:-$(echo "$LENGTH + 1" | bc)}"

# The test app's settings: Robin's, minus everything that reaches outside it.
mkdir -p "$DATA"
jq --arg overlay "${OVERLAY:-live}" --argjson cleanup "$([[ -n "${CLEANUP:-}" ]] && echo true || echo false)" '.settings.onboarding_completed = true
  | .settings.dictation_cleanup_enabled = $cleanup
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.overlay_style = $overlay
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

# Never start a build that could still reset a macOS permission.
if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi
# Never start an ad hoc build: older ones open macOS's permission window.
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc" \
  && security find-identity -p codesigning 2>/dev/null | grep -q '"Sokki Dev"'; then
  echo "The test app is signed ad hoc; rebuild it: scripts/test-dictation.sh --build" >&2
  exit 1
fi

LLM_ENV=()
if [[ -n "${SOKKI_TEST_LLM:-}" ]]; then
  MODEL="$HOME/Library/Application Support/$REAL_ID/meeting-models/llm/Qwen3.5-9B-Q4_K_M.gguf"
  [[ -f "$MODEL" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }
  mkdir -p "$DATA/meeting-models/llm"
  [[ -f "$DATA/meeting-models/llm/$(basename "$MODEL")" ]] || ln "$MODEL" "$DATA/meeting-models/llm/"
  LLM_ENV=(--env SOKKI_TEST_LLM=1)
fi

# The dictation cleanup model (Gemma 4 E4B + its MTP draft), when Sokki has
# it: linked, not copied. Without it the dictation is pasted uncleaned.
REAL_LLM="$HOME/Library/Application Support/$REAL_ID/meeting-models/llm"
for f in gemma-4-E4B-it-Q4_K_M.gguf mtp-gemma-4-E4B-it.gguf; do
  if [[ -f "$REAL_LLM/$f" && ! -f "$DATA/meeting-models/llm/$f" ]]; then
    mkdir -p "$DATA/meeting-models/llm"
    ln "$REAL_LLM/$f" "$DATA/meeting-models/llm/"
  fi
done

quit_test_app
sleep 1
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
# The app starts the dictation itself 3 s after launch and stops it after HOLD.
echo "Dictating ${LENGTH}s of audio, stopping after ${HOLD}s…"
open -g --env SOKKI_TEST_MIC="$WAV" --env SOKKI_TEST_HOLD_S="$HOLD" ${LLM_ENV[@]+"${LLM_ENV[@]}"} -a "$TESTAPP"
for _ in $(seq 1 240); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "Test mode, not pasted" && break
  sleep 0.5
done
if [[ -n "${SOKKI_TEST_LLM:-}" ]]; then
  for _ in $(seq 1 240); do
    tail -c +"$FROM" "$LOG" 2>/dev/null | grep -qE "language model (done|did not start)" && break
    sleep 0.5
  done
fi

tail -c +"$FROM" "$LOG" \
  | grep -E "Test mode|anguage model|Live text|Live long-form|previewed|Decoded long-form|chunks were decoded|not pasted|Dictation clean|ERROR|WARN" \
  | grep -v "Loaded settings"

quit_test_app
# The summary model (SOKKI_TEST_LLM) and the cleanup model each run in a
# llama-server that must stop with the app.
sleep 2
if pgrep -f "^$TESTAPP/Contents/Resources/resources/llm/llama-server" >/dev/null; then
  echo "FAIL: a language model server outlived the app" >&2
  exit 1
fi
echo "No language model server is left."
