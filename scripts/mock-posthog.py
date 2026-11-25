# Stands in for PostHog on 127.0.0.1:8799 (scripts/test-usage.sh): writes every request to OUT/<n>-..., answers like PostHog.
import http.server, json, os, sys, itertools
OUT = sys.argv[1]; n = itertools.count(len(os.listdir(OUT)))
class H(http.server.BaseHTTPRequestHandler):
    def _done(self, body=b"{}"):
        self.send_response(200); self.send_header("Content-Type","application/json"); self.end_headers(); self.wfile.write(body)
    def do_POST(self):
        data = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        open(os.path.join(OUT, f"{next(n):03d}-post{self.path.replace('/','_')}.json"), "wb").write(data); self._done()
    def do_GET(self):
        open(os.path.join(OUT, f"{next(n):03d}-get{self.path.replace('/','_')[:60]}.txt"), "w").write(self.path)
        self._done(json.dumps({"results":[{"id":"uuid-1"}]}).encode())
    def do_DELETE(self):
        open(os.path.join(OUT, f"{next(n):03d}-delete.txt"), "w").write(self.path + " " + self.headers.get("Authorization","")[:12]); self._done()
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", 8799), H).serve_forever()
