#!/usr/bin/env bash
# Night test: the meeting shortcut (Option+M) against real key events in the
# installed Sokki (0.2.31 or later). Checks that a deliberate Option+M
# starts and stops a meeting, and that typing "mö" fast does not.
# Posts keys into the front window: run ONLY when nobody uses the Mac,
# and never while a meeting records or processes.
# Background: docs/research/meeting-shortcut-typing-2026-11.md
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
log="$HOME/Library/Logs/org.sokki.app/sokki.log"
index="$HOME/Library/Application Support/org.sokki.app/meetings/index.json"
bin="${TMPDIR:-/tmp}/sokki-post-keys"

fail() { echo "FAIL: $*"; exit 1; }

if grep -q '"kind": *"\(recording\|processing\)"' "$index" 2>/dev/null; then
  fail "a meeting records or processes; try later"
fi
pgrep -f "/Sokki.app/Contents/MacOS/" >/dev/null || fail "Sokki is not running (start it with: open -g -a Sokki)"
swiftc -O -o "$bin" "$here/post_keys.swift"

# A blank TextEdit document takes the typed letters, so nothing lands in
# another app. The sending process needs Accessibility to post events.
osascript -e 'tell application "TextEdit" to make new document' -e 'tell application "TextEdit" to activate' >/dev/null
sleep 1

count() { grep -c "$1" "$log" || true; }
starts_before=$(count "Meeting start via the meeting shortcut")
ignored_before=$(count "Shortcut meeting (option+m) not used")

# 1. Typing "mö": no meeting, one "not used" line.
"$bin" typing
sleep 2
[ "$(count "Meeting start via the meeting shortcut")" = "$starts_before" ] \
  || fail "typing \"mö\" started a meeting"
[ "$(count "Shortcut meeting (option+m) not used")" -gt "$ignored_before" ] \
  || fail "typing \"mö\" logged no 'not used' line (were the keys seen?)"
echo "ok: typing \"mö\" did not start a meeting"

# 2. Deliberate Option+M: starts a meeting, the second one stops it.
"$bin" deliberate
sleep 2
[ "$(count "Meeting start via the meeting shortcut")" -gt "$starts_before" ] \
  || fail "deliberate Option+M did not start a meeting"
echo "ok: deliberate Option+M started a meeting"
sleep 5
"$bin" deliberate
sleep 3
if grep -q '"kind": *"recording"' "$index"; then
  sokki meeting stop >/dev/null 2>&1 || true
  fail "the second Option+M did not stop the meeting (stopped via CLI)"
fi
echo "ok: the second Option+M stopped it (a silent test meeting goes to the Trash by itself)"

osascript -e 'tell application "TextEdit" to close front document saving no' >/dev/null || true
echo "PASS"
