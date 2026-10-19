#!/usr/bin/env python3
"""Serve the production dist like Cloudflare Workers static assets.

Only headers declared in dist/_headers are added, so a missing COOP/COEP rule
fails the production suite instead of being hidden by a development server.
"""
import argparse
import fnmatch
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "web-instrument" / "dist"


def parse_headers(path: Path) -> list[tuple[str, list[tuple[str, str]]]]:
    rules: list[tuple[str, list[tuple[str, str]]]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if line[0].isspace():
            name, value = line.strip().split(":", 1)
            rules[-1][1].append((name.strip(), value.strip()))
        else:
            rules.append((line.strip(), []))
    return rules


RULES = parse_headers(ROOT / "_headers")


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm"}

    def send_head(self):
        # Workers parses _headers as configuration and never serves it.
        if self.path.split("?", 1)[0] == "/_headers":
            self.send_error(404)
            return None
        return super().send_head()

    def end_headers(self):
        request_path = self.path.split("?", 1)[0]
        for pattern, headers in RULES:
            if fnmatch.fnmatchcase(request_path, pattern):
                for name, value in headers:
                    self.send_header(name, value)
        super().end_headers()

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=8031)
    args = parser.parse_args()
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()
