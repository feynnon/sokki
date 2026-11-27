#!/bin/bash
# exits 0 once no meeting is recording or processing (checks every 20 s, gives up after $1 s)
end=$(( $(date +%s) + ${1:-1} ))
while :; do
  busy=$(python3 -c "import json;d=json.load(open('$HOME/Library/Application Support/org.sokki.app/meetings/index.json'));print(sum((m.get('status') or {}).get('kind') in ('recording','processing') for m in d))" 2>/dev/null || echo 1)
  [ "$busy" = "0" ] && { echo "free $(date +%T)"; exit 0; }
  [ $(date +%s) -ge $end ] && { echo "still busy, gave up $(date +%T)"; exit 3; }
  [ ${1:-5400} -lt 20 ] && sleep 1 || sleep 20
done
