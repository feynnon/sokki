# Sokki counter

A tiny Cloudflare Worker behind Sokki's **Your data** page:

- Installs with **Help improve Sokki** or **Share all my recordings** on
  report a random id and how many seconds of their own data they keep per
  language (`POST /report`); anyone can read the total (`GET /total`), with the
  number of installs rounded down to 1000, 10000, … (0 below 1000).
  Turning both off removes the install from the count (`POST /leave`).
- With **Help improve** on, an install sends one report a day
  (`POST /usage`): usage counts, problems from the log (quoted words and
  paths blanked), the Mac's model, chip, memory, disk, macOS version and
  region. Never names, audio or anyone's words. One row per install and day.
- "I'm interested" on a buyer message sends the id, the message's id, the
  language and the matching hours (`POST /interest`).
- With **Share all my recordings** on, an install uploads every recording of
  the user's (`POST /donate`): FLAC audio into
  Workers KV (`<id>/<clip>.flac`), the transcript and length into D1
  (`donations`), unchanged, names and numbers included. The store stops taking uploads at 900 MB (KV's free
  1 GB; beyond it Cloudflare returns errors, never a bill).
- **Tell me about paid recording tasks** sends the id and the languages of the
  user's own recordings (`POST /tasks`); unticking removes it. See who ticked it:
  `wrangler d1 execute verbatim-counter --remote --command "SELECT languages, count(*) FROM tasks GROUP BY languages"`.
- **Page views on sokki.org** (`GET /visit?p=/get/&s=note`): per day, page and
  the link's tag (`?s=`), so the launch shows which link brings people.
  Nothing about the visitor. See them:
  `wrangler d1 execute verbatim-counter --remote --command "SELECT source, page, sum(views) FROM visits GROUP BY source, page ORDER BY 3 DESC"`.
- **Delete what was shared** removes every row of the id and its audio
  (`POST /forget`).

### Against fake data

An open-source app can't hold a secret, so anyone can talk to this service.
What keeps fakes expensive and harmless:

- **Proof of work per id.** `sha256("sokki:<id>:<pow>")` must start with 24
  zero bits: about a second for the app, once per id, so inventing thousands
  of installs takes hours. Ids from before this check are accepted without it.
- **Rate limits per IP address** (Cloudflare's rate limiting binding, counted
  in memory, never stored): 20 writes and 120 uploads a minute. It's
  approximate: bursts get cut off, a slow trickle doesn't.
- **Plausibility.** Hours can't grow faster than time passes; at most 2,000
  hours per install; daily reports up to 32 KB.
- **Uploads** only from installs whose count says "share", FLAC only, clip
  names in the app's format, 5 MB each, 900 MB in all.
- At a sale, the buyer gets real audio, which can be checked; fake counts can
  only inflate the public total.

No IP addresses are stored.
It runs on Cloudflare's free plan: installs report at most once an hour, and the total is recomputed at most every 10 minutes, so it stays far below the free limits (which return errors when exceeded, never a bill).

## Deploy

```bash
npm install -g wrangler
wrangler login
wrangler d1 create verbatim-counter          # paste the printed id into wrangler.toml
wrangler d1 execute verbatim-counter --remote --file schema.sql
wrangler kv namespace create sokki-donations  # paste the id into wrangler.toml (AUDIO)
wrangler deploy                               # prints https://verbatim-counter.<you>.workers.dev
```

Then put the URL into `counter/url.txt`; `scripts/build-and-install.sh` builds it in,
so installs report and show the total (or set `SOKKI_COUNTER_URL` yourself):

```bash
echo https://verbatim-counter.<you>.workers.dev > counter/url.txt
scripts/build-and-install.sh
```

Without `SOKKI_COUNTER_URL` the app keeps counting locally and shows no total.

## Send a message

Every install downloads `GET /total` a minute after start and every 6
hours; it includes the current message, if any. A new message (its id is the
time it was set) is handled once.

A **buyer message** is JSON with a language. Each install compares it with
the hours of its own data in that language and, if at least an hour fits, the user
turned on **Ask me when there's a buyer** and doesn't share all recordings,
sends a notification (text in `buyer_message` in `voice_data.rs`) and shows a card on the Your data page with
"I'm interested" and "Not interested". `text` is optional and shown on the
card:

```bash
wrangler d1 execute verbatim-counter --remote --command \
  "INSERT INTO cache (key, value, updated) VALUES ('notice', '{\"language\": \"de\", \"text\": \"A team building German speech recognition wants 500 hours.\"}', unixepoch()) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated = excluded.updated"
```

An **important message** is JSON with `"kind": "message"`. The app shows it
as a window the user answers (never while a meeting records, never on the
first start); "Later" brings it back after a day, the action or "OK" ends it,
and after `until` (last day, local date) it never shows. `de` is optional
(other languages read `en`); `action` is optional (without it: just "OK") and
either opens `url` or, with `"update": true`, installs the waiting update:

```bash
wrangler d1 execute verbatim-counter --remote --command \
  "INSERT INTO cache (key, value, updated) VALUES ('notice', '{\"kind\": \"message\", \"title\": {\"de\": \"Bitte aktualisiere Sokki.\", \"en\": \"Please update Sokki.\"}, \"text\": {\"de\": \"…\", \"en\": \"…\"}, \"action\": {\"label\": {\"de\": \"Jetzt aktualisieren\", \"en\": \"Update now\"}, \"update\": true}, \"until\": \"2026-12-31\"}', unixepoch()) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated = excluded.updated"
```

Plain text is no longer shown (Robin, 2026-11-23: only important messages,
as a window). Clear the message:

```bash
wrangler d1 execute verbatim-counter --remote --command "DELETE FROM cache WHERE key = 'notice'"
```

See who answered, and the daily reports:

```bash
wrangler d1 execute verbatim-counter --remote --command "SELECT language, count(*), sum(hours) FROM interest GROUP BY language"
wrangler d1 execute verbatim-counter --remote --command "SELECT day, count(*) FROM usage GROUP BY day ORDER BY day DESC LIMIT 14"
```

## Try it locally

```bash
wrangler d1 execute verbatim-counter --local --file schema.sql
wrangler dev
curl -s localhost:8787/report -H 'Content-Type: application/json' \
  -d '{"id":"0123456789abcdef0123456789abcdef","clips":3,"seconds":{"de":120,"en":30}}'
curl -s localhost:8787/total
```
