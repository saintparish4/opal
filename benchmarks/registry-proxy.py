#!/usr/bin/env python3
"""A counting proxy in front of the npm registry.

    python3 benchmarks/registry-proxy.py [--port 4873] [--upstream https://registry.npmjs.org] [--log FILE]

Point a package manager at it (`opal --registry http://127.0.0.1:<port>`,
`npm --registry ...`) and every request it makes goes through here and is
counted: which path, packument or tarball, the status, and the bytes. Opal has
no request logging of its own, and adding it would be a dependency decision;
a proxy also counts npm's, pnpm's, and bun's requests the same way, so
"duplicate downloads" means the same thing for every tool.

Packuments name each tarball by its full URL on the registry, so a client
would fetch tarballs from there directly and never come back here. Each
packument's body has the upstream address replaced with the proxy's, so
tarball requests come through the proxy too. Bodies are fetched uncompressed
for that reason. The proxy passes ETag and If-None-Match through untouched;
the rewrite is the same every time, so a 304 still means what it says.

A lockfile written through the proxy records the proxy's address in its
tarball URLs. That is fine for a benchmark's scratch project and wrong for a
real one.

Imported by concurrent-installs.py; run on its own, it logs one line per
request and prints the totals on Ctrl-C.
"""

import argparse
import http.client
import sys
import threading
import time
import urllib.parse
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHUNK = 64 * 1024
PASSED_UP = ("Accept", "If-None-Match", "User-Agent")
PASSED_DOWN = ("Content-Type", "ETag", "Cache-Control", "Last-Modified")


class Proxy:
    """A proxy on a free localhost port, serving from a background thread."""

    def __init__(self, upstream="https://registry.npmjs.org", port=0, log=None):
        self.upstream = urllib.parse.urlsplit(upstream.rstrip("/"))
        self.log = log
        self.lock = threading.Lock()
        self.requests = []
        self.local = threading.local()
        proxy = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_GET(self):
                proxy.forward(self)

            def log_message(self, *args):
                pass

        class Server(ThreadingHTTPServer):
            # socketserver listens with a backlog of 5. Thirty-two installs
            # open hundreds of connections at once, and a connection the
            # backlog drops waits on TCP's retransmit, which looks like a
            # stalled registry: 4 of 32 installs took 30s instead of 5s.
            request_queue_size = 1024

        self.server = Server(("127.0.0.1", port), Handler)
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.server.shutdown()
        self.server.server_close()

    def reset(self):
        """Forget what has been counted, and return it."""
        with self.lock:
            counted, self.requests = self.requests, []
        return counted

    def connection(self):
        """One upstream connection per handler thread, kept alive between requests."""
        connection = getattr(self.local, "connection", None)
        if connection is None:
            kind = (http.client.HTTPSConnection if self.upstream.scheme == "https"
                    else http.client.HTTPConnection)
            connection = kind(self.upstream.netloc, timeout=60)
            self.local.connection = connection
        return connection

    def upstream_get(self, path, headers):
        for attempt in (1, 2):
            connection = self.connection()
            try:
                connection.request("GET", self.upstream.path + path, headers=headers)
                return connection.getresponse()
            except (http.client.HTTPException, OSError):
                # A kept-alive connection the server has since closed fails on
                # first use; one fresh connection settles whether it was that.
                connection.close()
                self.local.connection = None
                if attempt == 2:
                    raise

    def forward(self, handler):
        kind = "tarball" if "/-/" in handler.path else "packument"
        headers = {name: handler.headers[name] for name in PASSED_UP if handler.headers[name]}
        headers["Accept-Encoding"] = "identity"
        try:
            response = self.upstream_get(handler.path, headers)
        except (http.client.HTTPException, OSError) as error:
            handler.send_error(502, f"upstream: {error}")
            self.count(kind, handler.path, 502, 0)
            return

        handler.send_response(response.status)
        for name in PASSED_DOWN:
            if response.getheader(name):
                handler.send_header(name, response.getheader(name))
        sent = 0
        try:
            if response.status != 200:
                body = response.read()
                handler.send_header("Content-Length", str(len(body)))
                handler.end_headers()
                handler.wfile.write(body)
                sent = len(body)
            elif kind == "packument":
                sent = self.stream_rewritten(handler, response)
            else:
                sent = self.stream(handler, response)
        except (BrokenPipeError, ConnectionResetError):
            # The client gave up on this response; what it does next is its
            # own business, and the count records the request as cut short.
            handler.close_connection = True
            self.count(kind, handler.path, "cut", sent)
            return
        self.count(kind, handler.path, response.status, sent)

    def stream_rewritten(self, handler, response):
        """A packument, sent as it arrives with the upstream address replaced.

        Streamed rather than read whole first: a large packument through a
        busy link takes long enough that waiting for all of it before the
        first byte trips a client's time-to-first-byte limit. Its length
        changes with the rewrite, so the body ends when the connection does.
        """
        upstream = f"{self.upstream.scheme}://{self.upstream.netloc}{self.upstream.path}".encode()
        local = self.url.encode()
        handler.send_header("Connection", "close")
        handler.close_connection = True
        handler.end_headers()
        sent, carry = 0, b""
        while chunk := response.read(CHUNK):
            data = (carry + chunk).replace(upstream, local)
            # An address split across two reads is held back until the next
            # read completes it or shows it isn't one.
            carry = b""
            for length in range(min(len(upstream) - 1, len(data)), 0, -1):
                if data.endswith(upstream[:length]):
                    data, carry = data[:-length], data[-length:]
                    break
            handler.wfile.write(data)
            sent += len(data)
        handler.wfile.write(carry)
        return sent + len(carry)

    def stream(self, handler, response):
        """A tarball, passed through unchanged as it arrives."""
        length = response.getheader("Content-Length")
        if length:
            handler.send_header("Content-Length", length)
        else:
            handler.send_header("Connection", "close")
            handler.close_connection = True
        handler.end_headers()
        sent = 0
        while chunk := response.read(CHUNK):
            handler.wfile.write(chunk)
            sent += len(chunk)
        return sent

    def count(self, kind, path, status, sent):
        record = {"kind": kind, "path": path, "status": status, "bytes": sent,
                  "at": time.time()}
        with self.lock:
            self.requests.append(record)
        if self.log:
            print(f"{kind:9} {status} {sent:>10} {path}", file=self.log, flush=True)


def totals(requests):
    """Requests, distinct paths, and bytes, per kind."""
    summary = {}
    for kind in ("packument", "tarball"):
        mine = [r for r in requests if r["kind"] == kind]
        summary[kind] = {
            "requests": len(mine),
            "distinct": len({r["path"] for r in mine}),
            "bytes": sum(r["bytes"] for r in mine),
            "statuses": dict(Counter(str(r["status"]) for r in mine)),
        }
    return summary


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=4873)
    parser.add_argument("--upstream", default="https://registry.npmjs.org")
    parser.add_argument("--log", help="file to log each request to (default: stdout)")
    args = parser.parse_args()
    log = open(args.log, "a") if args.log else sys.stdout
    with Proxy(args.upstream, args.port, log) as proxy:
        print(f"registry proxy on {proxy.url}, forwarding to {args.upstream}", file=sys.stderr)
        try:
            while True:
                time.sleep(3600)
        except KeyboardInterrupt:
            pass
        for kind, numbers in totals(proxy.reset()).items():
            print(f"{kind}: {numbers['requests']} requests, {numbers['distinct']} distinct, "
                  f"{numbers['bytes'] / 2**20:.1f} MiB, {numbers['statuses']}", file=sys.stderr)


if __name__ == "__main__":
    main()
