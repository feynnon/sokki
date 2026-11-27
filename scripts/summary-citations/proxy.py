"""Capture proxy: forwards POSTs to llama-server and saves request+response (private: holds transcripts)."""
import http.server, json, sys, urllib.request, os, time
UP, PORT, OUT = sys.argv[1], int(sys.argv[2]), sys.argv[3]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers['Content-Length']))
        t0 = time.time()
        req = urllib.request.Request(UP + self.path, data=body, headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(req, timeout=900) as r:
            resp = r.read(); code = r.status
        name = os.environ.get('CAPTURE_NAME') or open(OUT + '/.name').read().strip()
        json.dump({'path': self.path, 'request': json.loads(body), 'response': json.loads(resp), 'secs': time.time() - t0},
                  open(f'{OUT}/{name}.json', 'w'), ensure_ascii=False)
        self.send_response(code); self.send_header('Content-Type', 'application/json'); self.end_headers(); self.wfile.write(resp)
    def log_message(self, *a): pass
http.server.ThreadingHTTPServer(('127.0.0.1', PORT), H).serve_forever()
