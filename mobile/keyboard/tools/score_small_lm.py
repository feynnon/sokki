"""Sokki: a small neural language model as the context scorer, against the word trigram model.

Same decision as score_context.py (the noisy channel): every candidate within
two slips and the typed word itself get
    log10 P_LM(word | left context) + LAMBDA * log10 P(typed | word),
the scores become shares, and the best word replaces the typed one only when it
is not the typed word and its share is at least TAU. Here P_LM is a small causal
LM (SmolLM2, Qwen2.5) reading the typed left context (up to 8 words, as typed).
To keep it cheap the LM only sees the typed word plus the 10 candidates with
the best trigram + channel score, i.e. the n-gram model still picks the
candidates. Words are folded (lower case, ss for ß), so each one is scored as
is, capitalised, and with ß restored when wordfreq has such a word; the best of
these counts. A word's score is the sum of its tokens' log-probs, " " + word
after context, word alone at the start of a sentence.

Inputs: the .hyps.json caches score_context.py writes. Runs in its own venv
(torch, transformers, plus wordfreq, symspellpy, numpy for score_context):

    uv venv -p 3.12 ~/.cache/sokki-lm-venv
    VIRTUAL_ENV=~/.cache/sokki-lm-venv uv pip install torch transformers wordfreq symspellpy numpy
    PY=~/.cache/sokki-lm-venv/bin/python
    $PY score_small_lm.py prep                                  # compact the caches once
    $PY score_small_lm.py score HuggingFaceTB/SmolLM2-135M      # LM scores per set (MPS)
    $PY score_small_lm.py time HuggingFaceTB/SmolLM2-135M cpu   # latency only
    $PY score_small_lm.py report [CAP] > scores-small-lm.txt    # sweep and compare

CAP (optional): only the first CAP typos and CAP correct words of each set.
SPDX-License-Identifier: GPL-3.0-only
"""

import collections
import json
import math
import os
import pickle
import statistics
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))
import score_context as sc  # noqa: E402

DATA = os.path.expanduser("~/sokki-typing-data")
OUT = os.path.join(DATA, "small-lm")
SETS = {
    "typing37k": f"{DATA}/public/typing37k-ctx.jsonl.hyps.json",
    "136m": f"{DATA}/public/136m-ctx.jsonl.hyps.json",
    "robin-typos": f"{DATA}/synthetic/robin-typos-ctx.jsonl.hyps.json",
    "robin-words": f"{DATA}/synthetic/robin-words-ctx.jsonl.hyps.json",
}
MODELS = ["HuggingFaceTB/SmolLM2-135M", "HuggingFaceTB/SmolLM2-360M", "Qwen/Qwen2.5-0.5B"]
TOP = 10
LAMS = (0.5, 0.75, 1.0, 1.25)
OOVS = (-8.0, -10.0, -12.0, -14.0)
TAUS = (0.9, 0.95, 0.98)
MAX_CHANGED = 1.0  # changed per 10k correct words allowed on typing37k and 136m
LN10 = math.log(10)


def slug(model):
    return model.split("/")[-1]


# --- prep: the caches hold every candidate; keep what the variants need -------------

def stream(path):
    """The items of a JSON list, one at a time (the caches are 0.7-1.5 GB)."""
    s = open(path, encoding="utf-8").read()
    dec = json.JSONDecoder()
    i = s.index("[") + 1
    while True:
        while s[i] in ", \n\r\t":
            i += 1
        if s[i] == "]":
            return
        obj, i = dec.raw_decode(s, i)
        yield obj


def eligible(h):
    """Items decide() may change at all (strict mode)."""
    t = h["typed"]
    return len(t) >= 3 and t.isalpha() and not any(c.isupper() for c in t[1:]) and not h["known"]


