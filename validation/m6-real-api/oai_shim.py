#!/usr/bin/env python3
"""OpenAI-route shim in front of a real whisper.cpp `whisper-server`.

Purpose (M6 acceptance, F-041 "one recorded real-API run"): humm's
`OpenAiEngine` posts to `{base}/audio/transcriptions` (the OpenAI route).
whisper.cpp's stock `whisper-server` (the exact server class Buzz's
`is_whisper_cpp` response branch exists for) serves the same multipart
transcription contract at `POST /inference` — same `file` part, same
`response_format=verbose_json`, an OpenAI-shaped response body — but not
the OpenAI *path*. This shim rewrites the path and nothing else:

  POST /v1/audio/transcriptions  ->  POST {upstream}/inference   (body verbatim)
  GET  /v1/models                ->  answered locally, 200        (models.list
                                     for the Test-button validate flow; stock
                                     whisper-server has no such route)

Everything that matters — the multipart body, the model inference, the
verbose_json response humm parses — is produced by the real server. The
shim never inspects or modifies the audio or the response.

Usage:
  whisper-server -m <ggml model> --port 18081 &
  python3 oai_shim.py --listen 18080 --upstream http://127.0.0.1:18081 &
  HUMM_OPENAI_TEST_BASE=http://127.0.0.1:18080/v1 cargo test -p humm-cli \
      --test cli -- --ignored real_api

`--request-log <path>` (M7 Task 20) appends one `<epoch_ms> <METHOD> <path>
<bytes>` line per request. It exists so an acceptance run can *prove* which
OpenAI route a client actually hit — specifically that a live session with
task=Translate posts to `/audio/translations`, not `/audio/transcriptions`
(deviation ledger #L15's closure, Task 18). Nothing else about the shim's
behavior changes; the log never records body bytes, only the count.
"""

import argparse
import http.server
import json
import time
import urllib.error
import urllib.request


def make_handler(upstream: str, request_log):
    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):  # quiet
            pass

        def record(self, nbytes=0):
            if request_log is None:
                return
            request_log.write(
                f"{int(time.time() * 1000)} {self.command} {self.path} {nbytes}\n"
            )
            request_log.flush()

        def do_GET(self):
            self.record()
            if self.path.rstrip("/").endswith("/models"):
                body = json.dumps(
                    {"object": "list", "data": [{"id": "whisper-local", "object": "model"}]}
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            self.record(length)
            if not self.path.endswith("/audio/transcriptions") and not self.path.endswith(
                "/audio/translations"
            ):
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            body = self.rfile.read(length)
            req = urllib.request.Request(
                upstream.rstrip("/") + "/inference",
                data=body,
                headers={"Content-Type": self.headers.get("Content-Type", "")},
                method="POST",
            )
            try:
                with urllib.request.urlopen(req, timeout=600) as resp:
                    out = resp.read()
                    self.send_response(resp.status)
                    self.send_header(
                        "Content-Type", resp.headers.get("Content-Type", "application/json")
                    )
                    self.send_header("Content-Length", str(len(out)))
                    self.end_headers()
                    self.wfile.write(out)
            except urllib.error.HTTPError as e:
                out = e.read()
                self.send_response(e.code)
                self.send_header("Content-Length", str(len(out)))
                self.end_headers()
                self.wfile.write(out)

    return Handler


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--listen", type=int, default=18080)
    ap.add_argument("--upstream", default="http://127.0.0.1:18081")
    ap.add_argument(
        "--request-log",
        default=None,
        help="append '<epoch_ms> <METHOD> <path> <bytes>' per request to this file",
    )
    args = ap.parse_args()
    request_log = open(args.request_log, "a", encoding="utf-8") if args.request_log else None
    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", args.listen), make_handler(args.upstream, request_log)
    )
    print(f"oai_shim: 127.0.0.1:{args.listen} -> {args.upstream}/inference", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
