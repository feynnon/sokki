# Sokki counter

A tiny Cloudflare Worker behind Sokki's **Your data** page:

- Installs with **Help improve Sokki** or **Support Sokki & Open Source** on
  report a random id and how many seconds of their own data they keep per
  language (`POST /report`); anyone can read the total (`GET /total`).
  Turning both off removes the install from the count (`POST /leave`).
- With **Help improve** on, an install sends one report a day
  (`POST /usage`): usage counts, problems from the log (quoted words and
  paths blanked), the Mac's model, chip, memory, disk, macOS version and
  region. Never names, audio or anyone's words. One row per install and day.
- "I'm interested" on a buyer message sends the id, the message's id, the
  language and the matching hours (`POST /interest`).
- **Delete what was shared** removes every row of the id (`POST /forget`).

No IP addresses are stored.
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
the hours of its own data in that language and, if at least an hour fits and
the user doesn't support Sokki & Open Source, sends the notification
"Someone wants German data · 12 hours of your anonymised data fit. You'd get
a share of the profit." and shows a card on the Your data page with
"I'm interested" and "Not interested". `text` is optional and shown on the
card:

```bash
wrangler d1 execute verbatim-counter --remote --command \
  "INSERT INTO cache (key, value, updated) VALUES ('notice', '{\"language\": \"de\", \"text\": \"A team building German speech recognition wants 500 hours.\"}', unixepoch()) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated = excluded.updated"
```

Any other text is shown as it is, to everyone, as a notification and on the
page. Clear the message:

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