def prep_item(h):
    rec = {k: h.get(k) for k in ("typed", "meant", "w", "prev", "next", "known", "rule")}
    rec["typo"] = h["typed"] != h["meant"]
    rec["elig"] = eligible(h)
    if not rec["elig"]:
        return rec
    t0 = h["hyps"][0]
    cands = [x for x in h["hyps"][1:] if x[1][2] is not None]
    rec["t0"], rec["t0_lm"], rec["ch0"] = t0[0], t0[1], t0[3]
    rec["words"] = [x[0] for x in cands]
    rec["lm"] = np.array([x[1][1:3] for x in cands], dtype=np.float64).reshape(-1, 2)  # bigram, trigram
    rec["ch"] = np.array([x[3] for x in cands], dtype=np.float64)
    rec["top"] = np.argsort(-(rec["lm"][:, 1] + rec["ch"]), kind="stable")[:TOP].tolist()
    rec["base"] = sc.decide(h, order=3, lam=1.0, p_oov=-10, tau=0.95)  # check for decide_arrays
    return rec


def prep():
    os.makedirs(OUT, exist_ok=True)
    for name, path in SETS.items():
        t = time.time()
        recs = [prep_item(h) for h in stream(path)]
        pickle.dump(recs, open(f"{OUT}/{name}.prep.pkl", "wb"), protocol=5)
        el = sum(r["elig"] for r in recs)
        print(f"{name}: {len(recs)} items, {el} eligible "
              f"({sum(r['elig'] and r['typo'] for r in recs)} typos), {time.time() - t:.0f}s", flush=True)


def load_prep(name):
    return pickle.load(open(f"{OUT}/{name}.prep.pkl", "rb"))


def sample(recs, cap):
    """Indices of the first `cap` typos and `cap` correct words (the caches are in random order)."""
    if not cap:
        return list(range(len(recs)))
    n = collections.Counter()
    out = []
    for i, r in enumerate(recs):
        if n[r["typo"]] < cap:
            n[r["typo"]] += 1
            out.append(i)
    return out


# --- neural LM scores ------------------------------------------------------------------

LEIPZIG_WORDS = [f"{DATA}/corpora/{c}/{c}-words.txt" for c in ("deu-de_web_2021_1M", "deu_news_2023_1M")]


def unfold_map(min_count=5):
    """Folded word -> its likeliest ß spelling ("strasse" -> "straße"). wordfreq folds ß
    away, so the spellings come from the German Leipzig word lists."""
    count = collections.Counter()
    for path in LEIPZIG_WORDS:
        if os.path.exists(path):
            for x in open(path, encoding="utf-8"):
                p = x.rstrip("\n").split("\t")
                if len(p) == 3 and "ß" in p[1] and p[1].isalpha():
                    count[p[1].lower()] += int(p[2])
    best = {}
    for w, f in count.items():
        k = sc.tm.fold(w)
        if f >= min_count and f > best.get(k, ("", 0))[1]:
            best[k] = (w, f)
    return {k: w for k, (w, _) in best.items()}


def forms(word, unss):
    out = [word, word.capitalize()]
    if word in unss:
        out += [unss[word], unss[word].capitalize()]
    return list(dict.fromkeys(out))


