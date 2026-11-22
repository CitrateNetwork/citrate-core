#!/usr/bin/env python3
"""Throttling, fault-injecting HTTPS CONNECT proxy for the scripted model-download verification.

The app's HTTP client (ureq) honours HTTPS_PROXY, so pointing a download at this proxy runs the
REAL chain (citrate.ai -> Hugging Face -> the signed Xet CDN URL) through a slow or broken link.

A JSON control file is re-read about once a second, so faults can be switched mid-download:

  {"rate": 0,          downstream bytes/second shared by all tunnels (0 = unthrottled)
   "down": false,      refuse new tunnels and cut open ones (link down)
   "stall": false}     keep tunnels open but forward nothing (a stalled link)

Usage: throttleproxy.py <port> <control.json> <log>
"""
import json
import socket
import sys
import threading
import time

PORT, CTL, LOG = int(sys.argv[1]), sys.argv[2], sys.argv[3]
state = {"rate": 0, "down": False, "stall": False}
lock = threading.Lock()
tunnels = set()
bucket = {"t": time.time(), "tokens": 0.0}


def log(line):
    with open(LOG, "a") as f:
        f.write(f"{time.time():.1f} {line}\n")


def reload_loop():
    last = None
    while True:
        try:
            with open(CTL) as f:
                c = json.load(f)
        except (OSError, ValueError):
            c = {}
        with lock:
            state.update({"rate": int(c.get("rate", 0)), "down": bool(c.get("down")), "stall": bool(c.get("stall"))})
            cur = dict(state)
        if cur != last:
            log(f"CTL {cur}")
            last = cur
        if cur["down"]:
            for s in list(tunnels):
                try:
                    s.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
        time.sleep(1)


def take(n):
    """Block until `n` bytes may pass under the shared downstream rate."""
    while True:
        with lock:
            rate, stall, down = state["rate"], state["stall"], state["down"]
            if down:
                return False
            if not stall:
                if not rate:
                    return True
                now = time.time()
                bucket["tokens"] = min(rate, bucket["tokens"] + (now - bucket["t"]) * rate)
                bucket["t"] = now
                if bucket["tokens"] >= n:
                    bucket["tokens"] -= n
                    return True
        time.sleep(0.02)


def pump(src, dst, throttle):
    try:
        while True:
            data = src.recv(16384)
            if not data:
                break
            if throttle and not take(len(data)):
                break
            dst.sendall(data)
    except OSError:
        pass
    for s in (src, dst):
        try:
            s.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass


def handle(client):
    try:
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = client.recv(4096)
            if not chunk:
                client.close()
                return
            head += chunk
        line = head.split(b"\r\n", 1)[0].decode()
        method, target = line.split(" ")[:2]
        with lock:
            down = state["down"]
        if method != "CONNECT" or down:
            log(f"REFUSE {line} down={down}")
            client.close()
            return
        host, port = target.rsplit(":", 1)
        up = socket.create_connection((host, int(port)), timeout=30)
        up.settimeout(None)
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        log(f"TUNNEL {target}")
        tunnels.update({client, up})
        t = threading.Thread(target=pump, args=(client, up, False), daemon=True)
        t.start()
        pump(up, client, True)
        t.join(1)
        tunnels.discard(client)
        tunnels.discard(up)
    except Exception as e:  # noqa: BLE001 (log and drop the tunnel)
        log(f"ERR {e}")
        try:
            client.close()
        except OSError:
            pass


threading.Thread(target=reload_loop, daemon=True).start()
srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", PORT))
srv.listen(64)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
