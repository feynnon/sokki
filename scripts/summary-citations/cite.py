"""No-LLM citation: match each summary point to transcript lines by shared words (idf-weighted,
prefix match for inflections and compounds, numbers exact). Lines are '[mm:ss] Speaker: text'."""
import math, re
STOP = set('''der die das den dem des ein eine einen einem einer eines und oder aber auch noch nur schon sehr mal dann wenn weil dass ob wie was wer wo
ist sind war waren wird werden wurde hat haben hatte sein kann können soll sollen muss müssen will wollen mit mit von zu zum zur für auf aus bei
nach über unter vor im in an am als es er sie wir ihr ich du man sich nicht kein keine so ja nein also ähm äh hm mhm okay ok genau gut eben halt
the a an and or but also only very then if because that whether how what who where is are was were will would be been has have had can could should
must want with of to for on from at by after over under before in as it he she we they you i not no so yes yeah um uh like just really
this these those there here their them his her its our your my me us him all any some more most other such into than too'''.split())
TOK = re.compile(r"[0-9]+(?:[.,][0-9]+)*|[^\W\d_]+", re.U)

def toks(s):
    return [t for t in (w.lower() for w in TOK.findall(s)) if (t[0].isdigit() or len(t) >= 3) and t not in STOP]

def parse(line):
    m = re.match(r'^\[([0-9:]+)\] ([^:]+): (.*)$', line)
    return (m.group(2), m.group(3)) if m else ('', line)

def same(a, b):
    if a == b: return True
    if a[0].isdigit() or b[0].isdigit(): return False
    s, l = (a, b) if len(a) <= len(b) else (b, a)
    return len(s) >= 4 and (l.startswith(s) or l.endswith(s) or (len(s) >= 6 and s in l))

def match(summary, lines, k=3, ratio=0.6, floor=2.0):
    parsed = [parse(l) for l in lines]
    ltoks = [set(toks(t)) for _, t in parsed]
    n = len(lines); df = {}
    for ts in ltoks:
        for t in ts: df[t] = df.get(t, 0) + 1
    def idf(t):
        d = sum(c for u, c in df.items() if same(t, u)) if t not in df else df[t]
        return math.log(1 + n / (1 + d))
    def cites(text, owner=None):
        ct = set(toks(text))
        w = {t: idf(t) for t in ct}
        scores = []
        for i, ts in enumerate(ltoks):
            s = sum(w[t] for t in ct if any(same(t, u) for u in ts))
            if owner and parsed[i][0].strip().lower() == owner.strip().lower(): s += 0.5
            scores.append(s)
        best = max(scores) if scores else 0
        if best < floor: return []
        top = sorted(range(n), key=lambda i: -scores[i])[:k]
        return sorted(i + 1 for i in top if scores[i] >= ratio * best)
    get = lambda key: summary.get(key) or []
    return {'summary': [cites(s if isinstance(s, str) else s.get('text', '')) for s in get('summary')],
            'decisions': [cites(d if isinstance(d, str) else d.get('text', '')) for d in get('decisions')],
            'action_items': [cites(a.get('text', ''), a.get('owner')) for a in get('action_items')]}
