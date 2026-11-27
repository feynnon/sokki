import json, glob, os, collections
R = os.path.join(os.environ.get('SOKKI_EVALS', os.path.expanduser('~/sokki-evals')), 'runs', 'summary-citations'); J = f'{R}/judge'
key = json.load(open(f'{J}/key.json'))
def load(prefix):
    out = {}
    for f in glob.glob(f'{J}/{prefix}_*.json'):
        v = json.load(open(f)); n = str(v['packet'])
        for c in v['claims']: out[(n, c['claim'])] = c
    return out
V = load('verdict'); S = load('second')
sup = collections.defaultdict(collections.Counter); cit = collections.defaultdict(collections.Counter)
real = lambda n: key[n]['meeting'].startswith('meeting-')
for n, k in key.items():
    for c in k['claims']:
        v = V.get((n, c['claim']))
        if not v: continue
        grp = 'real' if real(n) else 'made-up'
        sup[(c['src'], grp)][v['supported']] += 1; sup[(c['src'], 'all')][v['supported']] += 1
        for L, src in c['sets'].items():
            r = v['sets'].get(L, 'none')
            # only claims the transcript supports (yes/partly) count for citation quality
            if v['supported'] != 'no':
                cit[(src, grp)][r] += 1; cit[(src, 'all')][r] += 1
pct = lambda c, k: f"{100*c[k]/max(1,sum(c.values())):.0f}%"
print('claims supported by transcript (yes/partly/no):')
for (s, g), c in sorted(sup.items()): print(f'  {s} {g}: n={sum(c.values())} yes {pct(c,"yes")} partly {pct(c,"partly")} no {pct(c,"no")}')
print('cited lines support the claim (full/partial/none), supported claims only:')
for (s, g), c in sorted(cit.items()): print(f'  {s} {g}: n={sum(c.values())} full {pct(c,"full")} partial {pct(c,"partial")} none {pct(c,"none")}')
# agreement
if S:
    a = b = tot = 0; sa = st = 0
    for kk, s in S.items():
        v = V.get(kk)
        if not v: continue
        st += 1; sa += v['supported'] == s['supported']
        for L in s['sets']:
            tot += 1; a += v['sets'].get(L) == s['sets'][L]
    print(f'agreement of two judges: supported {sa}/{st}, sets {a}/{tot}')
