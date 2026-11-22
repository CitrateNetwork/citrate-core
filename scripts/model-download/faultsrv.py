#!/usr/bin/env python3
"""Fault-injecting model server for the scripted model-download verification.

It imitates the real chain: an ORIGIN (`/model`) that answers 302 to a SIGNED CDN URL
(`/cdn/blob?Expires=<unix>&Signature=<hex>`) which answers 403 once it has expired, exactly as
the Hugging Face Xet CDN does. The CDN serves a local file with HTTP Range support.

Behaviour is read from a JSON control file on EVERY request, so a running download can be
switched between faults without restarting the server:

  {"expiry_s": 3600,       signed-URL lifetime
   "rate": 0,              bytes/second per response (0 = unthrottled)
   "down": false,          close every connection at once (link down)
   "drop_after": 0,        close the connection after N body bytes (0 = never)
   "stall_after": 0,       stop sending (hold the socket open) after N body bytes (0 = never)
   "ignore_range": false,  answer 200 with the whole file, ignoring Range
   "wrong_bytes": false,   serve the right length but wrong bytes after the first 4
   "html": false,          the origin answers 200 with a web page instead of the model
   "cut_at_expiry": false} cut an in-flight CDN response once its signed URL expires

Usage: faultsrv.py <file> <port> <control.json> <request-log>
"""
import hashlib
import hmac
import json
import os
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

FILE, PORT, CTL, LOG = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
SIZE = os.path.getsize(FILE)
KEY = b"faultsrv-signing-key"


def ctl():
    try:
        with open(CTL) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def log(line):
    with open(LOG, "a") as f:
        f.write(f"{time.time():.1f} {line}\n")


def sign(exp):
    return hmac.new(KEY, str(exp).encode(), hashlib.sha256).hexdigest()


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_GET(self):
        c = ctl()
        u = urlparse(self.path)
        rng = self.headers.get("Range", "")
        if c.get("down"):
            log(f"DOWN {u.path} {rng}")
            self.close_connection = True
            try:
                self.connection.shutdown(2)
            except OSError:
                pass
            return
        if u.path == "/model":
            if c.get("html"):
                body = b"<!DOCTYPE html><html><body>join the waitlist</body></html>"
                self.send_response(200)
                self.send_header("Content-Type", "text/html")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                log(f"ORIGIN html {rng}")
                return
            exp = int(time.time()) + int(c.get("expiry_s", 3600))
            loc = f"/cdn/blob?Expires={exp}&Signature={sign(exp)}"
            self.send_response(302)
            self.send_header("Location", loc)
            self.send_header("Content-Length", "0")
            self.end_headers()
            log(f"ORIGIN 302 exp={exp} {rng}")
            return
        if u.path != "/cdn/blob":
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        q = parse_qs(u.query)
        exp = int(q.get("Expires", ["0"])[0])
        sig = q.get("Signature", [""])[0]
        if not hmac.compare_digest(sig, sign(exp)) or time.time() > exp:
            body = b"<Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>"
            self.send_response(403)
            self.send_header("Content-Type", "application/xml")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            log(f"CDN 403 expired exp={exp} now={int(time.time())} {rng}")
            return
        start, end = 0, SIZE - 1
        status = 200
        m = re.match(r"bytes=(\d+)-(\d*)", rng)
        if m and not c.get("ignore_range"):
            start = int(m.group(1))
            if m.group(2):
                end = min(int(m.group(2)), SIZE - 1)
            if start >= SIZE:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{SIZE}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                log(f"CDN 416 {rng}")
                return
            status = 206
        n = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(n))
        self.send_header("Accept-Ranges", "bytes")
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{SIZE}")
        self.end_headers()
        log(f"CDN {status} {rng} -> {start}-{end}")
        rate = int(c.get("rate", 0))
        drop_after = int(c.get("drop_after", 0))
        stall_after = int(c.get("stall_after", 0))
        wrong = bool(c.get("wrong_bytes"))
        cut = bool(c.get("cut_at_expiry"))
        sent = 0
        t0 = time.time()
        with open(FILE, "rb") as f:
            f.seek(start)
            while sent < n:
                chunk = f.read(min(65536, n - sent))
                if not chunk:
                    break
                if wrong:
                    off = start + sent
                    chunk = bytes(b if off + i < 4 else (b ^ 0x5A) for i, b in enumerate(chunk))
                if drop_after and sent + len(chunk) > drop_after:
                    chunk = chunk[: max(0, drop_after - sent)]
                    try:
                        self.wfile.write(chunk)
                    except OSError:
                        return
                    log(f"CDN drop after {drop_after} bytes")
                    self.close_connection = True
                    self.connection.shutdown(2)
                    return
                if cut and time.time() > exp:
                    log(f"CDN cut at expiry after {sent} bytes")
                    self.close_connection = True
                    self.connection.shutdown(2)
                    return
                if stall_after and sent + len(chunk) > stall_after:
                    log(f"CDN stall after {stall_after} bytes")
                    time.sleep(10_000)
                    return
                try:
                    self.wfile.write(chunk)
                except OSError:
                    log(f"CDN client went away at {sent}")
                    return
                sent += len(chunk)
                if rate:
                    ahead = sent / rate - (time.time() - t0)
                    if ahead > 0:
                        time.sleep(ahead)


ThreadingHTTPServer.daemon_threads = True
ThreadingHTTPServer(("127.0.0.1", PORT), H).serve_forever()
