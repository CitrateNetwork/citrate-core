#!/usr/bin/env python3
"""Cold-sync harness (RC52 G4). RED STUB: keeps the behaviour of
src-tauri/scripts/c1_1_node_sync_proof.sh (a fixed `--network testnet`, no profile, no genesis
check: pass when the height advances) so the tests in cold-sync.test.mjs fail on assertions."""
import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--node-bin", required=True)
    p.add_argument("--profile")
    p.add_argument("--profiles")
    p.add_argument("--name")
    p.add_argument("--address-book")
    p.add_argument("--out", required=True)
    p.add_argument("--blocks")
    p.add_argument("--every")
    p.add_argument("--timeout", default="5")
    p.add_argument("--poll", default="4")
    p.add_argument("--genesis-wait")
    p.add_argument("--tip-slack")
    p.add_argument("--rpc-port", type=int, default=0)
    p.add_argument("--expect-sha256")
    a = p.parse_args()
    port = a.rpc_port
    if not port:
        s = socket.socket()
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()
    data = tempfile.mkdtemp(prefix="citrate-c1_1-sync-")
    proc = subprocess.Popen([a.node_bin, "--network", "testnet", "--data-dir", data],
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    first = last = None
    deadline = time.time() + float(a.timeout)
    while time.time() < deadline:
        try:
            req = urllib.request.Request(f"http://127.0.0.1:{port}", data=json.dumps(
                {"jsonrpc": "2.0", "id": 1, "method": "eth_blockNumber", "params": []}).encode(),
                headers={"content-type": "application/json"})
            h = int(json.loads(urllib.request.urlopen(req, timeout=1).read())["result"], 16)
            first = h if first is None else first
            last = h
            if h > first:
                break
        except Exception:
            pass
        time.sleep(float(a.poll))
    proc.kill()
    ok = first is not None and last > first
    shutil.rmtree(data, ignore_errors=True)
    os.makedirs(a.out, exist_ok=True)
    v = {"verdict": "PASS" if ok else "FAIL", "reasons": [] if ok else ["height did not advance"],
         "rules": [{"rule": "height_advances", "status": "ok" if ok else "FAIL", "detail": ""}]}
    open(os.path.join(a.out, "verdict.json"), "w").write(json.dumps(v))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
