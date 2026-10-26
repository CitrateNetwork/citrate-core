#!/usr/bin/env python3
"""Test double for the citrate node sidecar, used only by scripts/e2e/cold-sync.test.mjs.

It takes the node's real command line (--network, --data-dir, --config), records what it was
started with, and serves a small JSON-RPC on the [rpc] listen_addr of the config it was given.
Its behaviour comes from the JSON file named by the first line of its own wrapper (the test writes
a wrapper script that sets STAND_IN_CFG and execs this file), because cold-sync.py starts the node
with a minimal environment and nothing else would reach it.

Config keys: record (path), chainId, genesis, seed, rate (blocks/s), headMax, forkAt, forkSeed,
missing ([from, to]: no block served in that range), exitAfter (s), exitOffGrid (exit right after
serving a block whose height is not a multiple of it: the harness reads one only in its final checks),
exitAfterProbe (after such an off-grid read, exit right after serving the next eth_blockNumber: the
harness's liveness probe), noRpc, peers, encMeta.
Block hash at height h > 0: 0x + sha256("<seed>:<h>"); block 0 is `genesis`.
"""
import hashlib
import json
import os
import re
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

cfg = json.loads(open(os.environ["STAND_IN_CFG"]).read())
argv = sys.argv[1:]


def arg(name):
    return argv[argv.index(name) + 1] if name in argv else None


data_dir = arg("--data-dir")
config_path = arg("--config")
config_text = open(config_path).read() if config_path and os.path.exists(config_path) else None
env = {k: v for k, v in os.environ.items() if k != "STAND_IN_CFG"}
key_file = env.get("CITRATE_STORAGE_KEY_FILE")
key_file_mode = None
key_len = None
if key_file and os.path.exists(key_file):
    key_file_mode = oct(os.stat(key_file).st_mode & 0o777)
    key_len = len(open(key_file).read().strip())
with open(cfg["record"], "w") as f:
    json.dump({"pid": os.getpid(), "argv": argv, "env": env, "config": config_text, "keyFileMode": key_file_mode,
               "keyLen": key_len, "cwd": os.getcwd()}, f)

if data_dir and cfg.get("encMeta", True):
    os.makedirs(data_dir, exist_ok=True)
    open(os.path.join(data_dir, "encryption.meta"), "w").write("stand-in\n")

if cfg.get("noRpc"):
    time.sleep(3600)

m = re.search(r"\[rpc\][^\[]*?listen_addr\s*=\s*\"([0-9.]+):(\d+)\"", config_text or "", re.S)
if not m:
    sys.exit("stand-in: no [rpc] listen_addr in the config")
host, port = m.group(1), int(m.group(2))
start = time.time()
exit_now = False
probe_armed = False


def head():
    h = int((time.time() - start) * cfg.get("rate", 500))
    if cfg.get("headMax") is not None:
        h = min(h, cfg["headMax"])
    return h


def block_hash(h):
    if h == 0:
        return cfg["genesis"]
    seed = cfg.get("seed", "main")
    if cfg.get("forkAt") is not None and h >= cfg["forkAt"]:
        seed = cfg.get("forkSeed", "fork")
    return "0x" + hashlib.sha256(f"{seed}:{h}".encode()).hexdigest()


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))))
        method, params = req.get("method"), req.get("params") or []
        global exit_now, probe_armed
        if method == "eth_chainId":
            result = hex(cfg["chainId"])
        elif method == "eth_blockNumber":
            result = hex(head())
            if probe_armed:
                exit_now = True
        elif method == "net_peerCount":
            result = hex(cfg.get("peers", 2))
        elif method == "eth_getBlockByNumber":
            tag = params[0]
            h = head() if tag == "latest" else int(tag, 16)
            gap = cfg.get("missing")
            if h > head() or (gap and gap[0] <= h <= gap[1]):
                result = None
            else:
                result = {"number": hex(h), "hash": block_hash(h)}
                if cfg.get("exitOffGrid") and h % cfg["exitOffGrid"] != 0:
                    exit_now = True
                if cfg.get("exitAfterProbe") and h % cfg["exitAfterProbe"] != 0:
                    probe_armed = True
        else:
            result = None
        body = json.dumps({"jsonrpc": "2.0", "id": req.get("id"), "result": result}).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


srv = HTTPServer((host, port), H)
srv.timeout = 0.1
while True:
    if cfg.get("exitAfter") is not None and time.time() - start > cfg["exitAfter"]:
        sys.exit(3)
    srv.handle_request()
    if exit_now:
        sys.exit(5 if probe_armed else 4)
