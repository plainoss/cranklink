#!/usr/bin/env python3
"""Local secure-context static server for the WebUSB/WASM terminal."""
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import argparse

class Handler(SimpleHTTPRequestHandler):
    def __init__(self,*args,**kwargs):super().__init__(*args,directory=str(Path(__file__).resolve().parent),**kwargs)
    def end_headers(self):
        self.send_header('Cross-Origin-Opener-Policy','same-origin')
        self.send_header('Cross-Origin-Embedder-Policy','require-corp')
        self.send_header('Cross-Origin-Resource-Policy','same-origin')
        self.send_header('Content-Security-Policy',"default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
        self.send_header('X-Content-Type-Options','nosniff')
        super().end_headers()
    extensions_map={**SimpleHTTPRequestHandler.extensions_map,'.wasm':'application/wasm','.js':'text/javascript'}
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--port',type=int,default=8020);a=p.parse_args()
    print(f'Open http://localhost:{a.port} in Chrome/Edge',flush=True)
    ThreadingHTTPServer(('127.0.0.1',a.port),Handler).serve_forever()
