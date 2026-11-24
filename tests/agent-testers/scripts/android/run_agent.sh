#!/usr/bin/env bash
# Runs once the emulator is up (inside android-emulator-runner on CI, or by
# hand against a local emulator): applies the persona's phone settings,
# installs Sokki, puts the made-up test audio in place and hands the phone to
# Claude Code. Everything the agent writes goes to results/ (encrypted
# before upload), never to the public log.
#
# Based on scripts/android/run_agent.sh of pierridotite/mobile-agent-testers
# at d610b95a (MIT, see ../../LICENSE-mobile-agent-testers). Changes: Sokki's
# test audio and model, no `install -g` (permission dialogs are part of the
# test), a dollar cap per phone, and a narrower tool list.
#
# Env: PERSONA, LOCALE, FONT_SCALE, THEME, DEVICE, SPEECH (language of the test
#   audio), BRIEF, MODEL, BUDGET_USD, APP_ID (default org.sokki.app),
#   PRELOAD_MODEL (true: the speech model is already on the phone),
#   MODEL_FILE (path of the .gguf on this machine, for PRELOAD_MODEL),
#   AUDIO_DIR (test audio from make_audio.sh), RECORD_VIDEO.
# Run from tests/agent-testers/. Needs `adb` with exactly one device, or
# ANDROID_SERIAL set.
set -uo pipefail
cd "$(dirname "$0")/../.."
APP_ID=${APP_ID:-org.sokki.app}
OUT="results/$PERSONA"
mkdir -p "$OUT"
adb wait-for-device

# 1. Phone language (the emulator boots in en-US; google_apis images allow adb root)
if [ "$(adb shell getprop persist.sys.locale | tr -d '\r')" != "$LOCALE" ]; then
  adb root >/dev/null 2>&1; sleep 3; adb wait-for-device
  adb shell "setprop persist.sys.locale $LOCALE; setprop ctl.restart zygote"; sleep 10; adb wait-for-device
  for _ in $(seq 1 60); do [ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = 1 ] && break; sleep 3; done
fi

# 2. Sokki, without granting permissions: the dialogs are part of the test
adb install -r app.apk >/dev/null || { echo "::error::install failed"; exit 1; }
ACTIVITY=$(adb shell cmd package resolve-activity --brief -c android.intent.category.LAUNCHER "$APP_ID" | tail -1 | tr -d '\r')
launch() { adb shell am start -W -n "$ACTIVITY" >/dev/null 2>&1; }
adb shell settings put system font_scale "${FONT_SCALE:-1.0}"
[ "${THEME:-light}" = dark ] && adb shell cmd uimode night yes >/dev/null

# 3. Made-up test audio. keyboard.wav is what the Sokki keyboard "hears" in
# debug builds (SokkiVoice.testAudio); mic.wav plays on a loop into the
# emulator's microphone (see make_audio.sh), for the Dictate tab and meetings.
if [ -n "${AUDIO_DIR:-}" ] && [ -f "$AUDIO_DIR/$SPEECH/keyboard.wav" ]; then
  adb shell mkdir -p "/sdcard/Android/data/$APP_ID/files" >/dev/null 2>&1
  adb push "$AUDIO_DIR/$SPEECH/keyboard.wav" "/sdcard/Android/data/$APP_ID/files/sokki_test.wav" >/dev/null \
    || echo "keyboard test audio could not be placed"
  { echo "Sokki keyboard mic hears:"; cat "$AUDIO_DIR/$SPEECH/keyboard.txt"; echo
    if [ "${MIC_FEED:-false}" = true ]; then
      echo "Phone microphone hears, on a loop:"; cat "$AUDIO_DIR/$SPEECH/mic.txt"
    else
      echo "Phone microphone hears: silence (no microphone feed on this machine). Not a bug."
    fi; } > "$OUT/speech.txt"
else
  echo "No test audio: the phone hears silence." > "$OUT/speech.txt"
fi

# 4. The speech model (740 MB), unless the brief tests its download. Debug
# builds are debuggable, so run-as can write into the app's own storage.
if [ "${PRELOAD_MODEL:-true}" = true ] && [ -f "${MODEL_FILE:-}" ]; then
  name=$(basename "$MODEL_FILE")
  adb push "$MODEL_FILE" "/data/local/tmp/$name" >/dev/null
  launch; sleep 8   # creates the app's folders
  adb shell am force-stop "$APP_ID"
  adb shell run-as "$APP_ID" sh -c "'mkdir -p models && cp /data/local/tmp/$name models/$name'" \
    && echo "speech model preloaded" || echo "::warning::speech model could not be preloaded"
  adb shell rm -f "/data/local/tmp/$name"
fi

launch
adb logcat -c; sleep 5

# 5. Optional screen recording, kept in results/ (encrypted like the rest)
REC=""
if [ "${RECORD_VIDEO:-false}" = true ]; then
  adb shell settings put system show_touches 1
  rm -f /tmp/stop-rec
  ( i=0
    while [ ! -f /tmp/stop-rec ] && [ $i -lt 40 ]; do
      i=$((i + 1))
      adb shell screenrecord --bit-rate 4000000 --time-limit 180 "/sdcard/rec_$(printf %02d $i).mp4"
    done ) &
  REC=$!
fi

# 6. The agent: only these tools, a hard dollar cap, no web access
BRIEF=${BRIEF:-AGENT_BRIEF.md}
PROMPT="Read AGENT_BRIEF.md, then $BRIEF if it is another file, and personas.json, then do the mission until the final report. \
PERSONA=$PERSONA APP_ID=$APP_ID. Device: ${DEVICE}, font scale x${FONT_SCALE:-1.0}, theme ${THEME:-light}, language $LOCALE. \
What the test audio says is in $OUT/speech.txt."
claude -p "$PROMPT" \
  --model "${MODEL:-sonnet}" --max-turns 200 --max-budget-usd "${BUDGET_USD:-0.80}" --output-format json \
  --allowedTools "Bash(adb shell input:*),Bash(python3 scripts/android/screen.py:*),Bash(python3 scripts/android/type.py:*),Bash(bash scripts/android/phone.sh:*),Bash(sleep:*),Read,Write,Edit" \
  --disallowedTools "WebFetch,WebSearch,Task,NotebookEdit" \
  > results/agent-output.json 2> results/agent-errors.txt
CODE=$?
python3 scripts/agent_status.py results/agent-output.json results/agent-errors.txt "$PERSONA" || true
if [ -n "$REC" ]; then
  touch /tmp/stop-rec; adb shell pkill -INT screenrecord; sleep 4; kill "$REC" 2>/dev/null
  mkdir -p "$OUT/video"
  for f in $(adb shell ls /sdcard/ | tr -d '\r' | grep '^rec_'); do adb pull "/sdcard/$f" "$OUT/video/" >/dev/null 2>&1; done
fi
# Crashes the agent may have missed, for the report
adb logcat -d -b crash > "$OUT/crash-log.txt" 2>/dev/null || true
echo "Agent finished (exit $CODE), $(ls "$OUT" | wc -l | tr -d ' ') files"
