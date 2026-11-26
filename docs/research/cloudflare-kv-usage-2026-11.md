# Cloudflare KV usage and the free plan (2026-11-26)

**Task** (Robin, 2026-11-26): Cloudflare mailed "KV operations are nearing the
daily cap: 50 % of the daily Workers KV free tier". Find which operation,
which Worker and route, whether it runs over before 00:00 UTC, and weigh a
lasting fix against Workers Paid ($5/month).

**Sources**: Cloudflare GraphQL Analytics API (`kvOperationsAdaptiveGroups`,
`workersInvocationsAdaptive`, `d1AnalyticsAdaptiveGroups`,
`r2OperationsAdaptiveGroups`, account-wide, queried 2026-11-26 13:00 UTC with
wrangler's OAuth token); D1 tables `kv_ops` and `cache`; `wrangler kv key list`
of the `AUDIO` namespace; the code of `counter/` and `website-counter/`.
Analytics numbers are sampled, so hourly figures are about ±5 %. Free limits:
cloudflare.com Workers KV/D1/R2 pricing pages (KV per day: 100,000 reads,
1,000 writes, 1,000 deletes, 1,000 lists).

## Result

- **Operation: deletes**, namespace `AUDIO` (`verbatim-counter`), from the
  hourly cron (`moveToR2`): moving the audio shared before R2 (25.11.) out of
  KV, 100 files per run, each one KV read + one KV delete. Ran from
  25.11. 21:00 UTC to 26.11. 08:00 UTC; the 50 % mark was passed around
  05:00 UTC.
- **26.11. total**: 838 deletes (D1 `kv_ops`: 837 of the counter's own budget
  of 900), 816 reads, 11 lists, 0 writes. KV `AUDIO` is now empty, the cron
  marked it done (`kv_moved`, 26.11. ~09:20 UTC). **No more KV deletes today:
  it does not run over.** Even if it had, the counter stops itself at 900.
- **What would have broken past 1,000 deletes**: only the KV part of "Delete
  what was shared" (waits for the next day, already handled). Uploads, usage
  reports, the update feed (GitHub) and the website redirects don't use KV
  deletes.
- **Per day, 18.–26.11.** (namespace `AUDIO`; the website's `SALTS` beside it):

| Day | deletes | writes | reads | lists | SALTS reads / writes |
|---|---|---|---|---|---|
| 18.11. | 521 | 655 | 1 | 11 | – |
| 19.–22.11. | 0–2 | 11–170 | 0 | 0–2 | – |
| 23.11. | 0 | 224 | 0 | 1 | – |
| 24.11. | **1,475** (over: the `/forget` of 874 clips) | 699 | 0 | 3 | 94 / 3 |
| 25.11. | 298 | 259 (last KV uploads) | 291 | 10 | 113 / 10 |
| 26.11. | 838 (the move) | 0 | 816 | 11 | 58 / 11 |

- **Side finding, D1**: the move's lookup (`json_each` against `donations`
  without an index) read up to 105,000 D1 rows per hour, 606,000 on 26.11.
  (free: 5 million a day). It ended with the move; since 09:00 UTC D1 reads
  are near zero.

## After the move: what is left in KV

- `verbatim-counter`: no uploads, reads or deletes in KV any more (R2 since
  25.11.). Until this change, each "Delete what was shared" still listed the
  empty KV twice (in the request and in the last look), within a budget of 100
  lists a day, so about 50 deletions a day before the last looks waited. Fixed
  2026-11-26: once `kv_moved` is set the counter never touches KV
  (`kvInUse`, test "once KV is moved to R2, deleting never lists KV").
- `sokki-website-counter` (`SALTS`): one read per Worker copy per day (the
  salt is cached in memory), one write per day for the salt, one write per
  hour for the zip total: ~60–110 reads and ~25 writes a day.

## Launch (early December) estimate

| | today | launch day, guess 50,000 page views and 2,000 new apps | free limit |
|---|---|---|---|
| KV deletes / writes / lists (counter) | 0 after the move | 0 | 1,000 each |
| KV reads (website salt) | ~100 | a few thousand at most (per Worker copy, not per view) | 100,000 |
| KV writes (website) | ~25 | ~25 | 1,000 |
| Worker runs, website | 860 | ~50,000–100,000 | 100,000 (then "fail open": the site works, only counting pauses) |
| Worker runs, counter | 130–1,300 | a few thousand (reports, uploads) | shared with the 100,000 |
| D1 rows read | ~60,000 | well under 1 million | 5,000,000 |
| R2 writes | ≤ 1,000 | a few thousand | ~33,000 a day (1M a month) |

## Recommendation

- **KV needs no Workers Paid.** The cap was a one-off from the move to R2,
  which is done. KV now costs a few hundred operations a day.
- **The only limit launch can reach is Worker runs** (100,000 a day, both
  Workers together), on a day with more than ~80,000 page views. Then the
  site keeps working and only visit counting and app reports pause until
  00:00 UTC. Workers Paid ($5/month, 10 million runs) would remove that; it
  is worth it only if launch-day numbers matter. Robin's choice, not urgent.
