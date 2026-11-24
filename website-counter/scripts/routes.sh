#!/usr/bin/env bash
# Routes on sokki.org without a Worker for the asset files, so only pages
# count against the free plan's 100,000 Worker runs a day, and "fail open" on
# every route: past the limit the site keeps working, only counting pauses.
# Run after `wrangler deploy` (it uses wrangler's login).
set -euo pipefail
ZONE=d1f9bd2eedf4798a7e1f5d979810a5f4   # sokki.org
ASSETS=(style.css app.js v.js favicon.svg og.png CNAME 404.html)
npx wrangler whoami >/dev/null
TOKEN=$(grep '^oauth_token' ~/Library/Preferences/.wrangler/config/default.toml | cut -d'"' -f2)
api() { curl -fsS "https://api.cloudflare.com/client/v4/zones/$ZONE/workers/routes$1" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' "${@:2}"; }

routes=$(api "")
for a in "${ASSETS[@]}"; do
  pattern="sokki.org/$a*"
  if ! grep -qF "\"pattern\":\"$pattern\"" <<<"$routes"; then
    api "" -X POST -d "{\"pattern\":\"$pattern\",\"request_limit_fail_open\":true}" >/dev/null
    echo "added $pattern (no Worker)"
  fi
done
# Fail open on the Worker's own route too.
python3 -c 'import json,sys
for r in json.load(sys.stdin)["result"]:
    if r.get("script"): print(r["id"], r["pattern"], r["script"])' <<<"$(api "")" |
while read -r id pattern script; do
  api "/$id" -X PUT -d "{\"pattern\":\"$pattern\",\"script\":\"$script\",\"request_limit_fail_open\":true}" >/dev/null
  echo "fail open: $pattern -> $script"
done
api "" | python3 -c 'import json,sys
for r in json.load(sys.stdin)["result"]: print(r["pattern"], "->", r.get("script") or "(none)", "fail_open" if r.get("request_limit_fail_open") else "")'
