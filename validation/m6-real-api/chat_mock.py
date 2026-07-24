#!/usr/bin/env python3
"""Deterministic OpenAI-compatible `/chat/completions` mock for M6 acceptance.

Used by the windowed F-063 live-translation acceptance run (gui-manual v4):
humm's real `TranslatorHandle` posts real HTTP here; the mock answers in
Buzz's numbered-batch shape so the parse path is exercised end to end.

Behavior: reads the LAST user message; if it contains `[N]` markers
(the batch form), replies `[i] FR{i}:<first 24 chars of item i>` per marker;
otherwise (single form) replies `FR:<first 24 chars>`. Marker parsing mirrors
Buzz's `re.split(r'\[(\d+)\]\s*', ...)` so item texts round-trip.

Usage: python3 chat_mock.py --listen 18090
"""

import argparse
import http.server
import json
import re


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(length) or b"{}")
        user = ""
        for msg in body.get("messages", []):
            if msg.get("role") == "user":
                user = msg.get("content", "")
        parts = re.split(r"\[(\d+)\]\s*", user)
        if len(parts) > 1:
            items = {}
            for i in range(1, len(parts) - 1, 2):
                items[int(parts[i])] = parts[i + 1].strip()
            content = "\n".join(f"[{n}] FR{n}:{t[:24]}" for n, t in sorted(items.items()))
        else:
            content = f"FR:{user[:24]}"
        out = json.dumps({"choices": [{"message": {"role": "assistant", "content": content}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", type=int, default=18090)
    args = ap.parse_args()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", args.listen), Handler)
    print(f"chat_mock: 127.0.0.1:{args.listen}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
