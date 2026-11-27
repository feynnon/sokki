"""Summary-citation bench (private outputs). Approaches per meeting and run:
A  today's request, byte for byte (captured from meeting-core via proxy.py)
D  A + a follow-up turn in the same chat: timestamps per point (prefix reused from the KV cache)
B  one request: numbered lines, every point an object with "cites"
C  no LLM: word matching of A's points against the transcript lines (cite.py)
usage: bench.py MEETINGS.tsv RUNS   (env PORT)"""
import json, os, re, sys, time, urllib.request
B = os.path.dirname(os.path.abspath(__file__))
R = os.path.join(os.environ.get('SOKKI_EVALS', os.path.expanduser('~/sokki-evals')), 'runs', 'summary-citations')
PORT = int(os.environ.get('PORT', '18131'))
sys.path.insert(0, B); import cite

def post(body):
    t0 = time.time()
    req = urllib.request.Request(f'http://127.0.0.1:{PORT}/v1/chat/completions', data=json.dumps(body).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=900) as r: resp = json.loads(r.read())
    return resp, time.time() - t0

def stats(resp, secs):
    t = resp.get('timings', {})
    return {'secs': round(secs, 2), 'prompt_n': t.get('prompt_n'), 'cache_n': t.get('cache_n'), 'prompt_ms': t.get('prompt_ms'),
            'predicted_n': t.get('predicted_n'), 'predicted_ms': t.get('predicted_ms'), 'draft_n': t.get('draft_n'), 'draft_accepted_n': t.get('draft_n_accepted')}

def content(resp): return resp['choices'][0]['message']['content']

def loads(s):
    s = s.strip()
    if s.startswith('```'): s = s.split('\n', 1)[1].rsplit('```', 1)[0]
    return json.loads(s[s.find('{'): s.rfind('}') + 1])

LINE = re.compile(r'^\[(\d+(?::\d+)+)\] ')
def number_lines(user):
    head, text = user.split('\n\n', 1)
    out = [f'#{i} {l}' for i, l in enumerate(text.split('\n'), 1)]
    return head + '\n\n' + '\n'.join(out)

SHAPE_A = '{"title": "...", "summary": ["...", "..."], "decisions": ["..."], "action_items": [{"owner": "..." or null, "text": "..."}]}'
SHAPE_B = '{"title": "...", "summary": [{"text": "...", "cites": [12, 13]}], "decisions": [{"text": "...", "cites": [40]}], "action_items": [{"owner": "..." or null, "text": "...", "cites": [41]}]}'
RULE_B = '- Every transcript line starts with its number, like #12. For every point, decision and task, list in "cites" the numbers of the 1 to 4 lines that state it most directly. Cite only lines that really say it.\n'

def make_b(body):
    b = json.loads(json.dumps(body))
    sys_m = b['messages'][0]['content']
    for old, new in [(SHAPE_A, SHAPE_B), ('bullet points (strings)', 'bullet points (objects with "text" and "cites")'),
                     ('"decisions": list of strings,', '"decisions": list of objects with "text" and "cites",'),
                     ('"text" is the task, including its deadline if one was stated.', '"text" is the task, including its deadline if one was stated; "cites" as above.')]:
        assert old in sys_m, old
        sys_m = sys_m.replace(old, new, 1)
    assert '\n\nAnswer with one JSON object' in sys_m
    sys_m = sys_m.replace('\n\nAnswer with one JSON object', '\n' + RULE_B + '\nAnswer with one JSON object', 1)
    b['messages'][0]['content'] = sys_m
    b['messages'][1]['content'] = number_lines(b['messages'][1]['content'])
    cites = {'type': 'array', 'items': {'type': 'integer', 'minimum': 1}, 'minItems': 1, 'maxItems': 4}
    item = lambda extra: {'type': 'object', 'properties': {**extra, 'text': {'type': 'string'}, 'cites': cites}, 'required': [*extra, 'text', 'cites'], 'additionalProperties': False}
    b['response_format'] = {'type': 'json_schema', 'json_schema': {'name': 'meeting_summary', 'schema': {
        'type': 'object', 'properties': {'title': {'type': 'string'}, 'summary': {'type': 'array', 'items': item({})},
        'decisions': {'type': 'array', 'items': item({})}, 'action_items': {'type': 'array', 'items': item({'owner': {'type': ['string', 'null']}})}},
        'required': ['title', 'summary', 'decisions', 'action_items'], 'additionalProperties': False}}}
    return b