class Scorer:
    def __init__(self, model, device="mps"):
        import torch
        from transformers import AutoModelForCausalLM, AutoTokenizer
        self.torch = torch
        self.dev = torch.device(device)
        self.tok = AutoTokenizer.from_pretrained(model)
        self.model = AutoModelForCausalLM.from_pretrained(model, dtype=torch.float32).to(self.dev).eval()
        self.start = self.tok.bos_token_id if self.tok.bos_token_id is not None else self.tok.eos_token_id
        self.params = sum(p.numel() for p in self.model.parameters())

    def enc(self, text):
        return self.tok.encode(text, add_special_tokens=False)

    def logprobs(self, prev, strings):
        """log10 P(string | start token + prev words) for each string, in one batch,
        reusing the context's KV cache."""
        torch = self.torch
        with torch.no_grad():
            ctx = [self.start] + (self.enc(" ".join(prev)) if prev else [])
            sep = " " if prev else ""
            ids = [self.enc(sep + s) for s in strings]
            out = self.model(input_ids=torch.tensor([ctx], device=self.dev), use_cache=True)
            first = torch.log_softmax(out.logits[0, -1].float(), -1)
            b, n = len(ids), max(map(len, ids))
            x = torch.zeros((b, n), dtype=torch.long)
            for i, t in enumerate(ids):
                x[i, :len(t)] = torch.tensor(t)
            mask = (torch.arange(n)[None, :] < torch.tensor([len(t) for t in ids])[:, None]).to(self.dev)
            x = x.to(self.dev)
            lp = first[x[:, 0]]
            if n > 1:  # right padding: real tokens never see the pads after them
                cache = out.past_key_values
                cache.batch_repeat_interleave(b)
                logits = self.model(input_ids=x[:, :-1], past_key_values=cache, use_cache=False).logits.float()
                tl = logits.gather(-1, x[:, 1:, None])[..., 0] - torch.logsumexp(logits, -1)
                lp = lp + (tl * mask[:, 1:]).sum(1)
            return (lp / LN10).tolist()

    def ctx_ids(self, prev):
        return [self.start] + (self.enc(" ".join(prev)) if prev else [])

    def logprobs_group(self, jobs):
        """logprobs() for several (prev, strings) whose contexts have the same token
        length (so nothing needs padding but the strings), for throughput: one pass
        for the contexts, one for every string on its context's KV cache."""
        torch = self.torch
        ctxs = [self.ctx_ids(prev) for prev, _ in jobs]
        assert len({len(c) for c in ctxs}) == 1
        rep, ids = [], []
        for k, (prev, strings) in enumerate(jobs):
            for s in strings:
                rep.append(k)
                ids.append(self.enc((" " if prev else "") + s))
        with torch.no_grad():
            out = self.model(input_ids=torch.tensor(ctxs, device=self.dev), use_cache=True)
            first = torch.log_softmax(out.logits[:, -1].float(), -1)
            b, n = len(ids), max(map(len, ids))
            x = torch.zeros((b, n), dtype=torch.long)
            for i, t in enumerate(ids):
                x[i, :len(t)] = torch.tensor(t)
            mask = (torch.arange(n)[None, :] < torch.tensor([len(t) for t in ids])[:, None]).to(self.dev)
            x, repi = x.to(self.dev), torch.tensor(rep, device=self.dev)
            lp = first[repi, x[:, 0]]
            if n > 1:
                cache = out.past_key_values
                cache.batch_select_indices(repi)
                logits = self.model(input_ids=x[:, :-1], past_key_values=cache, use_cache=False).logits.float()
                tl = logits.gather(-1, x[:, 1:, None])[..., 0] - torch.logsumexp(logits, -1)
                lp = lp + (tl * mask[:, 1:]).sum(1)
            flat = (lp / LN10).tolist()
        res, k = [], 0
        for _, strings in jobs:
            res.append(flat[k:k + len(strings)])
            k += len(strings)
        return res

    def logprobs_plain(self, prev, strings):
        """The same without the cache, one full sequence each (to check logprobs)."""
        torch = self.torch
        res = []
        with torch.no_grad():
            ctx = [self.start] + (self.enc(" ".join(prev)) if prev else [])
            for s in strings:
                t = self.enc((" " if prev else "") + s)
                x = torch.tensor([ctx + t], device=self.dev)
                lp = torch.log_softmax(self.model(input_ids=x).logits[0].float(), -1)
                res.append(sum(lp[len(ctx) - 1 + j, t[j]].item() for j in range(len(t))) / LN10)
        return res


def item_words(rec):
    return [rec["t0"]] + [rec["words"][j] for j in rec["top"]]


def best_forms(fs, lp):
    out, k = [], 0
    for group in fs:
        out.append(max(lp[k:k + len(group)]))
        k += len(group)
    return out


def score_rec(scorer, rec, unss):
    """log10 P_LM per word of item_words(rec): the best of its written forms."""
    fs = [forms(w, unss) for w in item_words(rec)]
    return best_forms(fs, scorer.logprobs(rec["prev"], [f for group in fs for f in group]))


