# Eval sets

Sokki's test and validation data lives outside this repository, one folder per set under
`$SOKKI_EVALS` (default `~/sokki-evals`). This page is the index: every set, where it lives, how to
get it, and which scripts and results use it. It holds no data and no private text.

Results (numbers only) go to `docs/research/`, with the date, the task, the set and its sample.

## For contributors: test with the public sets

The public sets come straight from their sources; nothing is re-hosted except our own German sample
on Hugging Face. One command downloads what you need into `~/sokki-evals` (or `$SOKKI_EVALS`) and
checks each file against the checksum Sokki's results used:

```bash
python3 mobile/keyboard/tools/fetch_evals.py --list
python3 mobile/keyboard/tools/fetch_evals.py de-clean-10k heliboard-dicts typing37k
```

| To test | Set | Where it comes from | Then run |
|---|---|---|---|
| Wrong autocorrect edits on clean German | `de-clean-10k` | [huggingface.co/datasets/Shannonigan/de-clean-10k](https://huggingface.co/datasets/Shannonigan/de-clean-10k) | `score_de_clean.py <sokki-typos binary>` |
| Typos people really make (phone, English) | `typing37k` | [userinterfaces.aalto.fi/typing37k](https://userinterfaces.aalto.fi/typing37k/) | `public_pairs.py typing37k`, then `score_context.py` or `robin_eval.py` |
| Typos on physical keyboards (English) | `136m` | [userinterfaces.aalto.fi/136Mkeystrokes](https://userinterfaces.aalto.fi/136Mkeystrokes/) | `public_pairs.py 136m`, then as above |
| The keyboard's word lists | `heliboard-dicts` | [codeberg.org/Helium314/aosp-dictionaries](https://codeberg.org/Helium314/aosp-dictionaries) (pinned commit) | needed by `robin_eval.py`, `gen_lexicon.py` |
| Language models, bigrams, the source of `de-clean-10k` | `text-corpora` | [Leipzig Corpora Collection](https://wortschatz.uni-leipzig.de/en/download), [Tatoeba](https://tatoeba.org/downloads), [Wikimedia dumps](https://dumps.wikimedia.org/) | `ngram_lm.py`, `make_de_clean.py` |

Sources and checksums: `mobile/keyboard/tools/eval_sources.json`. If a file differs (Tatoeba changes
weekly, Wikimedia drops old dumps), the script says so; your numbers may then differ a little from
`docs/research/`.

The private sets (the maintainer's typing, dictations and calls) can't be shared. Their results are
in `docs/research/`; to get comparable numbers, use your own data.

## Test on your own data

Your data stays on your machine, in its own set folder. Don't commit it, and don't put it in a
test or fixture (tests use made-up data only).

**Your typing.** Make a set with your own messages and label the words the keyboard doesn't know:

    ~/sokki-evals/my-typing/data/texts-<anything>.txt   your messages, plain text
    ~/sokki-evals/my-typing/data/typo-labels.json       [{"word": "schiken", "label": "typo", "meant": "schicken"},
                                                         {"word": "Kita", "label": "intended"}]

```bash
SOKKI_TYPING_SET=my-typing python3 mobile/keyboard/tools/robin_eval.py now
```

It needs `heliboard-dicts`, `typing37k` and `136m` too (fetch them first). A `phone/` folder with the
keyboard's journal and tap log is optional. Unlabelled unknown words count as intended.

**Typed/meant pairs.** Any `.jsonl` of `{"typed": …, "meant": …, "prev": [up to 8 words before],
"next": …}` works with `score_context.py`.

**Meeting transcripts.** Make a set like `real-calls`: one folder per call with `raw.json` (the
uncorrected transcript, from `crates/meeting-e2e/examples/raw_transcript.rs`) and `labels.json`
(misheard words to fix, unusual words to keep; format in `scripts/correction_eval.py`). Then point
`SOKKI_EVALS` at the folder that holds your `real-calls/`.

Data shared through the app (Settings, "Share all my recordings") goes to Sokki's counter service,
not into these sets. A build from source without `SOKKI_COUNTER_URL` sends nothing.

## Layout

    $SOKKI_EVALS/<set>/README.md   what it is, source, licence, private or public, how to get or
                                   rebuild it, scripts that use it, fixed samples (seed + recipe)
    $SOKKI_EVALS/<set>/data        the files (for older sets a link to where they were first made)
    $SOKKI_EVALS/runs/<area>/      outputs of eval runs: scores, examples, caches

Python tools find a set with `mobile/keyboard/tools/evals.py`:

```python
import evals
sample = evals.path("de-clean-10k", "de-clean-10k.jsonl")   # exits with a clear message if missing
out = evals.runs("typing", "loop-log.md")                    # outputs of a run
```

`scripts/correction_eval.py` and `crates/meeting-e2e/examples/summary_eval.rs` read
`$SOKKI_EVALS/real-calls/data` the same way.

## Rules

- **Private sets** (the maintainer's typing, dictations, calls) stay on the maintainer's Mac. They
  never go into the repo, a test, a fixture, a cloud service or anywhere public. Scripts that read
  them print counts, and examples only to the terminal or to `runs/`.
- **Fixed samples:** a sample keeps its seed and recipe (the script that made it, saved next to
  it), so later results compare with earlier ones. A changed sample gets a new name.
- **Don't rewrite a set.** Add files with new names instead.
- **New set:** a folder with a README in `$SOKKI_EVALS`, and a row here.

## Typing and autocorrect

| Set | Private? | What | Get it | Used by | Results |
|---|---|---|---|---|---|
| `maintainer-typing` | private | The maintainer's messages (~47k words, mostly typed on the Mac), every word the keyboard didn't know labelled typo or intended, phone tap log and journal | Only on the maintainer's Mac | `mobile/keyboard/tools/robin_eval.py`, `caps_truth.py`, `synth_typos.py`, `seed_personal_words.py`, `typo_pairs.py`, `typing_eval.py`; `crates/sokki-typos/tools/parity.py` | `research/mac-autocorrect-2026-11.md`, `research/word-learning-2026-11.md` |
| `maintainer-typing-synthetic` | private | Made-up German typos on the maintainer's words (each copies a real typing37k slip, QWERTZ), and the words as correct pairs | `synth_typos.py` (seed 1) from the two sets above | `score_context.py`, `score_small_lm.py`, `bench_device.py` | `research/mac-autocorrect-2026-11.md` |
| `typing37k` | public | Phone typing, 37k people, English. Palin et al. 2019 | [userinterfaces.aalto.fi/typing37k](https://userinterfaces.aalto.fi/typing37k/), then `public_pairs.py typing37k` | `robin_eval.py`, `score_context.py`, `score_small_lm.py`, `synth_typos.py`, `bench_device.py`, `parity.py` | `research/mac-autocorrect-2026-11.md` |
| `136m` | public | Physical keyboards, 168k people, English. Dhakal et al. 2018. Research and non-commercial use with attribution | [userinterfaces.aalto.fi/136Mkeystrokes](https://userinterfaces.aalto.fi/136Mkeystrokes/), then `public_pairs.py 136m` | `robin_eval.py`, `score_context.py`, `score_small_lm.py`, `parity.py` | `research/mac-autocorrect-2026-11.md` |
| `de-clean-10k` | public | 10,000 edited German sentences from Leipzig news, web and Wikipedia: nearly every change autocorrect makes is wrong, so it counts wrong edits | `fetch_evals.py de-clean-10k` ([Hugging Face](https://huggingface.co/datasets/Shannonigan/de-clean-10k)), or rebuild with `make_de_clean.py` (needs `text-corpora`) | `score_de_clean.py <sokki-typos binary A> [B]` (typos model, `fixes` mode; counts edits by kind, diffs two builds) | `research/mac-autocorrect-2026-11.md` |
| `text-corpora` | public | Leipzig 1M corpora (deu news 2023, web 2021, Wikipedia 2021; eng news 2023, web 2018, Wikipedia 2016), Tatoeba deu/eng, Wikipedia dumps 2026-11-01, and the LMs, bigram tables and case counts built from them | [wortschatz.uni-leipzig.de/en/download](https://wortschatz.uni-leipzig.de/en/download) (downloads CC BY per the [terms](https://wortschatz.uni-leipzig.de/en/usage), read 2026-11-26), [tatoeba.org/downloads](https://tatoeba.org/downloads) (CC BY 2.0 FR), [dumps.wikimedia.org](https://dumps.wikimedia.org/) (CC BY-SA 4.0) | `ngram_lm.py`, `gen_bigrams.py`, `gen_case_counts.py`, `gen_known_words.py`, `score_jamspell.py`, `score_small_lm.py`, `wiki_sentences.py` | `research/mac-autocorrect-2026-11.md` |
| `heliboard-dicts` | public | The keyboard's word lists, German and English | [codeberg.org/Helium314/aosp-dictionaries](https://codeberg.org/Helium314/aosp-dictionaries) (licence per dictionary in its README) | `robin_eval.py`, `crates/sokki-typos/tools/gen_lexicon.py`, `gen_known_words.py` | `research/mac-autocorrect-2026-11.md` |
| `keyboard-device-bench` | private | Input and answers of the phone keyboard's decoder bench (typing37k + `maintainer-typing-synthetic`) | `bench_device.py prepare` (seed 1) | `bench_device.py` | — |
| `jamspell` | public | JamSpell 0.0.12 build, Python module and models, for comparison | [github.com/bakwc/JamSpell](https://github.com/bakwc/JamSpell) (MIT), build steps in `score_jamspell.py` | `score_jamspell.py` | — |

Run outputs of these tools (`loop-log.md`, `eval-examples-*.json`, `scores-*.txt`, `small-lm/`) go
to `$SOKKI_EVALS/runs/typing` and stay private.

### Fixed samples

| Set | Sample | Seed and recipe |
|---|---|---|
| `de-clean-10k` | 10,000 sentences (156,983 words): 3,334 news + 3,333 web (6,651 sites in all) + 3,333 Wikipedia | `random.seed(1)`; 6–35 words, ends in `.`, `!` or `?`, ≥ 85 % letters and spaces, no URL or bullet; news and web: one sentence per source site before any site gets a second; Wikipedia: `random.sample`. `mobile/keyboard/tools/make_de_clean.py` rebuilds it byte for byte. Use the full 10k every run; it counts wrong edits only. Baselines: 0.2.15 1,001 edits in 919 sentences, 0.2.16 999 in 918 (`research/mac-autocorrect-2026-11.md`) |
| `typing37k`, `136m` | `parity.py`: 1,500 typos + 2,500 correct words each | `robin_eval.sample(path, 1500, 2500, seed=1)` |
| `typing37k`, `136m` | `robin_eval.py` public score | `robin_eval.sample(..., seed=1)` |
| `typing37k`, `136m`, synthetic | `score_context.py` | up to `CAP` = 10,000 typos and 10,000 correct words per file, `random.seed(1)` |
| `maintainer-typing` | `robin_eval.py` umlaut test | 1,500 umlaut words, `random.seed(2)`; everything else: every word |
| `keyboard-device-bench` | 4,000 typing37k + 2,000 made-up typos + 2,000 correct words | `bench_device.py prepare`, `random.seed(1)` |

## Dictation, meetings and summaries

| Set | Private? | What | Get it | Used by | Results |
|---|---|---|---|---|---|
| `dictation-live-vs-final` | private | 34 Mac dictations: recordings, live text, pasted text, two reference transcripts, the test app's logs per cleanup round | Only on the maintainer's Mac | `crates/meeting-e2e/examples/reference_transcripts.rs`, `scripts/test-dictation.sh`, analysis scripts in the set | `research/live-vs-final-2026-11.md`, `research/data/live_vs_final.json` |
| `dictation-cleanup-2026-11-24` | private | 38 dictations (30–484 words), ideal corrections, blind ratings, model and summary runs of 23–24 Nov 2026 | Only on the maintainer's Mac | scripts in the set | `research/dictation-formatting-and-cleanup-2026-11.md`, `research/muesli-and-cleanup-log-2026-11.md`, `research/meeting-summaries-2026-11.md`, `research/data/` (ratings, latency, summary runs) |
| `real-calls` | private | Two real calls: uncorrected transcripts per speech model, hand labels (misheard words, words that must stay), old correction runs | Only on the maintainer's Mac | `scripts/correction_eval.py`, `crates/meeting-e2e/examples/summary_eval.rs`, `raw_transcript.rs` (writes `raw.json`) | `MEETINGS.md` (real-call evaluation), `KNOWN_ISSUES.md` F4 |

### Data read from the app, not copied into a set

Some evals read the installed app's own data in `~/Library/Application Support/org.sokki.app/`
(private, live; read only, never write to it):

- `meetings/`: real meetings, used by `crates/meeting-e2e/examples/live_eval.rs` (one meeting
  folder), `research/speaker-names-call-apps-2026-11.md` (46 meetings) and
  `research/meeting-summaries-2026-11.md` (4 meetings).
- `history.db`, `voice-data/`: dictation history, the source of the dictation sets above.

### Made-up data in the repo

Public, invented, fine to commit:

- `testdata/sample_meeting/` (script and truth) and `testdata/generate.py`, which builds the
  synthetic fixtures in `testdata/audio/` (gitignored) with macOS voices. Used by
  `crates/meeting-e2e` (`pipeline_e2e`, `asr_baseline`).
- `scripts/german_probe.py`: 14 sentences in the script itself.

## Add a set

1. Make `$SOKKI_EVALS/<set>/` with `data/` and a `README.md` (copy another set's README: what,
   private or public, source, licence, how to get or rebuild, used by, fixed samples).
2. Save the recipe (the script that made the sample, with its seed) in `data/`, or in the repo if the set is public. A public set gets a row in `eval_sources.json` with its source and checksum.
3. Read it with `evals.path("<set>", ...)`, not a hard-coded path.
4. Add a row above, and name the set in the `docs/research/` file that reports its results.
