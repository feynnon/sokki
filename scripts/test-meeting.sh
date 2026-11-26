#!/bin/bash
# A simulated call without a person, in the test app ("Sokki Test",
# org.sokki.test) next to Robin's Sokki, which it never touches.
# Test mode (src-tauri/src/test_mic.rs): the microphone plays one WAV, the
# system audio another, both in real time; the app records one meeting,
# stops it, processes it and logs how long that took after the stop. No
# recording pill, no notification, no voice data, no language model (the
# note goes to a temporary folder). CPU and memory of the app are sampled
# every 5 s while it runs.
#
#   scripts/test-dictation.sh --build                 # build the test app first
#   scripts/test-meeting.sh mic.wav system.wav [hold] # 16 kHz mono WAVs; hold in s
#   NO_LIVE=1 scripts/test-meeting.sh ...             # without the live transcript (A/B)
#   SYSTEM_DELAY=12 scripts/test-meeting.sh ...        # system audio starts 12 s late
#   DEAD_AT=20 scripts/test-meeting.sh ...             # the system audio tap goes dead at 20 s
#                                                      # (exact zeros, as on 2026-11-24) until
#                                                      # the watchdog rebuilds it
#   OUTPUT_CHANGE_AT=45 scripts/test-meeting.sh ...    # the default output changes at 45 s
#   SECOND_S=30 GAP_S=2 scripts/test-meeting.sh ...   # a second meeting while the first is processed
#   NO_YIELD=1 SECOND_S=30 scripts/test-meeting.sh ... # same, processing not yielding (A/B)
#   TITLE_AFTER=60 scripts/test-meeting.sh ...        # provisional title after 60 s, with the
#                                                     # language model and the summary on
#   TESTAPP=/path/Sokki\ Test.app scripts/test-meeting.sh ... # another build of the test app
#   SLACK=auth.json scripts/test-meeting.sh ...        # pretend the call is a Slack huddle and
#                                                      # post the notes into its thread, signed
#                                                      # in with that sokki-slack Auth file (make
#                                                      # one with `cargo run --example slack --
#                                                      # login`). Start a huddle in a TEST channel
#                                                      # first and stay in it until the post; add
#                                                      # TITLE_AFTER for a summary to post.
#                                                      # SLACK_THREAD=C…/ts: no huddle needed,
#                                                      # post into that existing test thread.
#   CHAT=zoom scripts/test-meeting.sh ...              # post the chat notice into the running
#                                                      # Zoom (or teams, chrome, arc, edge, brave)
#                                                      # call and watch it for "stop"; your own
#                                                      # test meeting only. Needs Accessibility
#                                                      # for Sokki Test. Add OWN_TEST_CALL=1 when
#                                                      # Robin's Sokki records that test call.
#   CALENDAR=events.json scripts/test-meeting.sh ...  # a made-up calendar (SOKKI_TEST_CALENDAR)
#   KEEP_MEETINGS=1 scripts/test-meeting.sh ...       # keep the test app's earlier meetings
#                                                     # (voices across meetings)
#   CALL_SPEAKERS=spans.json scripts/test-meeting.sh ... # who spoke when, as Meet's captions
#                                                     # would show it (speaker-names), for a
#                                                     # made-up call in Chrome
#   NAME_SPEAKER="Speaker 2=Lena Vogt" CONSENT=1 scripts/test-meeting.sh ...
#                                                     # name a speaker after processing, as a
#                                                     # click would (speaker-names)
#   MAIL_TO=me@example.com scripts/test-meeting.sh ... # notes to everyone (meeting/mail.rs): the
#                                                      # summary on, a made-up calendar event
#                                                      # around the call (CALENDAR=events.json for
#                                                      # your own), the automatic send on, and every
#                                                      # mail to MAIL_TO only. Your own address only;
#                                                      # needs an account in Apple Mail and asks once
#                                                      # whether Sokki Test may control Mail.
#
# Real recordings stay outside the repository: convert them to WAV in a
# temporary folder (ffmpeg -i mic.flac -ar 16000 -ac 1 mic.wav).
#
# Safety, after the 2026-11-19 input freeze: the app is started only with
# `open`, never its binary from a shell; nothing here runs tccutil or touches
# Robin's Sokki, its settings or its data.
set -euo pipefail

