# Voice-data counter

A tiny Cloudflare Worker that counts how much of their own voice opted-in
Verbatim installs keep, per language. It stores a random id per install and
seconds per language, nothing else, and serves the total at `GET /total`.
It runs on Cloudflare's free plan.

## Deploy

```bash
npm install -g wrangler
wrangler login
wrangler d1 create verbatim-counter          # paste the printed id into wrangler.toml
wrangler d1 execute verbatim-counter --remote --file schema.sql
wrangler deploy                               # prints https://verbatim-counter.<you>.workers.dev
```

Then build Verbatim with the URL, so installs can report and show the total:

```bash
VERBATIM_COUNTER_URL=https://verbatim-counter.<you>.workers.dev scripts/build-and-install.sh
```

Without `VERBATIM_COUNTER_URL` the app keeps counting locally and shows no total.

## Try it locally

```bash
wrangler d1 execute verbatim-counter --local --file schema.sql
wrangler dev
curl -s localhost:8787/report -H 'Content-Type: application/json' \
  -d '{"id":"0123456789abcdef0123456789abcdef","clips":3,"seconds":{"de":120,"en":30}}'
curl -s localhost:8787/total
```
