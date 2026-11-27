# Memory index: words vs trigram search on German meetings (26 Nov 2026)

**Task** (memory layer step 1, cli-index session): measure recall@5 of the two keyword searches in
`index.sqlite` (FTS5 unicode61 and FTS5 trigram), and their fusion, on a small made-up German
question set, before choosing the default of `sokki search`. The research behind the index:
`memory-layer-2026-11.md` ("Store", "Eval gate before vectors").

**Set:** `memory-search-de` (`docs/evals.md`): 10 made-up meetings, 46 turns, 36 keyword
queries as an agent sends them (one to three words), each with the turn(s) that answer it. Public,
invented, outside the repo in `$SOKKI_EVALS/memory-search-de`.

**Run:** `crates/sokki-index/tests/recall_eval.rs`
(`cargo test --test recall_eval -- --ignored --nocapture`). Each meeting is rendered as a real Sokki
note (`meeting_core::note`), indexed by `sokki_index::sync`, and searched with
`--section transcript`. A hit is an expected turn's paragraph cite among the top k. Ranking is
deterministic (ties broken by row); three runs gave the same numbers.

## Modes

| Mode | What it matches |
|---|---|
| `words` | FTS5 `unicode61 remove_diacritics 2`; each query word as a prefix (`"vertrag"*`), OR-ed, ranked by bm25 |
| `trigram` | FTS5 `trigram remove_diacritics 1`; each query word of 3+ letters as a substring, OR-ed, bm25 |
| `both` | the two lists fused by reciprocal rank (k = 60); the default of `sokki search` |

## Results

| Mode | recall@1 | recall@5 | MRR |
|---|---|---|---|
| words | 0.72 | 0.75 | 0.74 |
| trigram | 0.89 | 0.89 | 0.89 |
| both | 0.86 | 0.89 | 0.88 |

recall@5 by category:

| Category | Questions | words | trigram | both |
|---|---|---|---|---|
| A part of a compound ("Vertrag" for "Rahmenvertrag") | 15 | 0.67 | 1.00 | 1.00 |
| Whole words as spoken | 10 | 1.00 | 1.00 | 1.00 |
| Typed without umlauts ("Kundigungsfrist") | 3 | 1.00 | 1.00 | 1.00 |
| English meeting | 2 | 1.00 | 1.00 | 1.00 |
| Another inflection ("reklamieren" for "reklamiert") | 3 | 0.67 | 0.67 | 0.67 |
| English query, German meeting | 3 | 0.00 | 0.00 | 0.00 |

## What it shows

- **Compounds need the trigram table.** Whole words found 10 of 15 compound questions, and only
  those where the query word starts the compound ("Kündigung" → "Kündigungsfrist" through the
  prefix). "Vertrag", "Lieferung", "Ticket", "Sicherung" and "Bestellung" inside a compound were
  found only by trigrams.
- **Fusion costs one first place.** `both` lost one recall@1 against `trigram` ("Ziel Zahlung":
  the word prefix "ziel*" pulled "Zielgruppenanalyse" ahead of "Zahlungsziel"). Same recall@5.
- **Both keyword modes miss** a changed ending (stemming would help, or vectors) and every English
  question about a German meeting. These are the cases for the vector eval
  (`memory-layer-2026-11.md`, "Eval gate before vectors").

## On the maintainer's real notes (counts only)

Indexed read-only into a scratch file on 26 Nov 2026, then deleted: 38 meetings, 39 dictations,
2,198 transcript turns, 41 tasks, 50 decisions, 6 people, no errors. First sync 0.90 s, the next
one (nothing changed) 3 ms; the index file 2.7 MB. Code found an evidence turn for 50 of 50
decisions and 40 of 41 tasks (`evidence_origin: "match"`); whether each is the right turn is not
measured (`KNOWN_ISSUES.md` IX1). No note had `segments.jsonl` yet (written from now on), so no turn
had exact times or a channel. 916 of 2,198 turns had a confirmed name.

## Decision

`sokki search` uses `both` by default, with `--mode words|trigram` to pick one. Trigram alone
scored as well here, but this set has few distractors: on a large archive a three-letter substring
also matches unrelated words ("rat" in "Beratung", "Rathaus"), which the whole-word list ranks
down. That cost is not measured yet (`KNOWN_ISSUES.md`).

**Limits:** 36 questions on 46 turns, made up by the same session that built the index. A sanity
check of the two tokenizers, not a benchmark. The private 50–100 question eval on real meetings
(`memory-layer-2026-11.md`) is still to be done.

**Sources:** SQLite FTS5 tokenizers, unicode61 and trigram (<https://sqlite.org/fts5.html>);
reciprocal rank fusion, Cormack, Clarke and Büttcher 2009
(<https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf>). Raw run:
`$SOKKI_EVALS/runs/memory-search/2026-11-26.json` (per question: the rank of the first hit per
mode).
