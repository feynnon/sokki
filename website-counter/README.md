# sokki.org visitor count

A Cloudflare Worker in front of sokki.org (GitHub Pages). It counts each page
as it is delivered and sends the count to PostHog EU (project 367071, org
"sokki"). Visitors run no tracking script and get no cookie, so the site needs
no consent banner.

- **Page views** (`$pageview`): HTML pages with status 200, from browsers only
  (bots, link previews, curl and prefetches are skipped).
- **`download_click`, `github_click`**: `sokki.org/download` and
  `sokki.org/github` count a click, then redirect to the latest release or the
  repository. The website links there.
- **`zip_downloads`**: every hour, new zip downloads from GitHub's release
  numbers. The Terminal install (`install.sh`) downloads the zip too.

Each event has the page, the referring site, the link's tag (`?s=`, `utm_*`,
`ref`), country (from Cloudflare), browser, OS, device type, and a visitor
number: a SHA-256 of a random daily salt, the IP address, the user agent and
the host. Salts live in KV for two days, then expire, so visitor numbers can't
be linked across days. No IP address or user agent is stored or sent.
`$session_id` is one UUIDv7 per visitor and day.

Known: scanners that pretend to be a browser still count. When Cloudflare
issued sokki.org's certificate (24.11.2026, 21:07 UTC), certificate-log
scanners loaded the home page 28 times in one minute, mostly as "Chrome" and
"Edge" from the US. No filter for them yet (few rules: add one when they
distort real numbers).

The wording on sokki.org/privacy ("This website") must match what this Worker
records.

## Deploy

```bash
cd website-counter
npx wrangler deploy
./scripts/routes.sh   # asset files bypass the Worker; fail open on every route
```

Past the free plan's 100,000 Worker runs a day, the site keeps working and
only counting pauses until midnight UTC.

## Try it locally

```bash
(cd website && python3 -m http.server 8090) &
npx wrangler dev --local --port 8788 --var ORIGIN:http://127.0.0.1:8090 \
  --var POSTHOG_HOST:http://127.0.0.1:8799 --var POSTHOG_KEY:phc_test
```

Point `POSTHOG_HOST` at any server that logs POST bodies to see the events.
