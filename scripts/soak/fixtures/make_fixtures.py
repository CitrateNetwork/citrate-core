#!/usr/bin/env python3
"""Regenerate the golden soak fixtures (deterministic).

Each fixture is a 3-hour run sampled every 5 minutes (37 samples) against two RPC endpoints,
rpc-a and rpc-b, with the app monitored. "pass" is a healthy run that still carries the small
imperfections a real run has (one unreachable sample, one low-peer sample, a 5-minute finality
pause, one failed re-read that a later sample verifies, slow RSS and fd growth, a 2-block head
spread), so every threshold in the shipped criteria has something to bite on. Every other
fixture is "pass" with one fault injected. soak-monitor.test.mjs checks the committed files
match this generator byte for byte.
"""
import argparse
import copy
import hashlib
import json
import math
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
        "format": 2,
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
    for i in range(N):
        t = T0 + i * STEP
        hours = i * STEP / 3600.0
        head_a = 5000 + 130 * i
        fin = 4980 + 130 * (19 if i == 20 else i)  # one 5-minute finality pause at seq 20
        a = rpc("rpc-a", head_a, fin, peers=2 if i == 8 else 3)
        b = down("rpc-b") if i == 7 else rpc("rpc-b", head_a - 2, fin)
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
        out.append({"kind": "sample", "seq": i, "t": t, "rpcs": [a, b], "app": app})
    return out


def add_fork_checks(ss, ctx):
    """The sampler's fork check, replayed: hash at (lowest head - LAG) per reachable endpoint,
    then a re-read of each endpoint's reference height. A failed re-read keeps the reference
    (carried forward, listed in "unverified") until a later re-read succeeds.
    ctx: "side" {(seq, label)} reports a different hash at the common height; "unchecked"
    {(seq, label)} re-read fails; "changed" {(seq, label)} re-read finds a different hash."""
    refs = {}
    for s in ss:
        seq = s["seq"]
        ok = [r for r in s["rpcs"] if r["ok"]]
        h = min(r["head"] for r in ok) - LAG
        hashes = {r["label"]: bhash("side" if (seq, r["label"]) in ctx["side"] else "main", h) for r in ok}
        recheck = []
        for r in ok:
            label = r["label"]
            if label not in refs:
                continue
            if (seq, label) in ctx["unchecked"]:
                recheck.append({"label": label, "height": refs[label], "result": "unchecked"})
                continue
            result = "changed" if (seq, label) in ctx["changed"] else "same"
            recheck.append({"label": label, "height": refs.pop(label), "result": result})
        unverified = dict(sorted(refs.items()))
        for label in hashes:
            refs.setdefault(label, h)
        s["fork_check"] = {"height": h, "hashes": hashes, "recheck": recheck, "unverified": unverified}


def _rpc(s, label):
    return next(r for r in s["rpcs"] if r["label"] == label)


def fork(ss, ctx):
    ctx["side"] |= {(i, "rpc-b") for i in range(20, N)}


def reorg(ss, ctx):
    ctx["changed"].add((25, "rpc-a"))


def reread_errored(ss, ctx):
    # rpc-a's re-reads fail from seq 30 to the end: its reference height is never verified.
    ctx["unchecked"] |= {(i, "rpc-a") for i in range(30, N)}


def reread_carried_reorg(ss, ctx):
    # The re-read at seq 20 fails; the carried height is re-read at seq 21 and has changed.
    ctx["unchecked"].add((20, "rpc-a"))
    ctx["changed"].add((21, "rpc-a"))


def stalled_head(ss, ctx):
    for s in ss[15:]:
        for r in s["rpcs"]:
            if r["ok"]:
                f = _rpc(ss[14], r["label"])
                r["head"], r["finalized"] = f["head"], f["finalized"]


def finality_stall(ss, ctx):
    frozen = _rpc(ss[15], "rpc-a")["finalized"]
    for s in ss[15:21]:
        for r in s["rpcs"]:
            r["finalized"] = frozen


def no_finality(ss, ctx):
    for s in ss:
        for r in s["rpcs"]:
            if r["ok"]:
                r["finalized"] = 0


def leak(ss, ctx):
    for i, s in enumerate(ss):
        s["app"]["rss_kb"] = int(400000 * (1 + 0.05 * i * STEP / 3600.0))


def restart_storm(ss, ctx):
    for i, s in enumerate(ss[10:], 10):
        s["app"]["children"][0]["pid"] = 6000 + i
        s["app"]["crash_records"] = {"citrate-node": i - 9}


def restart_once(ss, ctx):
    for s in ss[18:]:
        s["app"]["children"][0]["pid"] = 5101
        s["app"]["crash_records"] = {"citrate-node": 1}


def sidecar_vanished(ss, ctx):
    # hermes-agent drops out after the warm-up and never comes back; no crash record.
    for s in ss[25:]:
        s["app"]["children"] = [c for c in s["app"]["children"] if c["name"] != "hermes-agent"]


def genesis_mismatch(ss, ctx):
    for s in ss:
        for r in s["rpcs"]:
            if r["label"] == "rpc-b" and r["ok"]:
                r["genesis_hash"] = OTHER_GENESIS


def wrong_chain(ss, ctx):
    _rpc(ss[12], "rpc-a")["chain_id"] = 1


def rpc_down(ss, ctx):
    for s in ss[10:16]:
        s["rpcs"][1] = down("rpc-b")


def app_exit(ss, ctx):
    for s in ss[30:]:
        s["app"] = {"pid": 4242, "alive": False, "rss_kb": None, "fds": None, "children": [], "crash_records": {}}


def sleep_gap(ss, ctx):
    del ss[12:18]


def head_spread(ss, ctx):
    for s in ss[20:]:
        r = _rpc(s, "rpc-b")
        r["head"] -= 500
        r["finalized"] = min(r["finalized"], r["head"])


def chain_only(ss, ctx):
    for s in ss:
        s["app"] = None


VARIANTS = {
    "pass": None,
    "fork": fork,
    "reorg": reorg,
    "reread-errored": reread_errored,
    "reread-carried-reorg": reread_carried_reorg,
    "stalled-head": stalled_head,
    "finality-stall": finality_stall,
    "no-finality": no_finality,
    "leak": leak,
    "restart-storm": restart_storm,
    "restart-once": restart_once,
    "sidecar-vanished": sidecar_vanished,
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
    # The healthy run's one failed re-read, verified by the next sample.
    ctx = {"side": set(), "unchecked": {(20, "rpc-a")}, "changed": set()}
    fn = VARIANTS[name]
    if fn is not None:
        fn(ss, ctx)
    add_fork_checks(ss, ctx)
    lines = [json.dumps(header(app=name != "chain-only"), sort_keys=True)]
    lines += [json.dumps(s, sort_keys=True) for s in ss]
    return "\n".join(lines) + "\n"


def main():
    p = argparse.ArgumentParser(description="Regenerate the golden soak fixtures.")
    p.add_argument(
        "out",
        nargs="?",
        default=str(Path(__file__).resolve().parent),
        help="output directory (default: this fixtures directory)",
    )
    out = Path(p.parse_args().out)
    out.mkdir(parents=True, exist_ok=True)
    for name in VARIANTS:
        (out / f"{name}.jsonl").write_text(render(name))
    print(f"wrote {len(VARIANTS)} fixtures to {out}")


if __name__ == "__main__":
    main()
