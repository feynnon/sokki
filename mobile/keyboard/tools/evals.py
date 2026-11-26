"""Sokki: where the eval sets live. The index is docs/evals.md.

Eval data stays outside the repository, one folder per set:
    $SOKKI_EVALS/<set>/README.md   what it is, source, licence, private or public, how to get it
    $SOKKI_EVALS/<set>/data/       the files (often a link to where they were first made)
    $SOKKI_EVALS/runs/<area>/      outputs of eval runs (scores, examples, caches)
SOKKI_EVALS defaults to ~/sokki-evals.

    import evals
    sample = evals.path("de-clean-10k", "de-clean-10k.jsonl")
SPDX-License-Identifier: GPL-3.0-only
"""

import os
import sys

ROOT = os.path.expanduser(os.environ.get("SOKKI_EVALS", "~/sokki-evals"))


def missing(name, p):
    sys.exit(f"eval set '{name}' is missing: {p} not found.\n"
             f"  How to get it: docs/evals.md, section '{name}', or {os.path.join(ROOT, name, 'README.md')}.\n"
             f"  Sets elsewhere? Set SOKKI_EVALS (now {ROOT}).")


def path(name, *parts, optional=False):
    """A file or folder of an eval set. Exits with a clear message if it is missing,
    unless optional (then the path is returned either way)."""
    p = os.path.join(ROOT, name, "data", *parts)
    if not optional and not os.path.exists(p):
        missing(name, p)
    return p


def runs(area, *parts):
    """Where eval runs of one area write their outputs; the folder is created."""
    d = os.path.join(ROOT, "runs", area)
    if not os.path.isdir(d):
        os.makedirs(d, exist_ok=True)
    return os.path.join(d, *parts)
