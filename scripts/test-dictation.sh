#!/bin/bash
# End-to-end dictation test without a person, in a separate test app
# ("Sokki Test", dev.handymeetings.test) next to Robin's Sokki, which it never
# touches. Test mode (src-tauri/src/test_mic.rs): the microphone plays a WAV,
# nothing is pasted, no shortcuts, no permission resets. The script starts a
# dictation, stops it, and prints what the log says (live text, timings, the
# final text).
#
#   scripts/test-dictation.sh --build          # build the test app (once per code change)
#   scripts/test-dictation.sh                  # a German sentence spoken by `say` (to a file, silent)
#   scripts/test-dictation.sh audio.wav [12]   # your own 16 kHz mono WAV, stop after 12 s
#   OVERLAY=none scripts/test-dictation.sh ...  # the same run without live text (A/B)
#
# Safety, after the 2026-11-19 input freeze: the app is started only with
# `open`, never its binary from a shell; nothing here runs tccutil or touches
# Robin's Sokki, its settings or its data.
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ID=dev.handymeetings.test
NAME="Sokki Test"
TESTAPP="$HOME/Applications/$NAME.app"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/handy.log"
SOKKI_SETTINGS="$HOME/Library/Application Support/dev.handymeetings.app/settings_store.json"
# Quits the test app and only the test app (matched by its own path). It
# ignores a plain quit request, so it gets SIGTERM if still running after 5 s.
quit_test_app() {
  osascript -e "tell application id \"$ID\" to quit" 2>/dev/null || true
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" || true
}

if [[ "${1:-}" == "--build" ]]; then
  export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
  export CMAKE_POLICY_VERSION_MINIMUM=3.5 HANDY_BRAND=sokki VITE_HANDY_BRAND=sokki SOKKI_DEV_BUILD=1
  # Sign like the real app: an ad hoc signature changes with every build, so
  # macOS treats each build as a new app and would ask for permissions again.
  IDENTITY="Handy Meetings Dev"
  if security find-identity -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
    export APPLE_SIGNING_IDENTITY="$IDENTITY"
  else
    export APPLE_SIGNING_IDENTITY="-"
  fi
  source "$REPO/scripts/build-lock.sh"
  build_lock_acquire
  (cd "$REPO" && bun run tauri build --bundles app \
    --config src-tauri/brands/sokki/tauri.conf.json \
    --config "{\"identifier\":\"$ID\",\"productName\":\"$NAME\",\"bundle\":{\"createUpdaterArtifacts\":false}}")
  build_lock_release
  quit_test_app
  mkdir -p "$HOME/Applications"
  rm -rf "$TESTAPP"
  cp -R "$REPO/src-tauri/target/release/bundle/macos/$NAME.app" "$TESTAPP"
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
jq --arg overlay "${OVERLAY:-live}" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.overlay_style = $overlay
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false' "$SOKKI_SETTINGS" > "$DATA/settings_store.json"

# Never start a build that could still reset a macOS permission.
if strings "$TESTAPP/Contents/MacOS/handy" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi
# Never start an ad hoc build: older ones open macOS's permission window.
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc" \
  && security find-identity -p codesigning 2>/dev/null | grep -q '"Handy Meetings Dev"'; then
  echo "The test app is signed ad hoc; rebuild it: scripts/test-dictation.sh --build" >&2
  exit 1
fi

quit_test_app
sleep 1
FROM=$(($(stat -f %z "$LOG" 2>/dev/null || echo 0) + 1))
# The app starts the dictation itself 3 s after launch and stops it after HOLD.
echo "Dictating ${LENGTH}s of audio, stopping after ${HOLD}s…"
open -g --env SOKKI_TEST_MIC="$WAV" --env SOKKI_TEST_HOLD_S="$HOLD" -a "$TESTAPP"
for _ in $(seq 1 240); do
  tail -c +"$FROM" "$LOG" 2>/dev/null | grep -q "Test mode, not pasted" && break
  sleep 0.5
done

tail -c +"$FROM" "$LOG" \
  | grep -E "Test mode|Live text|Live long-form|previewed|Decoded long-form|chunks were decoded|not pasted|ERROR|WARN" \
  | grep -v "Loaded settings"

quit_test_app
