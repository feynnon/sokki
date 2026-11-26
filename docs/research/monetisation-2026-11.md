# Research: how apps like Sokki make money

**Date:** 26 Nov 2026.

**Task:** four research agents, each with its own question:
- How do local and open-source dictation apps make money?
- How do local and open-source meeting-notes apps make money?
- Which mechanics have earned solo open-source developers real money, with first-hand numbers?
- Which grants can a solo developer in Germany get?

A fifth agent wrote the full report. That report is not in the repo: it sits at `reports/How apps like Sokki make money.md`.

## Result

**Comparable apps charge for something next to the free app, not for the app itself:**
- **A signed build:** VoiceInk is GPL v3, and its ready build costs $25–49 once. That pays one full-time developer [1].
- **Cloud features:** Spokenly and OpenWhispr keep all local use free and charge $8–14 a month for cloud features [2][3].
- **Companies:** Superwhisper reports seven-figure revenue from paid plans [4]; Granola sells team plans [5].
- **Free, no direct income:** Handy, Sokki's upstream, is fully free and funded by four company sponsors. It has 40k stars and publishes no figures [6].

**Donations alone stay small:**
- Of 9,363 developers on GitHub Sponsors, only 39.5% had any sponsor, and the median among them was 2 [7].
- A sponsorship from an organisation averages about 15 times an individual one [8].
- Perks change the picture. Caleb Porzio went from about $40k to over $100k a year in 90 days with sponsor-only screencasts in a private repo [9].

**What failed:** in each case the line between free and paid moved after users had settled in.
- Screenpipe switched its licence late [10].
- Hyprnote put local features behind Pro [11].
- Buzz's paid build had fewer features than the free one [12].
- Krita's price rise in its app store was rolled back [13].

**Grants:** none of the open calls fits Sokki well.
- NLnet Restack offers €5–50k and closes on 3 Jan 2027, but excludes small generative-AI projects. Since 20 Oct 2026 it allows LLMs only read-only during development [14][15].
- The Prototype Fund bars "vibe coding". Sokki's class 03 application was stopped on 24 Nov 2026 [16].

**Ranked options for Sokki** (estimates, not forecasts):
1. Ko-fi tips, account-free and in euros, behind the milestone card: low effort.
2. GitHub Sponsors with a build-log tier that gives access to a private repo: medium effort, ongoing.
3. Company sponsors, once Sokki has traction.
4. A grant for a hand-written component that others can reuse.
5. Identical "supporter" copies in the app stores (Android first).
6. An organisation edition with paid support.
7. Optional hosted sync.

**Not recommended:** a paid-only build, Pro features behind a paywall, a later licence change, selling recordings.

## Sources

1. https://github.com/beingpax/VoiceInk · https://tryvoiceink.com/
2. https://spokenly.app/pricing
3. https://openwhispr.com/compare/voiceink
4. https://aiwiki.ai/wiki/superwhisper (quoting The Globe and Mail, May 2026)
5. https://www.granola.ai/pricing
6. https://github.com/cjpais/handy
7. https://ar5iv.arxiv.org/html/2202.05751
8. https://github.blog/open-source/maintainers/100-million-for-open-source-a-milestone-built-by-the-community/
9. https://calebporzio.com/i-just-hit-dollar-100000yr-on-github-sponsors-heres-how-i-did-it
10. https://hn.nuxt.dev/item/49024620
11. https://hn.svelte.dev/item/44725306
12. https://alternativeto.net/software/buzz-captions/about
13. https://krita.org/en/posts/2023/krita-in-stores-update/
14. https://nlnet.nl/restack/
15. https://nlnet.nl/news/2026/20261020-AI-policy.html
16. https://web.archive.org/web/20260723125027/https://www.prototypefund.de/en/blog/ai-at-prototype-fund
