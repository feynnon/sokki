# Voice-data counter

A tiny Cloudflare Worker that counts how much of their own voice opted-in
Verbatim installs keep, per language. It stores a random id per install and
seconds per language, nothing else, and serves the total at `GET /total`.
It runs on Cloudflare's free plan: installs report at most once an hour, and the total is recomputed at most every 10 minutes, so it stays far below the free limits (which return errors when exceeded, never a bill).

## Deploy

```bash
npm install -g wrangler
wrangler login
wrangler d1 create verbatim-counter          # paste the printed id into wrangler.toml
wrangler d1 execute verbatim-counter --remote --file schema.sql
wrangler deploy                               # prints https://verbatim-counter.<you>.workers.dev
```

Then put the URL into `counter/url.txt`; `scripts/build-and-install.sh` builds it in,
so installs report and show the total (or set `VERBATIM_COUNTER_URL` yourself):

```bash
echo https://verbatim-counter.<you>.workers.dev > counter/url.txt
scripts/build-and-install.sh
```

Without `VERBATIM_COUNTER_URL` the app keeps counting locally and shows no total.

## Message everyone who opted in

The "Your voice" page in Verbatim shows a notice from the counter, for
example when a buyer is interested. Set or clear it:

```bash
wrangler d1 execute verbatim-counter --remote --command \
  "INSERT INTO cache (key, value, updated) VALUES ('notice', 'A team that trains German speech models wants to buy 500 hours. Details: https://…', 0) ON CONFLICT(key) DO UPDATE SET value = excluded.value"
wrangler d1 execute verbatim-counter --remote --command "DELETE FROM cache WHERE key = 'notice'"
```

## Try it locally

```bash
wrangler d1 execute verbatim-counter --local --file schema.sql
wrangler dev
curl -s localhost:8787/report -H 'Content-Type: application/json' \
  -d '{"id":"0123456789abcdef0123456789abcdef","clips":3,"seconds":{"de":120,"en":30}}'
curl -s localhost:8787/total
```