FOLLOW = ('For each point, decision and task of your answer, in the same order, list the timestamps of the transcript lines that state it '
          '(the [mm:ss] at the start of each line, written like "12:34"): at most 4 per item, the lines that say it most directly. '
          'If no line states it, give an empty list. Answer with only one JSON object: {"summary": [["mm:ss", ...], ...], "decisions": [[...], ...], "action_items": [[...], ...]}.')

def make_d(body, a_reply, a):
    b = json.loads(json.dumps(body))
    b['messages'] += [{'role': 'assistant', 'content': a_reply}, {'role': 'user', 'content': FOLLOW}]
    stamp = {'type': 'string', 'pattern': '^[0-9]{1,2}:[0-9]{2}(:[0-9]{2})?$'}
    arr = lambda n: {'type': 'array', 'minItems': n, 'maxItems': n, 'items': {'type': 'array', 'items': stamp, 'minItems': 0, 'maxItems': 4}}
    b['response_format'] = {'type': 'json_schema', 'json_schema': {'name': 'cites', 'schema': {'type': 'object', 'properties': {
        'summary': arr(len(a.get('summary', []))), 'decisions': arr(len(a.get('decisions', []))), 'action_items': arr(len(a.get('action_items', [])))},
        'required': ['summary', 'decisions', 'action_items'], 'additionalProperties': False}}}
    return b

def main():
    rows = [l.split('\t') for l in open(sys.argv[1]).read().split('\n') if l.strip()]
    runs = int(sys.argv[2])
    res = open(f'{R}/results.jsonl', 'a')
    for run in range(1, runs + 1):
        for mid, _note in rows:
            path = f'{R}/out/{mid}.r{run}.json'
            if os.path.exists(path): continue
            if os.system(f'{B}/meetfree.sh') != 0: print('meeting started, stopping'); sys.exit(3)
            try:
                one(mid, run, path, res)
            except Exception as e:
                print('FAILED', mid, run, repr(e), flush=True)

def one(mid, run, path, res):
            cap = json.load(open(f'{R}/capture/{mid}.json'))
            body = cap['request']
            lines = body['messages'][1]['content'].split('\n\n', 1)[1].split('\n')
            rec = {'meeting': mid, 'run': run, 'lines': len(lines)}
            if run == 1:
                ra, sa = cap['response'], cap['secs']
            else:
                ra, sa = post(body)
            rec['A'] = {'stats': stats(ra, sa), 'reply': content(ra)}
            a = loads(content(ra))
            rd, sd = post(make_d(body, content(ra), a))
            rec['D'] = {'stats': stats(rd, sd), 'reply': content(rd)}
            t0 = time.time(); rec['C'] = {'cites': cite.match(a, lines), 'secs': round(time.time() - t0, 4)}
            rb, sb = post(make_b(body))
            rec['B'] = {'stats': stats(rb, sb), 'reply': content(rb)}
            json.dump(rec, open(path, 'w'), ensure_ascii=False, indent=1)
            print(json.dumps({'meeting': mid, 'run': run, 'lines': len(lines), 'A': rec['A']['stats'], 'D': rec['D']['stats'], 'B': rec['B']['stats'], 'C_s': rec['C']['secs']}), file=res, flush=True)
            print(mid, run, 'A', rec['A']['stats']['secs'], 'D', rec['D']['stats']['secs'], 'B', rec['B']['stats']['secs'], flush=True)
main() if __name__ == '__main__' else None
