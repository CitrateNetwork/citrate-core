#!/usr/bin/env python3
"""Test double for the citrate node sidecar, used only by scripts/e2e/cold-sync.test.mjs.

It takes the node's real command line (--network, --data-dir, --config), records what it was
started with, and serves a small JSON-RPC on the [rpc] listen_addr of the config it was given.
Its behaviour comes from the JSON file named by the first line of its own wrapper (the test writes
a wrapper script that sets STAND_IN_CFG and execs this file), because cold-sync.py starts the node
with a minimal environment and nothing else would reach it.

Config keys: record (path), chainId, genesis, seed, rate (blocks/s), headMax, forkAt, forkSeed,
missing ([from, to]: no block served in that range, by number or by hash), decoyEvery (n: by number,
every height that is a multiple of n answers with a sibling block that is not on the selected-parent
chain, the way eth_getBlockByNumber is not canonical on r1006, chain#421), exitAfter (s),
exitAfterHashOf (h: exit right after serving the block at height h by hash; the harness reads blocks
by hash only in its final selected-parent walk), exitAfterProbeAfterHashOf (h: after serving the block
at height h by hash, exit right after serving the next eth_blockNumber: the harness's liveness probe),
noRpc, peers, encMeta.
The selected-parent chain: block 0 is `genesis`; block h > 0 is 0x + sha256("<seed>:<h>") (the fork
seed from forkAt on) and its parent is the chain block at h - 1. A decoy at h is 0x + sha256("decoy:<h>")
with the same parent.
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


def decoy_hash(h):
    return "0x" + hashlib.sha256(f"decoy:{h}".encode()).hexdigest()


def is_decoy_height(h):
    n = cfg.get("decoyEvery")
    return bool(n) and h > 0 and h % n == 0


def missing(h):
    gap = cfg.get("missing")
    return bool(gap) and gap[0] <= h <= gap[1]


def block(h, decoy=False):
    return {"number": hex(h), "hash": decoy_hash(h) if decoy else block_hash(h),
            "parentHash": block_hash(h - 1) if h > 0 else "0x" + "00" * 32,
            "selectedParentHash": block_hash(h - 1) if h > 0 else "0x" + "00" * 32}


# hash -> (height, decoy), filled up to the head as it is asked for.
index = {}
indexed_to = -1


def by_hash(hsh):
    global indexed_to
    top = head()
    while indexed_to < top:
        indexed_to += 1
        index[block_hash(indexed_to)] = (indexed_to, False)
        if is_decoy_height(indexed_to):
            index[decoy_hash(indexed_to)] = (indexed_to, True)
    hit = index.get(hsh.lower()) if isinstance(hsh, str) else None
    if hit is None or hit[0] > top or missing(hit[0]):
        return None
    return block(*hit)


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
            if h > head() or missing(h):
                result = None
            else:
                result = block(h, decoy=is_decoy_height(h))
        elif method == "eth_getBlockByHash":
            result = by_hash(params[0])
            if result is not None:
                h = int(result["number"], 16)
                if cfg.get("exitAfterHashOf") is not None and h == cfg["exitAfterHashOf"]:
                    exit_now = True
                if cfg.get("exitAfterProbeAfterHashOf") is not None and h == cfg["exitAfterProbeAfterHashOf"]:
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