def score_recs(scorer, recs, unss):
    """score_rec() for several items with the same context length in one batch (throughput only)."""
    fss = [[forms(w, unss) for w in item_words(r)] for r in recs]
    lps = scorer.logprobs_group([(r["prev"], [f for g in fs for f in g]) for r, fs in zip(recs, fss)])
    return [best_forms(fs, lp) for fs, lp in zip(fss, lps)]


def groups(scorer, recs, idx, size):
    """idx in chunks of at most `size` items whose contexts have the same token length."""
    by = collections.defaultdict(list)
    for i in idx:
        by[len(scorer.ctx_ids(recs[i]["prev"]))].append(i)
    return [g[k:k + size] for g in by.values() for k in range(0, len(g), size)]


def score(model, device="mps", cap=0, size=16):
    scorer = Scorer(model, device)
    unss = unfold_map()
    # check the cached and the grouped batches against plain full sequences once
    recs = load_prep("robin-typos")
    el = [i for i, r in enumerate(recs) if r["elig"]]
    check = [g for g in groups(scorer, recs, el[:200], 4) if len(g) > 1][:2]
    check += [[i for i in el if not recs[i]["prev"]][:3]]
    for g in check:
        for i, c in zip(g, score_recs(scorer, [recs[i] for i in g], unss)):
            r = recs[i]
            fs = [forms(w, unss) for w in item_words(r)]
            flat = [f for x in fs for f in x]
            a, b = scorer.logprobs(r["prev"], flat), scorer.logprobs_plain(r["prev"], flat)
            d = max(abs(x - y) for x, y in zip(a, b))
            e = max(abs(x - y) for x, y in zip(c, best_forms(fs, b)))
            print(f"check {r['typed']}: max |cache - plain| = {d:.1e}, |group - plain| = {e:.1e}", flush=True)
    for name in SETS:
        path = f"{OUT}/{name}.{slug(model)}.json"
        if os.path.exists(path):
            print(f"{name}: done before", flush=True)
            continue
        recs = load_prep(name)
        idx = [i for i in sample(recs, cap) if recs[i]["elig"]]
        res = {}
        t = time.time()
        for n, part in enumerate(groups(scorer, recs, idx, size)):
            for i, s in zip(part, score_recs(scorer, [recs[i] for i in part], unss)):
                res[i] = s
            if n % 100 == 0:
                print(f"  {name} {len(res)}/{len(idx)} {time.time() - t:.0f}s", flush=True)
        json.dump({"model": model, "device": device, "params": scorer.params, "scores": res}, open(path, "w"))
        print(f"{name}: {len(idx)} items, {time.time() - t:.0f}s", flush=True)


def time_only(model, device, n=300):
    """Per-item latency, one item at a time as the keyboard would: KV-cached context + one batch."""
    scorer = Scorer(model, device)
    unss = unfold_map()
    recs = [r for r in load_prep("robin-typos") if r["elig"]][:n]
    for r in recs[:10]:  # warm up
        score_rec(scorer, r, unss)
    ms = []
    for r in recs:
        t0 = time.perf_counter()
        score_rec(scorer, r, unss)
        ms.append(1000 * (time.perf_counter() - t0))
    nf = statistics.mean(sum(len(forms(w, unss)) for w in item_words(r)) for r in recs)
    res = {"model": model, "device": device, "params": scorer.params, "n": len(ms), "median_ms": statistics.median(ms),
           "p95_ms": float(np.percentile(ms, 95)), "strings_per_item": nf,
           "torch_threads": scorer.torch.get_num_threads()}
    json.dump(res, open(f"{OUT}/latency.{slug(model)}.{device}.json", "w"))
    print(json.dumps(res))


# --- decisions -----------------------------------------------------------------------

def decide_arrays(lmv, ch, words, s0, ch0, lam, tau):
    """decide() on arrays: candidates' LM scores and channels, the typed word's LM score."""
    if len(lmv) == 0:
        return None
    s = lmv + lam * ch
    j = int(np.argmax(s))
    top, t = s[j], s0 + lam * ch0
    if t >= top:
        return None
    share = 1.0 / (10 ** (t - top) + np.sum(10 ** (s - top)))
    return words[j] if share >= tau else None


