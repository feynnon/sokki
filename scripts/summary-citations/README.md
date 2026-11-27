# Summary citations bench

The bench behind `docs/research/summary-citations-2026-11.md`: can each point of a meeting summary
point at the transcript lines it comes from, and what does that cost?

Approaches, per meeting and run:

- **A** today's summary request, byte for byte (captured from `meeting-core` through `proxy.py`).
- **B** one request: numbered transcript lines, every point an object with `cites` (JSON schema).
- **C** no language model: `cite.py` matches each point's words against the transcript lines.
- **D** A, then a follow-up turn in the same chat asking for the timestamps of each point.

Run (a heavy job: Sokki's llama-server with the built-in model):

    printf "m1\t$SOKKI_EVALS/summary-citations-made-up/data/m1.md\n" > meetings.tsv
    ~/.local/bin/build-slot submit --priority low --name "summary-citations: bench" -- \
        scripts/summary-citations/run.sh meetings.tsv 2
    python3 scripts/summary-citations/analyze.py      # Q1 and Q3 numbers
    python3 scripts/summary-citations/make_judge.py   # blind packets for judge agents
    python3 scripts/summary-citations/score_judge.py  # Q2 numbers from their verdicts

Everything it writes (captured requests, summaries, judge packets) holds transcript text and goes to
`$SOKKI_EVALS/runs/summary-citations/`, never into the repo. Data: the `summary-citations-made-up` set
(`docs/evals.md`) and, on the maintainer's Mac, meetings from the app.