# TEST_ID/TEST_NAME: a second test app, so two sessions can test side by side.
ID="${TEST_ID:-org.sokki.test}"
NAME="${TEST_NAME:-Sokki Test}"
TESTAPP="${TESTAPP:-$HOME/Applications/$NAME.app}"
DATA="$HOME/Library/Application Support/$ID"
LOG="$HOME/Library/Logs/$ID/sokki.log"
# Robin's settings: under the installed Sokki's bundle id.
REAL_ID="$(defaults read /Applications/Sokki.app/Contents/Info.plist CFBundleIdentifier 2>/dev/null || echo org.sokki.app)"
SOKKI_SETTINGS="${SOKKI_SETTINGS:-$HOME/Library/Application Support/$REAL_ID/settings_store.json}"
# A copy of the test app may carry its own bundle id (a separate
# Accessibility entry); its data and log still live under $ID.
APP_ID="$(defaults read "$TESTAPP/Contents/Info" CFBundleIdentifier 2>/dev/null || echo "$ID")"
quit_test_app() {
  osascript -e "tell application id \"$APP_ID\" to quit" 2>/dev/null || true
  for _ in $(seq 1 10); do
    pgrep -f "^$TESTAPP/Contents/MacOS/" >/dev/null || return 0
    sleep 0.5
  done
  pkill -TERM -f "^$TESTAPP/Contents/MacOS/" || true
}

[[ -d "$TESTAPP" ]] || { echo "No test app; run: scripts/test-dictation.sh --build" >&2; exit 1; }
[[ $# -ge 2 ]] || { echo "usage: $0 mic.wav system.wav [hold_s]" >&2; exit 1; }
abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
MIC="$(abs "$1")"
SYSTEM="$(abs "$2")"
LENGTH=$(afinfo "$MIC" | awk '/estimated duration/ {print $3}')
HOLD="${3:-$(echo "$LENGTH + 1" | bc)}"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DATA"
# Robin's settings, minus everything that reaches outside the test app.
jq --arg notes "$WORK/notes" --arg chat "${CHAT:-}" --arg mail "${MAIL_TO:-}" --arg slack "${SLACK:-}" '.settings.onboarding_completed = true
  | .settings.start_hidden = true
  | .settings.autostart_enabled = false
  | .settings.update_checks_enabled = false
  | .settings.show_tray_icon = false
  | .settings.meetings.detection_enabled = false
  | .settings.meetings.phone_link_enabled = false
  | .settings.meetings.correction_enabled = false
  | .settings.meetings.summary_enabled = ($title != "" or $mail != "")
  | .settings.meetings.mail_notes_auto = ($mail != "")
  | .settings.meetings.chat_notice_enabled = ($chat != "")
  | .settings.meetings.slack_post_huddles = ($slack != "")
  | .settings.meetings.notes_dir = $notes' --arg title "${TITLE_AFTER:-}" "$SOKKI_SETTINGS" > "$DATA/settings_store.json"
rm -f "$DATA/slack_auth.json"
if [[ -n "${SLACK:-}" ]]; then
  install -m 600 "$SLACK" "$DATA/slack_auth.json"
  # Releases leave Slack out; the test app gets the client id at run time.
  SLACK_ID="$(sed -n 's/^Client ID: `\(.*\)`$/\1/p' "$(dirname "$0")/../crates/sokki-slack/SLACK_APP.md")"
fi
if [[ -n "${TITLE_AFTER:-}" || -n "${MAIL_TO:-}" ]]; then
  # Sokki's downloaded language model (Gemma 4 E4B and its draft model),
  # linked, not copied.
  REAL_LLM="$HOME/Library/Application Support/$REAL_ID/meeting-models/llm"
  for f in gemma-4-E4B-it-Q4_K_M.gguf mtp-gemma-4-E4B-it.gguf; do
    [[ -f "$REAL_LLM/$f" ]] || { echo "Sokki has not downloaded the language model yet." >&2; exit 1; }
    mkdir -p "$DATA/meeting-models/llm"
    [[ -f "$DATA/meeting-models/llm/$f" ]] || ln "$REAL_LLM/$f" "$DATA/meeting-models/llm/"
  done
fi

if strings "$TESTAPP/Contents/MacOS/sokki" | grep -q /usr/bin/tccutil; then
  echo "The test app still contains tccutil; rebuild it from current meetings." >&2
  exit 1
fi
if codesign -dv "$TESTAPP" 2>&1 | grep -q "Signature=adhoc" \
  && security find-identity -p codesigning 2>/dev/null | grep -q '"Sokki Dev"'; then
  echo "The test app is signed ad hoc; rebuild it: scripts/test-dictation.sh --build" >&2
  exit 1
fi

# Never while one of Robin's real meetings records or processes.
# OWN_TEST_CALL=1: Robin's Sokki is only recording your own test call (it
# detects the call too); Robin allowed testing then (2026-11-24).
REAL_INDEX="$HOME/Library/Application Support/$REAL_ID/meetings/index.json"
if [[ -z "${OWN_TEST_CALL:-}" && -f "$REAL_INDEX" ]] && jq -e '[(if type == "array" then . else (.meetings // []) end)[]
    | select(.status.kind == "recording" or .status.kind == "processing")] | length > 0' \
    "$REAL_INDEX" >/dev/null 2>&1; then
  echo "A meeting of Robin's is recording or processing; try again later." >&2
  exit 1
fi

# A made-up calendar event around the test call, unless one was given.
if [[ -n "${MAIL_TO:-}" && -z "${CALENDAR:-}" ]]; then
  CALENDAR="$WORK/events.json"
  NOW=$(date +%s)
  jq -n --argjson s "$((NOW - 60))" --argjson e "$((NOW + ${HOLD%.*} + 600))" '[{
    title: "Sokki test call", start: $s, end: $e, all_day: false,
    url: "https://meet.google.com/abc-defg-hij",
    organizer: {name: "Test Organizer", email: "mailto:organizer@example.com", is_me: false, status: "accepted", kind: "person"},
    attendees: [
      {name: "Anna Test", email: "mailto:anna@example.com", is_me: false, status: "accepted", kind: "person"},
      {name: "Declined Dora", email: "mailto:dora@example.com", is_me: false, status: "declined", kind: "person"},
      {name: "Room 1", email: "mailto:room@resource.calendar.google.com", is_me: false, status: "accepted", kind: "room"}
    ]}]' > "$CALENDAR"