def ngram_pick(order=3, lam=1.0, p_oov=-10.0, tau=0.95, top_only=False):
    def pick(r):
        if not r["elig"]:
            return None
        s0 = p_oov if r["t0_lm"][order - 1] is None else r["t0_lm"][order - 1]
        lmv, ch, words = r["lm"][:, order - 2], r["ch"], r["words"]
        if top_only:
            j = r["top"]
            lmv, ch, words = lmv[j], ch[j], [words[k] for k in j]
        return decide_arrays(lmv, ch, words, s0, r["ch0"], lam, tau)
    return pick


def neural_pick(key, lam, tau, p_oov=None, mix=False):
    """p_oov None: the typed string gets the LM's own score (way b); else P_OOV when
    the typed word is no word (way a). mix: average of trigram and neural log10."""
    def pick(r):
        if not r["elig"]:
            return None
        nn = r[key]
        j = r["top"]
        lmv = nn[1:]
        if mix:
            lmv = (lmv + r["lm"][j, 1]) / 2
        tl = r["t0_lm"][2]
        if p_oov is not None and tl is None:
            s0 = p_oov
        else:
            s0 = (nn[0] + tl) / 2 if mix and tl is not None else nn[0]
        return decide_arrays(lmv, r["ch"][j], [r["words"][k] for k in j], s0, r["ch0"], lam, tau)
    return pick


def evaluate(sets, pick):
    res = {}
    for name, recs in sets.items():
        n, _ = sc.tally(recs, pick)
        typos = n["right"] + n["wrong"] + n["missed"]
        words = n["kept"] + n["changed"]
        res[name] = {"right": 100 * n["right"] / typos if typos else None,
                     "wrong": 100 * n["wrong"] / typos if typos else None,
                     "chg10k": 1e4 * n["changed"] / words if words else None,
                     "changed": n["changed"], "n": n}
    res["robin-words"]["items"] = len(changes(sets["robin-words"], pick))
    return res


def objective(res):
    """Mean over the three typo sets of right - wrong (percent points)."""
    names = ("typing37k", "136m", "robin-typos")
    return sum(res[k]["right"] - res[k]["wrong"] for k in names) / len(names)


def feasible(res):
    return all(res[k]["chg10k"] <= MAX_CHANGED + 1e-9 for k in ("typing37k", "136m"))


def row(label, res):
    a, b, c, d = (res[k] for k in ("typing37k", "136m", "robin-typos", "robin-words"))
    return (f"{label:52} {a['right']:5.1f} {a['wrong']:4.1f} {a['chg10k']:4.1f} | {b['right']:5.1f} {b['wrong']:4.1f} "
            f"{b['chg10k']:4.1f} | {c['right']:5.1f} {c['wrong']:4.1f} | {d['items']:6d} | {objective(res):5.1f}")


HEADER = (f"{'':52} {'typing37k':^16} | {'136m':^16} | {'robin-typos':^10} | {'robin-':^6} | {'net':^5}\n"
          f"{'variant (settings)':52} {'right':>5} {'wrng':>4} {'chg':>4} | {'right':>5} {'wrng':>4} {'chg':>4} | "
          f"{'right':>5} {'wrng':>4} | {'words':>6} | {'':>5}")


def sweep(sets, make, configs):
    """Every config of one family; the best feasible one by objective()."""
    rows = []
    for cfg in configs:
        res = evaluate(sets, make(*cfg))
        rows.append((feasible(res), objective(res), cfg, res))
    ok = [r for r in rows if r[0]]
    best = max(ok, key=lambda r: r[1]) if ok else None
    return best, rows


def changes(recs, pick):
    out = []
    for r in recs:
        p = pick(r)
        if p and sc.tm.fold(p) != sc.tm.fold(r["typed"]):
            out.append(f"{' '.join(r['prev'][-4:])} [{r['typed']} -> {p}] {r['next']}")
    return out


