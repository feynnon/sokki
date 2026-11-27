#!/bin/bash
# run.sh MEETINGS.tsv RUNS : Sokki's llama-server with Sokki's exact flags, capture A via meeting-core, then bench.py
set -u
B=$(cd "$(dirname "$0")" && pwd); R="${SOKKI_EVALS:-$HOME/sokki-evals}/runs/summary-citations"; mkdir -p "$R/capture" "$R/out" "$R/logs"
SRV=/Applications/Sokki.app/Contents/Resources/resources/llm/llama-server
LLM="$HOME/Library/Application Support/org.sokki.app/meeting-models/llm"
SET="$HOME/Library/Application Support/org.sokki.app/settings_store.json"
PORT=18131; PX=18132; export PORT
"$B/meetfree.sh" || { echo "meeting busy"; exit 3; }
"$SRV" --model "$LLM/gemma-4-E4B-it-Q4_K_M.gguf" --host 127.0.0.1 --port $PORT --ctx-size 16384 --parallel 1 --jinja --reasoning off \
  --temp 1.0 --top-p 0.95 --top-k 64 --min-p 0 --repeat-penalty 1.0 --cache-ram 0 --no-ui \
  --spec-type draft-mtp --model-draft "$LLM/mtp-gemma-4-E4B-it.gguf" </dev/null >"$R/logs/server.log" 2>&1 &
SP=$!
python3 "$B/proxy.py" http://127.0.0.1:$PORT $PX "$R/capture" & PP=$!
trap 'kill $SP $PP $MP $GP 2>/dev/null' EXIT
( pk=0; while kill -0 $SP 2>/dev/null; do r=$(ps -o rss= -p $SP | tr -d ' '); [ -n "$r" ] && [ "$r" -gt "$pk" ] && pk=$r && echo $pk > "$R/logs/peak_rss_kb"; [ -n "$r" ] && [ "$r" -gt 6710886 ] && { echo "OVER 6.4 GB" >> "$R/logs/guard"; kill -9 $SP; }; sleep 1; done ) & MP=$!
( while kill -0 $SP 2>/dev/null; do "$B/meetfree.sh" >/dev/null || { echo "MEETING STARTED $(date +%T)" >> "$R/logs/guard"; kill -9 $SP; exit; }; sleep 5; done ) & GP=$!
t0=$(date +%s)
for i in $(seq 1 180); do curl -s -m 2 http://127.0.0.1:$PORT/health | grep -q ok && break; sleep 1; done
echo "load_s $(( $(date +%s) - t0 ))"
( cd "$B/sumbench" && cargo build -q --target-dir "$R/sumbench-target" ) || exit 1
while IFS=$'\t' read -r id note; do
  [ -s "$R/capture/$id.json" ] && continue
  echo "$id" > "$R/capture/.name"
  timeout 900 "$R/sumbench-target/debug/sumbench" "$note" "http://127.0.0.1:$PX/v1" "$R/capture/$id.sum.json" "$SET" || echo "capture failed $id"
done < "$1"
python3 "$B/bench.py" "$1" "$(cat "$R/runs.txt" 2>/dev/null || echo $2)"
echo "peak_rss_kb $(cat "$R/logs/peak_rss_kb")"
