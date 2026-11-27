#!/usr/bin/env zsh
# Night test: fast typing while autocorrect swaps a word, in Chrome (Robin, 27 Nov 2026:
# "I type, it corrects, but I've already typed the next letter, and then it goes wrong").
# The lab app (scripts/autocorrect-lab.sh) runs Sokki's keyboard hook and autocorrect, and
# types a made-up sentence with two typos as real key events at 12 and 15 keys a second,
# 10 times each, once with the hook holding keys during a swap (the fix) and once without
# (Sokki before). Every run must end with each typo fixed or as typed, nothing else.
# Types into the front window: run ONLY when nobody uses the Mac (03:00), never during a
# meeting. Needs the lab's Accessibility grant. Background:
# docs/research/autocorrect-typing-race-2026-11.md.
set -euo pipefail
root=${0:A:h:h:h}
out=${OUT_DIR:-$HOME/sokki-night/autocorrect-race-$(date +%Y%m%d-%H%M)}
index="$HOME/Library/Application Support/org.sokki.app/meetings/index.json"
mkdir -p "$out"

if grep -q '"kind": *"\(recording\|processing\)"' "$index" 2>/dev/null; then
  echo "FAIL: a meeting records or processes; try later"; exit 1
fi
idle=$(( $(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ {print $NF; exit}') / 1000000000 ))
if [ "$idle" -lt 600 ] && [ -z "${FORCE:-}" ]; then
  echo "FAIL: the Mac was used ${idle}s ago; this test types into the front window"; exit 1
fi

zsh "$root/scripts/autocorrect-lab.sh" build >/dev/null
open -g -a "Google Chrome" "file://$root/crates/sokki-autocorrect/examples/lab/paste-edit.html"
sleep 3
pid=$(pgrep -x "Google Chrome" | head -1)
[ -n "$pid" ] || { echo "FAIL: Chrome is not running"; exit 1; }

fails=0
for mode in nohold hold; do
  for speed in 12 15; do
    f="$out/$mode-$speed.txt"
    zsh "$root/scripts/autocorrect-lab.sh" race "$pid" com.google.Chrome "$f" "$mode" "$speed" 10
    grep -q "trusted: true" "$f" || { echo "FAIL: the lab has no Accessibility grant"; cat "$f"; exit 1; }
    line=$(grep "^result:" "$f" || echo "result: none (see $f)")
    echo "$line"
    if [ "$mode" = hold ] && ! echo "$line" | grep -q " 0 wrong"; then fails=$((fails + 1)); fi
  done
done
echo "details: $out"
[ "$fails" -eq 0 ] && echo "PASS" || { echo "FAIL: runs ended wrong with the fix"; exit 1; }
