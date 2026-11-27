"""Round 2: B's claims with two sets: B's own cites and word matching (cite.py) on B's text, shuffled."""
import json, os, random, glob
import analyze, bench, cite
R = bench.R; J = f'{R}/judge2'; os.makedirs(J, exist_ok=True)
random.seed(11); key = {}
for n, f in enumerate(sorted(glob.glob(f'{R}/out/*.r2.json')), 1):
    rec, lines, a, b, cites, bad = analyze.load(f)
    cb = cite.match(b, lines)
    claims = []
    for s in analyze.SECS:
        for i, x in enumerate(b.get(s) or []):
            claims.append(dict(sec=s, text=x.get('text', ''), owner=x.get('owner'), sets={'B': cites['B'][s][i], 'CB': cb[s][i]}))
    random.shuffle(claims)
    out = [f'# Meeting {n}\n\nTranscript, one numbered line per turn:\n'] + [f'#{i} {l}' for i, l in enumerate(lines, 1)] + ['\n\n# Claims\n']
    kk = []
    for ci, c in enumerate(claims, 1):
        letters = list(c['sets']); random.shuffle(letters); lab = {'XY'[i]: s for i, s in enumerate(letters)}
        sec = {'summary': 'point', 'decisions': 'decision', 'action_items': 'task'}[c['sec']]
        who = f" (owner: {c['owner']})" if c['owner'] else ''
        out.append(f"## Claim {ci} [{sec}]{who}\n{c['text']}\n" + '\n'.join(f"- Set {L}: {', '.join('#'+str(i) for i in c['sets'][s]) or '(none)'}" for L, s in lab.items()) + '\n')
        kk.append(dict(claim=ci, sec=c['sec'], sets=lab, same=c['sets']['B'] == c['sets']['CB']))
    open(f'{J}/packet_{n}.md', 'w').write('\n'.join(out)); key[n] = dict(meeting=rec['meeting'], claims=kk)
json.dump(key, open(f'{J}/key.json', 'w'), indent=1)
print(sum(len(v['claims']) for v in key.values()), 'claims;', sum(c['same'] for v in key.values() for c in v['claims']), 'identical sets')
