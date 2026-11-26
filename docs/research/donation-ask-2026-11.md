# Research: wording of the donation ask

**Date:** 20 Nov 2026.

**Task:** two research agents. One looked for field experiments, A/B tests and studies on which wording of a small donation ask raises the most money. The other looked for real conversion and revenue numbers from in-app and open-source donation asks. Both were briefed on Sokki's milestone card, shown at 100/200/500/1,000 hours, which leads to GitHub Sponsors ($5 a month or once).

## Result

**What raises the most, by strength of evidence:**
1. **One clear, modest amount.** German field experiments show that higher suggested amounts lose donors. A €100 suggestion got 2.8% of patrons to give and €200 got 2.3%, against 3.1% with none; revenue per letter stayed flat [1]. Raising suggestions by about 20% cut the share who gave by about 15% [2].
2. **Thanks after value was received.** A gift sent with the letter raised giving by 17% (small gift) or 75% (large gift) [3]. Sokki's users have already had their hours.
3. **A personal appeal from the maker.** Wikipedia's "personal appeal from Jimmy Wales" banner got 2.87% click-through and raised $47k, against 0.90% and $2.9k for the next English banner [4].
4. **Monthly beats one-time** in annual revenue (+75% in one test), but this evidence comes only from vendors who sell recurring giving [5].

**What to avoid:**
- **"Already N people support this."** In six randomized Wikipedia Germany trials it lowered donations by 13–18% [6].
- **"Even €1 helps."** More people give, but each gives less, so the total stays flat (meta-analysis of 34 studies) [7].

**Coffee framing:** nobody has tested it against a plain amount. Comparing a gift to a small treat helped a little in one study [8].

**Real numbers from apps:**
- Overcast: 1.9% of users paid a goodwill $1/month; with patron perks it reached just under 3% [9].
- Thunderbird gets about 1 donation per 30 downloads; LibreOffice about 1 per 300 [10].
- KDE's one in-app notice a year doubled two months' worth of PayPal donations on its first day [11].
- In Wikipedia's Android app, a reminder tied to reading was closed about half as often as a banner [12].

**Platform data:**
- Buy Me a Coffee: forcing an account loses 26% of people; checkout without one brings 21% higher value [13].
- GitHub Sponsors needs a GitHub account and charges in dollars.
- On GitHub Sponsors, "buy me a coffee" sponsors brought one creator about $5k of his more than $1M; perks brought most of the rest [14].

## Decision (Robin, 20–23 Nov 2026)

- **Card text:** asks the user to chip in AI tokens, signed "– Robin". The button reads "Chip in tokens" / "Tokens spendieren".
- **Card off:** `SPONSORS_LIVE = false` keeps it hidden until GitHub Sponsors is approved.
- **Open:** whether to add an account-free link in euros (Ko-fi or Buy Me a Coffee) next to GitHub Sponsors.

## Sources

1. https://www.povertyactionlab.org/evaluation/use-nonbinding-contribution-suggestions-charitable-fundraising-scheme-germany
2. https://docs.iza.org/dp8680.pdf
3. https://www.econometricsociety.org/publications/econometrica/2007/09/01/gift-exchange-field
4. https://meta.wikimedia.org/wiki/Fundraising_2010/Banner_testing
5. https://www.nextafter.com/?p=17383
6. https://arxiv.org/abs/2106.11128
7. https://experts.arizona.edu/en/publications/the-legitimization-of-paltry-contributions-as-a-compliance-gainin/
8. https://insight.kellogg.northwestern.edu/article/cookies_cocktails_and_charitable_giving
9. https://marco.org/2016/09/
10. https://listarchives.libreoffice.org/global/marketing/2023/msg00065.html
11. https://pointieststick.com/2024/12/02/i-think-the-donation-notification-works/
12. https://www.mediawiki.org/wiki/Wikimedia_Apps/Team/Android/Customizable_Donation_Reminder_Experiment
13. https://buymeacoffee.com/blog/earn-more-with-one-time-tips/
14. https://calebporzio.com/i-just-cracked-1-million-on-github-sponsors-heres-my-playbook
