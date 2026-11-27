#!/bin/bash
# CPU and energy of encoding in real time, as during a meeting: runs bench.swift
# with --realtime (75 s) for a setting and samples, every 2 s, the bench and
# macOS's video encoder service with `top` (CPU %, energy impact "POWER").
# Frame drawing (the stand-in for ScreenCaptureKit) is in the bench's numbers.
#
#   scripts/screen-eval/energy.sh <name> <codec> <height> <fps> <quality>
set -euo pipefail
EVALS="${SOKKI_EVALS:-$HOME/sokki-evals}"
OUT="$EVALS/runs/screen-capture/$(date +%Y-%m-%d)"
NAME="$1"; shift
"$OUT/bench" "$EVALS/screen-capture-made-up" "$OUT/rt" "rt-$NAME" "$@" --realtime > "$OUT/rt-$NAME.json" &
PID=$!
SAMPLES="$OUT/rt-$NAME.top"
: > "$SAMPLES"
while kill -0 "$PID" 2>/dev/null; do
  top -l 2 -s 2 -stats pid,command,cpu,power -o power -n 40 2>/dev/null \
    | awk -v pid="$PID" 'BEGIN{s=0} /^PID/{s++; next} s==2 && ($1==pid || $2 ~ /VTEncoder|VTDecoder|replayd/) {print}' >> "$SAMPLES"
done
wait "$PID" || true
awk -v pid="$PID" '{ if ($1==pid) {n++; c+=$3; p+=$4} else {m[$2]+=$4; mc[$2]+=$3; k[$2]++} }
  END { printf "{\"name\":\"%s\",\"bench_cpu\":%.1f,\"bench_power\":%.1f", "'"$NAME"'", c/n, p/n;
        for (x in m) printf ",\"%s_cpu\":%.1f,\"%s_power\":%.1f", x, mc[x]/n, x, m[x]/n; print "}" }' "$SAMPLES" \
  | tee -a "$OUT/energy.jsonl"
cat "$OUT/rt-$NAME.json"
