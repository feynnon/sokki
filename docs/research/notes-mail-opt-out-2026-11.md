# Opt-out for "Notes to everyone" without reading mail (26 Nov 2026)

**Task (Robin, 26.11.):** the notes mail says "Reply stop". Since 25.11. Sokki only has Gmail's send scope, so it can no longer read that reply. How can a recipient tell the sender's Sokki "no more mails", through sokki.org or through something the app itself hosts?

## Result

**Recommended: a small opt-out drop box on sokki.org, in the existing Cloudflare Worker (`counter/`, D1 database, EU).**

1. When it sends, Sokki puts a link in every mail: `https://sokki.org/stop/<token>`. The token is random (128 bit). Sokki keeps the map token → recipient address locally and sends nothing else to the Worker beforehand.
2. The recipient clicks the link. The page asks "No more meeting notes from <sender name>?" with one button. A POST stores `(token, time)` in D1. Nothing else is stored: no address, no IP address.
3. Sokki asks the Worker every hour, and when it starts: "Which of my tokens were used?" It sends its own tokens, or a per-install secret it registered when creating them. The Worker answers and deletes the rows. Sokki adds the matching addresses to the existing "no mail" list (`objections`) and shows them in Settings.
4. The same URL also goes into the headers `List-Unsubscribe: <https://sokki.org/stop/<token>>` and `List-Unsubscribe-Post: List-Unsubscribe=One-Click` (RFC 8058). Mail apps can then show their own "Unsubscribe" button. Not verified: whether Gmail shows the button for a low-volume personal sender. Google documents the button mainly for bulk senders.

Why this way:
- **Works when the Mac is off or asleep.** The click waits in the drop box until Sokki asks.
- **No new scope.** Gmail stays send-only, so Google's review stays "sensitive", not "restricted" (restricted would need a paid security audit).
- **Almost nothing stored.** Random tokens are linked to an address only on the sender's Mac. The Worker deletes a row as soon as Sokki fetches it.
- **Free and already running.** The Worker, D1, rate limiting and the privacy-page section for it already exist. The privacy page needs one paragraph about the opt-out link.

## Alternatives considered

| Way | Why not (now) |
|---|---|
| Read "stop" replies (`gmail.metadata` / `gmail.readonly`) | Restricted scope: Google's security audit (CASA), paid each year. Dropped on 25.11. |
| The app hosts the page itself (local server, reached through a tunnel such as iroh/n0 or Cloudflare Tunnel) | The link is dead whenever the Mac sleeps or is offline, which is when most clicks happen. It would also need a public relay per user. iroh has no store-and-forward, so a drop box is still needed. |
| `mailto:` unsubscribe to the sender | Lands in the sender's inbox, which Sokki can't read. It works only as today: the person reads "stop" and adds the address by hand. |
| A calendar signal (recipient declines the event) | Already respected: declined attendees get nothing. It says nothing about future meetings. |

## Effort (estimate)

- Worker: two routes (`GET/POST /stop/<token>`, `POST /stop/poll`), one D1 table, about 80 lines plus tests.
- App: a token per recipient per mail, a link line in the mail and the two headers in the Gmail path, an hourly poll, and merging into `objections`. About 150 lines plus tests.
- Privacy page: one paragraph. Robin approves the text.

## Sources

- RFC 8058, One-Click Unsubscribe: https://www.rfc-editor.org/rfc/rfc8058
- Google sender guidelines (one-click unsubscribe for bulk senders): https://support.google.com/a/answer/81126
- Google OAuth scope categories (sensitive vs. restricted, CASA): https://developers.google.com/identity/protocols/oauth2/production-readiness/restricted-scope-verification
- Existing Worker: `counter/` (wrangler.toml: D1 `verbatim-counter`, rate limits, EU R2).
