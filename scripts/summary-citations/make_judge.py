"""Judge packets for run 2: every claim of A (with cite sets C, D under shuffled letters) and of B (own cites).
Writes judge/packet_<n>.md (private, holds transcript) and judge/key.json."""
import json, os, random, sys, glob
import analyze, bench
R = bench.R; J = f'{R}/judge'; os.makedirs(J, exist_ok=True)
random.seed(7); key = {}
files = sorted(glob.glob(f'{R}/out/*.r2.json'))
for n, f in enumerate(files, 1):
    rec, lines, a, b, cites, bad = analyze.load(f)
    claims = []
    for src, summ in (('A', a), ('B', b)):
        k = 0
        for s in analyze.SECS:
            for i, x in enumerate(summ.get(s) or []):
                text = x if isinstance(x, str) else x.get('text', '')
                owner = None if isinstance(x, str) else x.get('owner')
                sets = {'C': cites['C'][s][i], 'D': cites['D'][s][i]} if src == 'A' else {'B': cites['B'][s][i]}
                claims.append(dict(src=src, sec=s, text=text, owner=owner, sets=sets))
    random.shuffle(claims)
    out = [f'# Meeting {n}\n\nTranscript, one numbered line per turn:\n']
    out += [f'#{i} {l}' for i, l in enumerate(lines, 1)]
    out.append('\n\n# Claims\n')
    kk = []
    for ci, c in enumerate(claims, 1):
        letters = list(c['sets']); random.shuffle(letters)
        lab = {}
        for li, src in enumerate(letters): lab['XYZ'[li]] = src
        sec = {'summary': 'point', 'decisions': 'decision', 'action_items': 'task'}[c['sec']]
        who = f" (owner: {c['owner']})" if c['owner'] else ''
        out.append(f"## Claim {ci} [{sec}]{who}\n{c['text']}\n" + '\n'.join(f"- Set {L}: {', '.join('#'+str(i) for i in c['sets'][s]) or '(none)'}" for L, s in lab.items()) + '\n')
        kk.append(dict(claim=ci, src=c['src'], sec=c['sec'], sets=lab))
    open(f'{J}/packet_{n}.md', 'w').write('\n'.join(out))
    key[n] = dict(meeting=rec['meeting'], claims=kk)
json.dump(key, open(f'{J}/key.json', 'w'), indent=1)
print({n: (v['meeting'][-10:], len(v['claims'])) for n, v in key.items()})
