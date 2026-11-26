# Research: Sokki income in the AI era (second round)

**Date:** 26 Nov 2026. This round follows up on `monetisation-2026-11.md`.

**Task:** four research agents, each with one question:
- How did Handy and other open-source projects get company sponsors?
- What exactly were Caleb Porzio's perks, and how do Obsidian and VoiceInk earn?
- Which money models still work now that AI tools let anyone rebuild an app?
- Which team features could Sokki offer without a cloud?

Robin's constraints for this round:
- No running costs.
- Free for everyone.
- Not a Superwhisper- or Granola-style subscription.
- A "convenience build" is worth little in the age of AI.

The full report is local only: `reports/Sokki income in the AI era.md`.

## Result

**Handy's sponsors came to it; it didn't sell to them.**
- The first logos (Wordcab, Epicenter) arrived two months after the Show HN in June 2025 [1].
- Epicenter sponsors because its own app uses CJ Pais's library [2].
- Sponsors get only a logo in the README and on the website. There are no tiers, no prices and no amounts, just a contact address.
- The bigger money went to his libraries:
  - a Mozilla.ai residency, which he pitched to them himself
  - compute credits from Modal and Blacksmith, after he asked
  - Hugging Face and Kortix sponsoring him on GitHub

**Caleb Porzio:** about $725k of roughly $1M came from sponsor-only screencasts, gated by GitHub login with the code in a private repo. About $200k came from company logos, and only about $5k from coffee-style tips [3].
- Tiers were $7 and $14 a month; he warns against $1–5 tiers.
- Monthly education subscriptions had "the worst churn".
- He lost about $50k when GitHub dropped PayPal [4].

**Obsidian:**
- Team of 7, no investors.
- Catalyst is a one-time $25 / $50 / $100 licence; its perks are early builds, a badge and a channel.
- The commercial licence ($50 per user per year) has been optional since February 2025, with a listing for companies that buy 25 or more.
- Sync and Publish have running costs, so they don't carry over to Sokki [5][6].

**VoiceInk:**
- GPL v3, one-time keys at $25 / $39 / $49, sold and checked through Polar.
- 7-day trial.
- Building it yourself is officially one command (`make local`), and it still sells. The reason is that the paid copy is the default way people get it [7][8].

**AI era:** no hard data shows paid apps losing sales to clones. What still sells, according to Hacker News threads, is the maintained original, trust (above all for apps that use the microphone) and a direct audience [9]. AI-lab programmes for maintainers are a cost cut, not income:
- Claude for Open Source gives 6 months of Max to maintainers of repos with 5,000+ stars.
- OpenAI gives 6 months of ChatGPT Pro, with no published threshold [10][11].

**Team features without a cloud:**
- Every shared file has exactly one author and lives in a drive the company already pays for (iCloud, Dropbox, Google Drive, SharePoint). Admin settings go through the company's device management (MDM).
- Ranked:
  1. sending a recap
  2. a team vocabulary
  3. a team folder
  4. shared templates
  5. asking across meetings
  6. actions to Slack or Linear
  7. MDM policy
- Live peer-to-peer sync waits [12][13].

## Ranked options (estimates)

| # | Option | Money, year one / breakout | Effort |
|---|---|---|---|
| 1 | Turn on the ask (Ko-fi plus GitHub Sponsors) | €150–800 / €1,500–8,000 | a few hours |
| 2 | Supporter licence (€25 / €49 / €99 once through Polar, perks only, nothing locked) | €175–700 / €1,750–7,000 | 2–4 days |
| 3 | Company sponsors the Handy way (formatting engine as its own repo, sponsor page) | €0 / €1,000–15,000 | 2–3 days, then 2 h a month |
| 4 | Organisation edition (free team features, optional licence, paid setup) | €0 / €2,000–10,000 per deal | 3–6 weeks |
| 5 | Sponsor-only build log | $0–250 / $250–1,500 a month | 2–4 h a week |
| 6 | AI-lab maintainer programmes | cost cut of about $1,200 per 6 months | 1 h |
| 7 | Grants for a reusable component | €5–50k at 11–14% odds | days |

**Ruled out:**
- a paid or trial-gated build
- Pro features behind a paywall
- subscriptions or seats like Superwhisper
- hosted sync
- licence branding clauses
- cloud speech-API ads

## Sources

1. https://github.com/cjpais/handy (README history)
2. https://github.com/epicenter-so/epicenter (Whispering release notes)
3. https://calebporzio.com/i-just-cracked-1-million-on-github-sponsors-heres-my-playbook
4. https://calebporzio.com/i-just-hit-dollar-100000yr-on-github-sponsors-heres-how-i-did-it
5. https://obsidian.md/pricing
6. https://obsidian.md/license
7. https://github.com/beingpax/VoiceInk/blob/main/BUILDING.md
8. https://tryvoiceink.com/
9. https://news.ycombinator.com (2025–2026 threads, listed in the report)
10. https://simonwillison.net/2026/Feb/27/claude-max-oss-six-months/
11. https://simonwillison.net/2026/mar/7/codex-for-open-source
12. https://www.granola.ai/pricing
13. https://nlnet.nl/restack/
