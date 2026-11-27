#!/usr/bin/env python3
"""A fake Slack Web API on localhost, for testing the huddle-thread post
without a workspace (scripts/test-meeting.sh SLACK_FAKE=1).

It answers the calls Sokki makes after a huddle and records each one:

- conversations.history: the thread's root message exists
- files.getUploadURLExternal: an upload URL on this server
- POST /upload/<id>: the transcript file
- files.completeUploadExternal: the post into the thread

  scripts/fake-slack.py PORT LOG.jsonl

Each request becomes one JSON line in LOG.jsonl: method, whether the
bearer token was there, the form or JSON body (the file as its length and
first line). Made-up data only; nothing leaves the machine.
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs

PORT = int(sys.argv[1])
LOG = sys.argv[2]
TOKEN = "xoxp-fake-test-token"


def record(entry):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, body):
        data = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        authed = self.headers.get("Authorization") == f"Bearer {TOKEN}"
        if self.path.startswith("/upload/"):
            text = raw.decode("utf-8", "replace")
            record({"method": "upload", "bytes": len(raw),
                    "first_line": text.splitlines()[0] if text else ""})
            self.send_response(200)
            self.send_header("Content-Length", "2")
            self.end_headers()
            self.wfile.write(b"OK")
            return
        method = self.path.rsplit("/", 1)[-1]
        if self.headers.get("Content-Type", "").startswith("application/json"):
            body = json.loads(raw or b"{}")
        else:
            body = {k: v[0] for k, v in parse_qs(raw.decode()).items()}
        record({"method": method, "authed": authed, "body": body})
        if not authed:
            self.reply({"ok": False, "error": "invalid_auth"})
        elif method == "conversations.history":
            ts = body.get("latest", "1764230000.000100")
            self.reply({"ok": True, "has_more": False,
                        "messages": [{"ts": ts, "subtype": "huddle_thread",
                                      "room": {"id": "R0FAKE"}}]})
        elif method == "files.getUploadURLExternal":
            self.reply({"ok": True, "file_id": "F0FAKE",
                        "upload_url": f"http://127.0.0.1:{PORT}/upload/F0FAKE"})
        elif method == "files.completeUploadExternal":
            self.reply({"ok": True, "files": [{"id": "F0FAKE"}]})
        elif method == "users.profile.get":
            self.reply({"ok": True, "profile": {"huddle_state": "default_unset"}})
        else:
            self.reply({"ok": True})


HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
