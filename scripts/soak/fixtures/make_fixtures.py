#!/usr/bin/env python3
"""Regenerate the golden soak fixtures (deterministic). Usage: make_fixtures.py [OUT_DIR]

Each fixture is a 3-hour run sampled every 5 minutes (37 samples) against two RPC endpoints,
rpc-a and rpc-b, with the app monitored. "pass" is a healthy run that still carries the small
imperfections a real run has (one unreachable sample, one low-peer sample, a 5-minute finality
pause, slow RSS and fd growth, a 2-block head spread), so every threshold in the shipped
criteria has something to bite on. Every other fixture is "pass" with one fault injected.
soak-monitor.test.mjs checks the committed files match this generator byte for byte.
"""
import copy
import hashlib
import json
import math
import sys
from pathlib import Path

T0 = 1793000000.0
STEP = 300.0
N = 37
GENESIS = "0x" + "1d" * 32
OTHER_GENESIS = "0x" + "ee" * 32
CHAIN_ID = 40204
LAG = 6


def bhash(tag, height):
    return "0x" + hashlib.sha256(f"{tag}:{height}".encode()).hexdigest()


def header(app=True):
    return {
        "kind": "header",
        "format": 1,
        "tool": "citrate-core soak-monitor",
        "started_unix": T0,
        "expected": {"chain_id": CHAIN_ID, "genesis_hash": GENESIS},
        "interval_s": STEP,
        "duration_s": STEP * (N - 1),
        "lag_blocks": LAG,
        "rpcs": ["rpc-a", "rpc-b"],
        "app": {"pid": 4242, "data_dirs": 1} if app else None,
        "host": {"os": "linux", "arch": "x86_64"},
    }


def rpc(label, head, finalized, peers=3):
    return {
        "label": label,
        "ok": True,
        "error": None,
        "chain_id": CHAIN_ID,
        "genesis_hash": GENESIS,
        "head": head,
        "finalized": finalized,
        "peers": peers,
    }


def down(label):
    return {
        "label": label,
        "ok": False,
        "error": "URLError: <urlopen error timed out>",
        "chain_id": None,
        "genesis_hash": None,
        "head": None,
        "finalized": None,
        "peers": None,
    }


def base_samples():
    out = []
    prev_h = None
    for i in range(N):
        t = T0 + i * STEP
        hours = i * STEP / 3600.0
        head_a = 5000 + 130 * i
        head_b = head_a - 2
        fin = 4980 + 130 * (19 if i == 20 else i)  # one 5-minute finality pause at seq 20
        a = rpc("rpc-a", head_a, fin, peers=2 if i == 8 else 3)
        b = down("rpc-b") if i == 7 else rpc("rpc-b", head_b, fin)
        ok = [r for r in (a, b) if r["ok"]]
        h = min(r["head"] for r in ok) - LAG
        fc = {
            "height": h,
            "hashes": {r["label"]: bhash("main", h) for r in ok},
            "recheck": None if prev_h is None else {"height": prev_h, "changed": []},
        }
        prev_h = h
        app = {
            "pid": 4242,
            "alive": True,
            "rss_kb": int(400000 * (1 + 0.003 * hours) + 200 * math.sin(i)),
            "fds": 200 + int(hours),
            "children": [
                {"pid": 5001, "name": "citrate-node", "rss_kb": 900000},
                {"pid": 5002, "name": "hermes-agent", "rss_kb": 120000},
                {"pid": 5003, "name": "mcp_serve", "rss_kb": 60000},
            ],
            "crash_records": {},
        }
        out.append({"kind": "sample", "seq": i, "t": t, "rpcs": [a, b], "fork_check": fc, "app": app})
    return out


def _rpc(s, label):
    return next(r for r in s["rpcs"] if r["label"] == label)


def fork(ss):
    for s in ss[20:]:
        fc = s["fork_check"]
        fc["hashes"]["rpc-b"] = bhash("side", fc["height"])


