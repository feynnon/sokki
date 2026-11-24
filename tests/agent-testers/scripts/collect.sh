#!/usr/bin/env bash
# Run on YOUR machine: downloads every encrypted result of a run, decrypts it
# and writes SUMMARY.md. Needs the private key that matches results_public.pem.
# Usage: bash scripts/collect.sh <run id> [private key] [out dir]
# Env: REPO (default feynnon/sokki), SUMMARY_GROUPING=0 to skip grouping
#   similar reports with your local `claude` (only titles are sent).
# Based on scripts/collect.sh of pierridotite/mobile-agent-testers at d610b95a (MIT).
set -euo pipefail
RUN=$1
REPO=${REPO:-feynnon/sokki}
PRIV=${2:-$HOME/.config/sokki-agent-testers/results.pem}; OUT=${3:-runs/$1}
mkdir -p "$OUT/enc"
gh run download "$RUN" -R "$REPO" -D "$OUT/enc" --pattern 'results-*'
for key in "$OUT"/enc/*/*.key.bin; do
  name=$(basename "$key" .key.bin); dir=$(dirname "$key")
  pass=$(openssl pkeyutl -decrypt -inkey "$PRIV" -pkeyopt rsa_padding_mode:oaep -in "$key")
  mkdir -p "$OUT/$name"
  openssl enc -d -aes-256-cbc -pbkdf2 -in "$dir/$name.tgz.enc" -pass "pass:$pass" | tar xz -C "$OUT/$name"
  python3 "$(dirname "$0")/agent_status.py" "$OUT/$name/results/agent-output.json" "$OUT/$name/results/agent-errors.txt" "$name" || true
done
python3 "$(dirname "$0")/summarize.py" "$OUT" > "$OUT/SUMMARY.md" && echo "Summary: $OUT/SUMMARY.md"