fi

quit_test_app
sleep 1
# Only the test app's own meetings: a run cut short by quitting would
# otherwise be processed at this launch, next to the new one.
[[ -n "${KEEP_MEETINGS:-}" ]] || rm -rf "$DATA/meetings"
# Follow the log across rotation (a long meeting fills it several times).
EVENTS="$WORK/events.txt"
PATTERN="Silence hint|Meeting microphone|Slack|No speech|no speech|notes mailed|notes not mailed|Mail:|System audio|Call track|Meeting levels|Chat notice|meeting-chat|stopped and deleted|Test mode|Live transcript|live transcript|Live speakers|Live microphone|Live system|Meeting step|Aligned tracks|recording .* ms after|Diarized|summary later|Meeting title|dates follow|anguage model|ERROR|WARN"
# One process, so the trap ends it and nothing outlives the script.
tail -F -n0 "$LOG" > "$WORK/log.txt" 2>/dev/null &
TAILPID=$!
trap '{ kill $TAILPID; wait $TAILPID; } 2>/dev/null; rm -rf "$WORK"' EXIT
sleep 1
echo "Recording a ${HOLD}s meeting (live transcript ${NO_LIVE:+off}${NO_LIVE:-on})…"
open -g --env SOKKI_TEST_MIC="$MIC" --env SOKKI_TEST_SYSTEM="$SYSTEM" \
  --env SOKKI_TEST_MEETING=1 --env SOKKI_TEST_HOLD_S="$HOLD" \
  ${NO_LIVE:+--env SOKKI_TEST_NO_LIVE=1} ${SYSTEM_DELAY:+--env SOKKI_TEST_SYSTEM_DELAY_S=$SYSTEM_DELAY} \
  ${DEAD_AT:+--env SOKKI_TEST_SYSTEM_DEAD_AT_S=$DEAD_AT} ${OUTPUT_CHANGE_AT:+--env SOKKI_TEST_OUTPUT_CHANGE_AT_S=$OUTPUT_CHANGE_AT} \
  ${SECOND_S:+--env SOKKI_TEST_SECOND_S="$SECOND_S"} ${GAP_S:+--env SOKKI_TEST_GAP_S="$GAP_S"} \
  ${NO_YIELD:+--env SOKKI_TEST_NO_YIELD=1} ${TITLE_AFTER:+--env SOKKI_TEST_TITLE_AFTER_S="$TITLE_AFTER"} \
  ${CHAT:+--env SOKKI_TEST_CALL="$CHAT"} ${SLACK:+--env SOKKI_TEST_CALL=slack --env SOKKI_SLACK_CLIENT_ID="$SLACK_ID"} ${SLACK_THREAD:+--env SOKKI_TEST_SLACK_THREAD="$SLACK_THREAD"} \
  ${MAIL_TO:+--env SOKKI_TEST_MAIL_TO="$MAIL_TO"} ${CALENDAR:+--env SOKKI_TEST_CALENDAR="$CALENDAR"} \
  ${CALL_SPEAKERS:+--env SOKKI_TEST_CALL_SPEAKERS="$CALL_SPEAKERS"} ${CALL_SPEAKERS:+${CHAT:---env SOKKI_TEST_CALL=chrome}} \
  ${NAME_SPEAKER:+--env SOKKI_TEST_NAME_SPEAKER="$NAME_SPEAKER"} ${CONSENT:+--env SOKKI_TEST_CONSENT=1} -a "$TESTAPP"