def report(cap=0):
    sets, idx = {}, {}
    for name in SETS:
        recs = load_prep(name)
        idx[name] = sample(recs, cap)
        sets[name] = [recs[i] for i in idx[name]]
    models = []
    lat = {}
    for m in MODELS:
        key = "nn:" + slug(m)
        got = {}
        for name in SETS:
            path = f"{OUT}/{name}.{slug(m)}.json"
            if os.path.exists(path):
                got[name] = json.load(open(path))
        if len(got) < len(SETS):
            print(f"# {m}: scores missing for {sorted(set(SETS) - set(got))}, skipped")
            continue
        missing = 0
        for name in SETS:
            sc_ = got[name]["scores"]
            for i, r in zip(idx[name], sets[name]):
                if r["elig"]:
                    if str(i) in sc_:
                        r[key] = np.array(sc_[str(i)])
                    else:
                        missing += 1
        if missing:
            print(f"# {m}: {missing} sampled items have no LM score (scored with a smaller CAP?), skipped")
            continue
        models.append(m)
        for dev in ("mps", "cpu"):
            p = f"{OUT}/latency.{slug(m)}.{dev}.json"
            if os.path.exists(p):
                lat[(m, dev)] = json.load(open(p))

    print(f"Small neural LM vs word trigram as the context scorer, {time.strftime('%Y-%m-%d')}")
    print(f"Sample: {'all items' if not cap else f'first {cap} typos + {cap} correct words per set'}; "
          + ", ".join(f"{k} {len(v)} items ({sum(r['elig'] for r in v)} decide() may change)" for k, v in sets.items()))
    bad = sum(ngram_pick()(r) != r["base"] for v in sets.values() for r in v if r["elig"])
    print(f"Check: decide_arrays vs score_context.decide at the baseline settings: {bad} differences")
    ceil = {k: (sum(r["w"] for r in v if r["typo"] and r["elig"] and sc.tm.fold(r["meant"]) in r["words"]),
                sum(r["w"] for r in v if r["typo"] and r["elig"] and sc.tm.fold(r["meant"]) in [r["words"][j] for j in r["top"]]),
                sum(r["w"] for r in v if r["typo"]))
            for k, v in sets.items() if k != "robin-words"}
    print("Ceiling (typos whose meant word is a candidate at all / among the top 10 the LM sees): "
          + ", ".join(f"{k} {100 * a / t:.1f}% / {100 * b / t:.1f}%" for k, (a, b, t) in ceil.items()))
    print(f"Selection: best mean (right - wrong) over the three typo sets, with typing37k and 136m at most "
          f"{MAX_CHANGED} changed per 10k correct words.\nGrid: LAMBDA {LAMS}, P_OOV {OOVS} (way a), TAU {TAUS}.")
    print("chg = correct words changed per 10k; robin-words = correct words changed (items); "
          "net = mean (right - wrong) over the three typo sets.\n")

    full = [(o, l, p, t) for o in (3,) for l in LAMS for p in OOVS for t in TAUS]
    a_cfg = [(l, p, t) for l in LAMS for p in OOVS for t in TAUS]
    b_cfg = [(l, t) for l in LAMS for t in TAUS]
    lines, details, best_picks = [HEADER], [], []

    def add(label, pick):
        res = evaluate(sets, pick)
        lines.append(row(label, res))
        best_picks.append((label, pick))
        return res

    add("n-gram trigram, all candidates (l1 oov-10 t.95)", ngram_pick(3, 1.0, -10, 0.95))
    add("n-gram bigram, all candidates (l1 oov-10 t.95)", ngram_pick(2, 1.0, -10, 0.95))
    add("n-gram trigram, top-10 candidates (l1 oov-10 t.95)", ngram_pick(3, 1.0, -10, 0.95, True))
    for m in models:
        add(f"{slug(m)} (a), same settings (l1 oov-10 t.95)", neural_pick("nn:" + slug(m), 1.0, 0.95, -10.0))
    families = [("n-gram trigram, all candidates", lambda o, l, p, t: ngram_pick(o, l, p, t), full,
                 lambda c: f"l{c[1]} oov{c[2]:.0f} t{c[3]}"),
                ("n-gram trigram, top-10 candidates", lambda o, l, p, t: ngram_pick(o, l, p, t, True), full,
                 lambda c: f"l{c[1]} oov{c[2]:.0f} t{c[3]}")]
    for m in models:
        key = "nn:" + slug(m)
        families += [
            (f"{slug(m)} (a) P_OOV", lambda l, p, t, key=key: neural_pick(key, l, t, p), a_cfg,
             lambda c: f"l{c[0]} oov{c[1]:.0f} t{c[2]}"),
            (f"{slug(m)} (b) LM scores typed", lambda l, t, key=key: neural_pick(key, l, t), b_cfg,
             lambda c: f"l{c[0]} t{c[1]}"),
            (f"{slug(m)} + trigram avg (a)", lambda l, p, t, key=key: neural_pick(key, l, t, p, True), a_cfg,
             lambda c: f"l{c[0]} oov{c[1]:.0f} t{c[2]}"),
        ]
    lines.append("best feasible settings per variant:")
    for label, make, cfgs, fmt in families:
        best, rows = sweep(sets, make, cfgs)
        if best:
            lines.append(row(f"{label} ({fmt(best[2])})", best[3]))
            best_picks.append((f"{label} ({fmt(best[2])})", make(*best[2])))
        else:
            lines.append(f"{label}: no setting keeps typing37k and 136m at <= {MAX_CHANGED} per 10k")
        rows.sort(key=lambda r: -r[1])
        details.append(f"\n{label}: top settings by net (* = feasible)")
        details += [("* " if f else "  ") + row(fmt(c), res) for f, _, c, res in rows[:8]]
    print("\n".join(lines))

    if lat:
        print("\nLatency on this Mac (M1 Pro, fp32), one item at a time: context forward (KV cache) + one batch "
              "of all candidate forms (typed word + top 10, ~20 strings):")
        for (m, dev), x in lat.items():
            print(f"  {slug(m):16} {x['params'] / 1e6:4.0f}M params  {dev:3}  median {x['median_ms']:6.1f} ms  "
                  f"p95 {x['p95_ms']:6.1f} ms  ({x['n']} robin-typos items, {x['strings_per_item']:.1f} strings each)")

    langs = collections.defaultdict(list)
    for r in sets["robin-typos"]:
        lang = sc.tm.sentence_language(sc.TABLE, r["prev"] + [r["meant"], r["next"]])
        langs[{"d": "German", "e": "English"}.get(lang, "unclear")].append(r)
    print("\nrobin-typos by sentence language (sentence_language() of the context, the meant word and the "
          "next word): right / wrong %")
    print(f"{'':58}" + "".join(f"{k + f' ({len(v)})':>20}" for k, v in sorted(langs.items())))
    for label, pick in best_picks:
        cells = []
        for _, v in sorted(langs.items()):
            n, _ = sc.tally(v, pick)
            t = n["right"] + n["wrong"] + n["missed"]
            cells.append(f"{100 * n['right'] / t:5.1f} / {100 * n['wrong'] / t:4.1f}")
        print(f"{label:58}" + "".join(f"{c:>20}" for c in cells))

    print("\nrobin-words: every change, per variant (some are the user's own typos, so a change can be a right fix)")
    print("(the first list in full; the others as + added / - dropped against it)")
    rw = sets["robin-words"]
    base = None
    for label, pick in best_picks:
        ch = changes(rw, pick)
        print(f"\n{label}: {len(ch)} items changed")
        if base is None:
            base = ch
            print("\n".join("  " + c for c in ch))
        else:
            print("\n".join([f"  + {c}" for c in ch if c not in base] + [f"  - {c}" for c in base if c not in ch]))

    print("\n--- sweeps ---")
    print(HEADER)
    print("\n".join(details))


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "prep":
        prep()
    elif cmd == "score":
        score(sys.argv[2], *(sys.argv[3:4] or ["mps"]), cap=int(sys.argv[4]) if len(sys.argv) > 4 else 0)
    elif cmd == "time":
        time_only(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "mps")
    elif cmd == "report":
        report(int(sys.argv[2]) if len(sys.argv) > 2 else 0)
