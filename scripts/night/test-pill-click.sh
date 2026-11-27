#!/usr/bin/env bash
# Night test: a real click and a real drag on the meeting pill in the
# installed Sokki (0.2.31 or later). A click on the waveform must bring
# Sokki's window forward on the recording meeting's page, from a hidden
# window and, when the Mac has a second Space, from another Space; a 20 px
# drag must move the pill and open nothing.
# Posts mouse events into the front window: run ONLY when nobody uses the
# Mac, and never while a meeting records or processes. The process that
# runs it needs Accessibility to post events.
# Background: MEETINGS.md (recording pill), meeting/indicator.rs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
log="$HOME/Library/Logs/org.sokki.app/sokki.log"
index="$HOME/Library/Application Support/org.sokki.app/meetings/index.json"
bin="${TMPDIR:-/tmp}/sokki-post-mouse"
started=""

stop_meeting() {
  [ -n "$started" ] && sokki meeting stop >/dev/null 2>&1 || true
  started=""
}
fail() {
  echo "FAIL: $*"
  stop_meeting
  exit 1
}
trap stop_meeting EXIT

if grep -q '"kind": *"\(recording\|processing\)"' "$index" 2>/dev/null; then
  echo "FAIL: a meeting records or processes; try later"
  exit 1
fi
pgrep -f "/Sokki.app/Contents/MacOS/" >/dev/null || { echo "FAIL: Sokki is not running (start it with: open -g -a Sokki)"; exit 1; }
swiftc -O -o "$bin" "$here/post_mouse.swift"

count() { grep -c "$1" "$log" || true; }
opened() { count "Meeting page on screen .* after the pill click"; }

# A short test meeting of its own; a silent one goes to the Trash by itself.
sokki meeting record >/dev/null || fail "could not start a test meeting (sokki meeting record)"
started=1
for _ in $(seq 1 20); do
  "$bin" pill >/dev/null 2>&1 && break
  sleep 0.5
done
read -r px py pw ph < <("$bin" pill) || fail "no pill on screen after the start"
echo "ok: pill at $px,$py (${pw}x$ph)"
# The waveform: 16 px padding, then the bars; the middle of the pill's height.
wx=$((px + 26))
wy=$((py + 20))

# 1. Click from a hidden window: the meeting page, quickly.
osascript -e 'tell application "System Events" to set visible of process "Sokki" to false' >/dev/null 2>&1 || true
sleep 1
before=$(opened)
"$bin" click "$wx" "$wy"
sleep 2
[ "$(opened)" -gt "$before" ] || fail "click: no 'Meeting page on screen' line (was the click seen?)"
line=$(grep "Meeting page on screen .* after the pill click" "$log" | tail -1)
echo "$line" | grep -q "\[INFO\]" || fail "click: slow or wrong page: $line"
[ "$("$bin" main)" = "1" ] || fail "click: the log says shown, but Sokki's window is not on screen"
echo "ok: click opened the meeting page: ${line##*INFO\] }"

# 2. A 20 px drag: the pill moves, nothing opens.
before=$(opened)
"$bin" drag "$wx" "$wy" 20
sleep 1.5
read -r nx _ _ _ < <("$bin" pill) || fail "drag: the pill is gone"
moved=$((nx - px))
[ "$moved" -ge 15 ] && [ "$moved" -le 25 ] || fail "drag: the pill moved $moved px, not about 20"
[ "$(opened)" = "$before" ] || fail "drag: it opened the meeting page"
echo "ok: drag moved the pill ${moved} px and opened nothing"
wx=$((wx + moved))

# 3. Another Space, if the Mac has one (Control+Right must switch Spaces).
"$bin" space right
sleep 1.5
if [ "$("$bin" main)" = "1" ]; then
  echo "skip: no second Space (Sokki's window still on screen after Control+Right)"
else
  read -r sx sy _ _ < <("$bin" pill) || { "$bin" space left; fail "space: no pill on the other Space"; }
  before=$(opened)
  "$bin" click "$((sx + 26))" "$((sy + 20))"
  sleep 2
  ok_log=$([ "$(opened)" -gt "$before" ] && echo 1 || echo 0)
  ok_win=$("$bin" main)
  # Back to where the night run started, whatever Space the click left us on.
  [ "$ok_win" = "1" ] || "$bin" space left
  [ "$ok_log" = "1" ] || fail "space: no 'Meeting page on screen' line"
  [ "$ok_win" = "1" ] || fail "space: the page opened, but Sokki's window is not on screen"
  echo "ok: from another Space, the click brought Sokki's window to the screen"
fi

stop_meeting
sleep 2
grep -q '"kind": *"recording"' "$index" && fail "the test meeting did not stop"
echo "ok: the test meeting stopped"
echo "PASS"
