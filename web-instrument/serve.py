#!/usr/bin/env python3
"""Local secure-context static server for the cranklink web instrument."""
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import argparse

class Handler(SimpleHTTPRequestHandler):
    target_dir = Path(__file__).resolve().parent / "dist"
    if not target_dir.is_dir():
        target_dir = Path(__file__).resolve().parent

    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(self.target_dir), **kwargs)

    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.send_header('Cross-Origin-Resource-Policy', 'same-origin')
        self.send_header('Content-Security-Policy', "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self' 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
        self.send_header('X-Content-Type-Options', 'nosniff')
        super().end_headers()

    extensions_map = {
        **SimpleHTTPRequestHandler.extensions_map,
        '.wasm': 'application/wasm',
        '.js': 'text/javascript',
        '.mjs': 'text/javascript',
        '.json': 'application/json',
    }

if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--port', type=int, default=8020)
    p.add_argument('--dir', type=str, default=None)
    a = p.parse_args()
    if a.dir:
        Handler.target_dir = Path(a.dir).resolve()
    print(f'Serving {Handler.target_dir} on http://localhost:{a.port}', flush=True)
    ThreadingHTTPServer(('127.0.0.1', a.port), Handler).serve_forever()
