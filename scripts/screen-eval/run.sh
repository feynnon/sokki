#!/bin/bash
# Runs the screen-recording compression bench (bench.swift) over a set of
# settings and writes one JSON line per setting to
# $SOKKI_EVALS/runs/screen-capture/<date>/results.jsonl. Heavy-ish (hardware
# encoder, OCR): run it through ~/.local/bin/build-slot, never while a
# meeting records. Write-up: docs/research/screen-recording-size-2026-11.md.
#
#   python3 scripts/screen-eval/make_screens.py   # once: the made-up screens
#   ~/.local/bin/build-slot run --name screen-eval -- scripts/screen-eval/run.sh
set -euo pipefail
cd "$(dirname "$0")"
EVALS="${SOKKI_EVALS:-$HOME/sokki-evals}"
SCREENS="$EVALS/screen-capture-made-up"
OUT="$EVALS/runs/screen-capture/$(date +%Y-%m-%d)"
mkdir -p "$OUT"
BENCH="$OUT/bench"
swiftc -O bench.swift -o "$BENCH"
swiftc -O export.swift -o "$OUT/export"
RESULTS="$OUT/results.jsonl"
: > "$RESULTS"

run() { "$BENCH" "$SCREENS" "$OUT" "$@" | tee -a "$RESULTS"; }

# name codec height fps quality [bitrate]
run current        hevc 0    30 0     # what Sokki does today
run native-q50     hevc 0    2  0.5
run h1117-q50      hevc 1116 2  0.5   # 1x logical (1728x1117)
for q in 0.3 0.4 0.5 0.6 0.7; do run "h1080-q${q#0.}" hevc 1080 2 "$q"; done
for q in 0.4 0.5 0.6; do run "h900-q${q#0.}" hevc 900 2 "$q"; done
for q in 0.5 0.6 0.7; do run "h720-q${q#0.}" hevc 720 2 "$q"; done
run h1080-q50-1fps hevc 1080 1  0.5
run h1080-q50-5fps hevc 1080 5  0.5
run h1080-h264-q50 h264 1080 2  0.5
run h1080-300k     hevc 1080 2  0 300000
run h1080-600k     hevc 1080 2  0 600000

# What the render does today: the screen video re-encoded at "highest quality".
"$OUT/export" "$OUT/current.mov" "$OUT/current-export.mp4" | tee -a "$RESULTS"
