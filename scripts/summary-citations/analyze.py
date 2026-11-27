"""Numbers from out/*.json: content (Q1), citations shape, latency (Q3). Prints numbers only."""
import glob, json, os, re, statistics as st, sys
B = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(os.environ.get('SOKKI_EVALS', os.path.expanduser('~/sokki-evals')), 'runs', 'summary-citations')
sys.path.insert(0, B); import cite, bench
SECS = ['summary', 'decisions', 'action_items']

def items_a(a):  # [(section, text, owner)]
    out = []
    for s in SECS:
        for x in a.get(s) or []:
            if isinstance(x, str): out.append((s, x, None))
            else: out.append((s, x.get('text', ''), x.get('owner')))
    return out

def stamp_of(line):
    m = re.match(r'^\[([0-9:]+)\]', line); return m.group(1) if m else None

def secs(st_): 
    p = [int(x) for x in st_.split(':')]; v = 0
    for x in p: v = v * 60 + x
    return v

def d_to_lines(dc, lines):
    stamps = [stamp_of(l) for l in lines]; bad = 0; out = {}
    for s in SECS:
        out[s] = []
        for lst in dc.get(s) or []:
            ids = []
            for t in lst:
                hit = [i + 1 for i, x in enumerate(stamps) if x and secs(x) == secs(t)]
                if not hit: bad += 1
                ids += hit
            out[s].append(sorted(set(ids)))
    return out, bad

def b_cites(b, n):
    out = {s: [] for s in SECS}; bad = 0
    for s in SECS:
        for x in b.get(s) or []:
            c = [i for i in x.get('cites', []) if 1 <= i <= n]; bad += len(x.get('cites', [])) - len(c)
            out[s].append(sorted(set(c)))
    return out, bad

def load(path):
    rec = json.load(open(path)); n = rec['lines']
    lines = json.load(open(f"{R}/capture/{rec['meeting']}.json"))['request']['messages'][1]['content'].split('\n\n', 1)[1].split('\n')
    a = bench.loads(rec['A']['reply']); b = bench.loads(rec['B']['reply'])
    try: dc = bench.loads(rec['D']['reply'])
    except Exception: dc = {}
    d, dbad = d_to_lines(dc, lines); bc, bbad = b_cites(b, n)
    return rec, lines, a, b, {'C': rec['C']['cites'], 'D': d, 'B': bc}, {'D': dbad, 'B': bbad}

def jac(x, y):
    x, y = set(cite.toks(x)), set(cite.toks(y))
    return len(x & y) / max(1, len(x | y))

def overlap(X, Y, th=0.25):  # share of X's items with a counterpart in Y
    if not X: return None
    return sum(1 for _, t, _ in X if Y and max(jac(t, u) for _, u, _ in Y) >= th) / len(X)

def main():
    files = sorted(glob.glob(f'{R}/out/*.r*.json')); per = {}
    for f in files:
        rec, lines, a, b, cites, bad = load(f)
        per.setdefault(rec['meeting'], {})[rec['run']] = dict(rec=rec, lines=lines, a=a, b=b, cites=cites, bad=bad)
    rows = []
    for m, runs in per.items():
        for r, x in runs.items():
            A, Bi = items_a(x['a']), items_a(x['b'])
            row = {'meeting': m, 'run': r, 'lines': len(x['lines']),
                   'A_items': [sum(1 for i in A if i[0] == s) for s in SECS], 'B_items': [sum(1 for i in Bi if i[0] == s) for s in SECS],
                   'A_chars': sum(len(t) for _, t, _ in A), 'B_chars': sum(len(t) for _, t, _ in Bi),
                   'B_in_A': overlap(Bi, A), 'A_in_B': overlap(A, Bi),
                   'A_s': x['rec']['A']['stats']['secs'], 'D_s': x['rec']['D']['stats']['secs'], 'B_s': x['rec']['B']['stats']['secs'], 'C_s': x['rec']['C']['secs'],
                   'A_out': x['rec']['A']['stats']['predicted_n'], 'B_out': x['rec']['B']['stats']['predicted_n'], 'D_out': x['rec']['D']['stats']['predicted_n'],
                   'A_prompt': x['rec']['A']['stats']['prompt_n'], 'D_prompt': x['rec']['D']['stats']['prompt_n'], 'D_cache': x['rec']['D']['stats']['cache_n'], 'B_prompt': x['rec']['B']['stats']['prompt_n'],
                   'bad_cites': x['bad']}
            for k, c in x['cites'].items():
                flat = [l for s in SECS for l in c.get(s, [])]
                row[f'{k}_cited_share'] = sum(1 for l in flat if l) / max(1, len(flat))
                row[f'{k}_cites_per_item'] = st.mean(len(l) for l in flat) if flat else 0
                ts = [i for l in flat for i in l]
                row[f'{k}_span_deciles'] = len({min(9, (i - 1) * 10 // max(1, len(x['lines']))) for i in ts})
            if 2 in runs and r == 1:
                row['A1_in_A2'] = overlap(A, items_a(runs[2]['a'])); row['B1_in_B2'] = overlap(Bi, items_a(runs[2]['b']))
            rows.append(row)
    json.dump(rows, open(f'{R}/analysis.json', 'w'), indent=1)
    def agg(k, f=st.median):
        v = [r[k] for r in rows if r.get(k) is not None]; return round(f(v), 3) if v else None
    keys = ['A_s', 'D_s', 'B_s', 'C_s', 'A_out', 'B_out', 'D_out', 'A_chars', 'B_chars', 'B_in_A', 'A_in_B', 'A1_in_A2', 'B1_in_B2',
            'C_cited_share', 'D_cited_share', 'B_cited_share', 'C_cites_per_item', 'D_cites_per_item', 'B_cites_per_item', 'C_span_deciles', 'D_span_deciles', 'B_span_deciles']
    print('n runs', len(rows)); [print(k, 'median', agg(k), 'mean', agg(k, st.mean)) for k in keys]
    for r in rows: print(r['meeting'][-12:], r['run'], r['lines'], 'A', r['A_items'], 'B', r['B_items'], 'secs A/D/B', r['A_s'], r['D_s'], r['B_s'], 'prompt', r['A_prompt'], r['D_prompt'], r['D_cache'], r['B_prompt'], 'bad', r['bad_cites'])
main() if __name__ == '__main__' else None