def reorg(ss):
    ss[25]["fork_check"]["recheck"]["changed"] = ["rpc-a"]


def stalled_head(ss):
    for s in ss[15:]:
        for r in s["rpcs"]:
            if r["ok"]:
                f = _rpc(ss[14], r["label"])
                r["head"], r["finalized"] = f["head"], f["finalized"]
        h = min(r["head"] for r in s["rpcs"] if r["ok"]) - LAG
        s["fork_check"]["height"] = h
        s["fork_check"]["hashes"] = {k: bhash("main", h) for k in s["fork_check"]["hashes"]}


def finality_stall(ss):
    frozen = _rpc(ss[15], "rpc-a")["finalized"]
    for s in ss[15:21]:
        for r in s["rpcs"]:
            r["finalized"] = frozen


def no_finality(ss):
    for s in ss:
        for r in s["rpcs"]:
            if r["ok"]:
                r["finalized"] = 0


def leak(ss):
    for i, s in enumerate(ss):
        s["app"]["rss_kb"] = int(400000 * (1 + 0.05 * i * STEP / 3600.0))


def restart_storm(ss):
    for i, s in enumerate(ss[10:], 10):
        s["app"]["children"][0]["pid"] = 6000 + i
        s["app"]["crash_records"] = {"citrate-node": i - 9}


def restart_once(ss):
    for s in ss[18:]:
        s["app"]["children"][0]["pid"] = 5101
        s["app"]["crash_records"] = {"citrate-node": 1}


def genesis_mismatch(ss):
    for s in ss:
        for r in s["rpcs"]:
            if r["label"] == "rpc-b" and r["ok"]:
                r["genesis_hash"] = OTHER_GENESIS


def wrong_chain(ss):
    _rpc(ss[12], "rpc-a")["chain_id"] = 1


def rpc_down(ss):
    for s in ss[10:16]:
        s["rpcs"][1] = down("rpc-b")
        s["fork_check"]["hashes"].pop("rpc-b", None)


def app_exit(ss):
    for s in ss[30:]:
        s["app"] = {"pid": 4242, "alive": False, "rss_kb": None, "fds": None, "children": [], "crash_records": {}}


def sleep_gap(ss):
    del ss[12:18]


def head_spread(ss):
    for s in ss[20:]:
        r = _rpc(s, "rpc-b")
        r["head"] -= 500
        r["finalized"] = min(r["finalized"], r["head"])
        h = r["head"] - LAG
        s["fork_check"]["height"] = h
        s["fork_check"]["hashes"] = {k: bhash("main", h) for k in s["fork_check"]["hashes"]}


def chain_only(ss):
    for s in ss:
        s["app"] = None


VARIANTS = {
    "pass": None,
    "fork": fork,
    "reorg": reorg,
    "stalled-head": stalled_head,
    "finality-stall": finality_stall,
    "no-finality": no_finality,
    "leak": leak,
    "restart-storm": restart_storm,
    "restart-once": restart_once,
    "genesis-mismatch": genesis_mismatch,
    "wrong-chain": wrong_chain,
    "rpc-down": rpc_down,
    "app-exit": app_exit,
    "sleep-gap": sleep_gap,
    "head-spread": head_spread,
    "chain-only": chain_only,
}


def render(name):
    ss = copy.deepcopy(base_samples())
    fn = VARIANTS[name]
    if fn is not None:
        fn(ss)
    lines = [json.dumps(header(app=name != "chain-only"), sort_keys=True)]
    lines += [json.dumps(s, sort_keys=True) for s in ss]
    return "\n".join(lines) + "\n"


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent
    out.mkdir(parents=True, exist_ok=True)
    for name in VARIANTS:
        (out / f"{name}.jsonl").write_text(render(name))
    print(f"wrote {len(VARIANTS)} fixtures to {out}")


if __name__ == "__main__":
    main()