# Sample the app (and its model workers) every 5 s until processing is done.
SAMPLES="$WORK/samples.txt"
STARTED=""
LIMIT=$(echo "($HOLD + ${SECOND_S:-0} + 900) / 5" | bc)
for _ in $(seq 1 "$LIMIT"); do
  sleep 5
  grep -E "$PATTERN" "$WORK/log.txt" | grep -v "Loaded settings" > "$EVENTS" || true
  [[ -n "$STARTED" ]] || ! grep -q "Test mode: meeting starts" "$EVENTS" || STARTED=$(date +%s)
  if [[ -n "${SLACK:-}" ]]; then
    grep -qE "Slack: meeting .*(notes posted|posting failed|no huddle seen)|Test mode: (the meeting failed|the meeting didn)" "$EVENTS" && break
  elif grep -qE "Test mode: (meeting processed|the meeting failed|the meeting didn|no speech)|stopped and deleted after a stop" "$EVENTS"; then
    # The notes mail goes out after processing; wait for it (2 min at most).
    [[ -z "${MAIL_TO:-}" ]] || grep -qE "notes mailed|notes not mailed|the meeting failed" "$EVENTS" \
      || (( $(date +%s) - ${MAILWAIT:=$(date +%s)} > 120 )) || continue
    break
  fi
  PIDS=$(pgrep -f "^$TESTAPP/Contents/MacOS/" | paste -sd, - || true)
  [[ -n "$PIDS" ]] || break
  # %cpu summed over the app's processes, resident memory in MB.
  { ps -o %cpu=,rss= -p "$PIDS" || true; } \
    | awk -v t="$(date +%s)" '{c+=$1; m+=$2} END {printf "%s %.0f %.0f\n", t, c, m/1024}' >> "$SAMPLES"
done
sleep 1
grep -E "$PATTERN" "$WORK/log.txt" | grep -v "Loaded settings" || true
if [[ -s "$SAMPLES" && -n "$STARTED" ]]; then
  awk -v stop="$(echo "$STARTED + $HOLD" | bc)" '{
      if ($1 < stop) { n++; c+=$2; if ($2>cmax) cmax=$2; if ($3>mmax) mmax=$3 }
      else { pn++; pc+=$2; if ($2>pcmax) pcmax=$2; if ($3>pmmax) pmmax=$3 } }
    END {
      if (n) printf "During the call: CPU mean %.0f%%, max %.0f%%; memory max %d MB (%d samples)\n", c/n, cmax, mmax, n;
      if (pn) printf "After the stop:  CPU mean %.0f%%, max %.0f%%; memory max %d MB (%d samples)\n", pc/pn, pcmax, pmmax, pn }' "$SAMPLES"
fi
quit_test_app
