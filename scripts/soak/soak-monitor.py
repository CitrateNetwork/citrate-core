#!/usr/bin/env python3
"""Soak monitor for the 0.5.2 release candidate (RC52 gate G9). Read-only.

Two subcommands:

  run      Sample the chain (and, optionally, the running app) on an interval. Writes one JSON
           line per sample to <out>/samples.jsonl, then a verdict to <out>/verdict.json.
  verdict  Recompute the verdict from an existing samples.jsonl.

What one sample records
  Chain, for every --rpc: chain id, block-0 hash, head height, finalized height (the
  "finalized" block tag; null when the RPC refuses it), peer count (net_peerCount; null when
  refused). Then a fork check in the style of citrate-chain scripts/genesis/fleet_agree.sh:
  common height H = (lowest head) - LAG, block hash at H on every endpoint, compared. It also
  re-reads the previous sample's H, so a reorg below the lag shows up even with one RPC.
  App, when --pid is given: alive, RSS (kB), open file descriptors, the direct child processes
  (the sidecars: pid, name, RSS), and the supervisor crash records written since the run began
  (every *crash*.jsonl / *crash*.log under each --data-dir; one JSON line per crash, written by
  kit/src/supervisor.rs append_crash_record).

The pass/fail rules and their numbers live in scripts/soak/soak-criteria.json (versioned).
Each rule is reported as ok, FAIL or skip; the verdict is PASS only when no rule FAILs.
Missing evidence fails closed: an unknown is never read as a pass.

Usage
  soak-monitor.py run --rpc https://rpc.citrate.ai [--rpc LABEL=URL ...] \\
      --chain-id 40204 --genesis-hash 0x<64 hex> --interval 60 --duration 24h \\
      [--pid <app pid> --data-dir <app data dir> ...] [--lag 6] \\
      [--criteria scripts/soak/soak-criteria.json] [--explanations restarts.json] --out <dir>
  soak-monitor.py verdict --samples <dir>/samples.jsonl [--criteria ...] [--explanations ...] \\
      [--out verdict.json]

Exit: 0 PASS · 1 FAIL · 2 usage or invalid input (criteria, explanations, arguments).
Needs Python 3.9+ and nothing outside the standard library. It never writes to the chain,
never signals the app, and never starts anything.
"""
import argparse
import json
import os
import platform
import re
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

TOOL = "citrate-core soak-monitor"
SAMPLE_FORMAT = 2
CRITERIA_SCHEMA = "citrate-core/soak-criteria"
CRITERIA_VERSION = 1
DEFAULT_CRITERIA = Path(__file__).resolve().parent / "soak-criteria.json"
HASH_RE = re.compile(r"^0x[0-9a-f]{64}$")
RPC_TIMEOUT_S = 10
CRASH_SCAN_DEPTH = 3

# Every criteria key, its section, its type and the rule that reads it. load_criteria refuses a
# file with a key missing or a key not listed here, so a threshold can never be silently ignored.
CRITERIA_KEYS = {
    "run": {
        "min_duration_hours": ("num", "duration"),
        "max_sample_gap_minutes": ("num", "sampling_gaps"),
    },
    "chain": {
        "max_consecutive_unreachable": ("num", "rpc_reachable"),
        "max_head_spread": ("num", "rpcs_agree"),
        "head_window_minutes": ("num", "head_advances"),
        "min_blocks_per_window": ("num", "head_advances"),
        "min_peers": ("num", "peers"),
        "max_peers_below_minutes": ("num", "peers"),
    },
    "finality": {
        "mode": (("auto", "required", "off"), "finality"),
        "max_stall_minutes": ("num", "finality"),
    },
    "app": {
        "warmup_minutes": ("num", "rss_growth"),
        "min_post_warmup_minutes": ("num", "rss_growth"),
        "max_rss_growth_pct_per_hour": ("num", "rss_growth"),
        "max_fd_growth_pct_per_hour": ("num", "fd_growth"),
        "max_unexplained_restarts": ("num", "sidecar_restarts"),
    },
}
RULES = [
    "samples",
    "duration",
    "sampling_gaps",
    "chain_identity",
    "rpc_reachable",
    "rpcs_agree",
    "no_fork",
    "head_advances",
    "finality",
    "peers",
    "app_alive",
    "rss_growth",
    "fd_growth",
    "sidecar_restarts",
]


class UsageError(Exception):
    """Bad arguments or an invalid criteria/explanations file: exit 2."""


# ---------------------------------------------------------------------------------------------
# Criteria and explanations
# ---------------------------------------------------------------------------------------------


def load_criteria(path):
    try:
        doc = json.loads(Path(path).read_text())
    except (OSError, ValueError) as e:
        raise UsageError(f"criteria {path}: cannot read: {e}") from e
    if not isinstance(doc, dict):
        raise UsageError(f"criteria {path}: not a JSON object")
    if doc.get("schema") != CRITERIA_SCHEMA or doc.get("version") != CRITERIA_VERSION:
        raise UsageError(
            f"criteria {path}: want schema {CRITERIA_SCHEMA!r} version {CRITERIA_VERSION}, "
            f"got {doc.get('schema')!r} version {doc.get('version')!r}"
        )
    extra = set(doc) - {"schema", "version", "status", "notes"} - set(CRITERIA_KEYS)
    if extra:
        raise UsageError(f"criteria {path}: unknown section(s) {sorted(extra)}")
    out = {}
    for section, keys in CRITERIA_KEYS.items():
        got = doc.get(section)
        if not isinstance(got, dict):
            raise UsageError(f"criteria {path}: section {section!r} missing")
        unknown = set(got) - set(keys)
        if unknown:
            raise UsageError(f"criteria {path}: {section}: unknown key(s) {sorted(unknown)}")
        out[section] = {}
        for key, (kind, _rule) in keys.items():
            if key not in got:
                raise UsageError(f"criteria {path}: {section}.{key} missing")
            v = got[key]
            if kind == "num":
                if isinstance(v, bool) or not isinstance(v, (int, float)) or v < 0:
                    raise UsageError(f"criteria {path}: {section}.{key} must be a number >= 0")
            elif v not in kind:
                raise UsageError(f"criteria {path}: {section}.{key} must be one of {list(kind)}")
            out[section][key] = v
    return out


def load_explanations(path):
    """{"restarts": [{"sidecar": name, "count": n, "reason": text}]} -> {name: n}."""
    if path is None:
        return {}
    try:
        doc = json.loads(Path(path).read_text())
    except (OSError, ValueError) as e:
        raise UsageError(f"explanations {path}: cannot read: {e}") from e
    items = doc.get("restarts") if isinstance(doc, dict) else None
    if not isinstance(items, list):
        raise UsageError(f"explanations {path}: want {{\"restarts\": [...]}}")
    out = {}
    for i, it in enumerate(items):
        ok = (
            isinstance(it, dict)
            and isinstance(it.get("sidecar"), str)
            and it["sidecar"]
            and isinstance(it.get("count"), int)
            and not isinstance(it.get("count"), bool)
            and it["count"] > 0
            and isinstance(it.get("reason"), str)
            and it["reason"].strip()
        )
        if not ok:
            raise UsageError(
                f"explanations {path}: entry {i} needs a sidecar name, a count > 0 and a reason"
            )
        out[it["sidecar"]] = out.get(it["sidecar"], 0) + it["count"]
    return out


# ---------------------------------------------------------------------------------------------
# Verdict
# ---------------------------------------------------------------------------------------------


def read_samples(path):
    """Returns (header, samples, problems). A line that is not valid JSON is a problem."""
    header = None
    samples = []
    problems = []
    try:
        text = Path(path).read_text()
    except OSError as e:
        return None, [], [f"cannot read {path}: {e}"]
    for n, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        try:
            obj = json.loads(line)
        except ValueError:
            problems.append(f"line {n} is not valid JSON")
            continue
        kind = obj.get("kind") if isinstance(obj, dict) else None
        if kind == "header" and header is None:
            header = obj
        elif kind == "sample":
            samples.append(obj)
        else:
            problems.append(f"line {n} is not a header or sample")
    if header is not None and header.get("format") != SAMPLE_FORMAT:
        problems.append(f"sample format {header.get('format')!r}, want {SAMPLE_FORMAT}")
    return header, samples, problems


def _longest_run(flags_with_t, t_end=None):
    """Longest stretch, in minutes, over which the flag stayed true. A stretch runs from the
    first true sample to the next false one (or to t_end)."""
    longest = 0.0
    start = None
    last_t = None
    for t, flag in flags_with_t:
        if flag and start is None:
            start = t
        elif not flag and start is not None:
            longest = max(longest, (t - start) / 60.0)
            start = None
        last_t = t
    if start is not None:
        end = t_end if t_end is not None else last_t
        longest = max(longest, (end - start) / 60.0)
    return longest


def _slope_pct_per_hour(points):
    """Least-squares slope of (t seconds, value), as a percent of the mean value per hour."""
    n = len(points)
    if n < 2:
        return None
    mt = sum(t for t, _ in points) / n
    mv = sum(v for _, v in points) / n
    den = sum((t - mt) ** 2 for t, _ in points)
    if den == 0 or mv <= 0:
        return None
    slope = sum((t - mt) * (v - mv) for t, v in points) / den  # per second
    return slope * 3600.0 / mv * 100.0


def evaluate(header, samples, problems, criteria, explained):
    rules = {}

    def put(rule, status, detail):
        rules[rule] = {"rule": rule, "status": status, "detail": detail}

    run_c, chain_c, fin_c, app_c = (
        criteria["run"],
        criteria["chain"],
        criteria["finality"],
        criteria["app"],
    )

    if header is None or not samples or problems:
        why = list(problems)
        if header is None:
            why.append("no header line")
        if not samples:
            why.append("no samples")
        put("samples", "FAIL", "; ".join(why))
        for r in RULES[1:]:
            put(r, "FAIL", "no usable samples")
        return _finish(rules, header, samples, criteria)
    put("samples", "ok", f"{len(samples)} samples")

    ts = [s["t"] for s in samples]
    span_h = (ts[-1] - ts[0]) / 3600.0
    want_h = run_c["min_duration_hours"]
    put(
        "duration",
        "ok" if span_h >= want_h else "FAIL",
        f"ran {span_h:.2f} h, need >= {want_h} h",
    )

    gaps = [(b - a) / 60.0 for a, b in zip(ts, ts[1:])]
    max_gap = max(gaps) if gaps else 0.0
    put(
        "sampling_gaps",
        "ok" if max_gap <= run_c["max_sample_gap_minutes"] else "FAIL",
        f"longest gap between samples {max_gap:.1f} min, limit {run_c['max_sample_gap_minutes']} min"
        + ("" if max_gap <= run_c["max_sample_gap_minutes"] else " (did the host sleep?)"),
    )

    expected = header.get("expected") or {}
    want_id = expected.get("chain_id")
    want_g = (expected.get("genesis_hash") or "").lower()
    labels = [r for r in header.get("rpcs", [])]

    # chain identity: every answer names the expected chain id and genesis.
    bad = []
    for s in samples:
        for r in s.get("rpcs", []):
            if not r.get("ok"):
                continue
            if r.get("chain_id") != want_id:
                bad.append(f"seq {s['seq']} {r['label']} chain id {r.get('chain_id')}, want {want_id}")
            if (r.get("genesis_hash") or "").lower() != want_g:
                bad.append(f"seq {s['seq']} {r['label']} genesis {r.get('genesis_hash')}, want {want_g}")
    put(
        "chain_identity",
        "FAIL" if bad or not want_g else "ok",
        ("; ".join(bad[:5]) + (f" (+{len(bad) - 5} more)" if len(bad) > 5 else ""))
        if bad
        else (f"chain {want_id}, genesis {want_g}" if want_g else "no expected genesis in header"),
    )

    # reachability: no endpoint unreachable for more than N consecutive samples.
    worst = {}
    for label in labels:
        streak = best = 0
        for s in samples:
            r = _rpc(s, label)
            if r is None or not r.get("ok"):
                streak += 1
                best = max(best, streak)
            else:
                streak = 0
        worst[label] = best
    limit = chain_c["max_consecutive_unreachable"]
    over = {k: v for k, v in worst.items() if v > limit}
    put(
        "rpc_reachable",
        "FAIL" if over or not labels else "ok",
        (
            "; ".join(f"{k} unreachable {v} samples in a row (limit {limit})" for k, v in over.items())
            if over
            else (f"longest unreachable streak {max(worst.values())} (limit {limit})" if labels else "no RPCs")
        ),
    )

    # rpcs agree: same genesis across endpoints and heads within the spread.
    bad = []
    max_spread = 0
    for s in samples:
        ok = [r for r in s.get("rpcs", []) if r.get("ok")]
        gs = {(r.get("genesis_hash") or "").lower() for r in ok}
        if len(gs) > 1:
            bad.append(f"seq {s['seq']} endpoints report different genesis hashes")
        heads = [r["head"] for r in ok if isinstance(r.get("head"), int)]
        if heads:
            spread = max(heads) - min(heads)
            max_spread = max(max_spread, spread)
            if spread > chain_c["max_head_spread"]:
                bad.append(f"seq {s['seq']} heads {spread} blocks apart (limit {chain_c['max_head_spread']})")
    put(
        "rpcs_agree",
        "FAIL" if bad else "ok",
        "; ".join(bad[:5]) if bad else f"largest head spread {max_spread} blocks",
    )

    # no fork: the hash at the common height agrees everywhere, no re-read changed, and every
    # reference height was re-read in the end (a failed re-read is carried, never assumed same).
    bad = []
    checked = 0
    deferred = 0
    for s in samples:
        fc = s.get("fork_check") or {}
        hashes = fc.get("hashes") or {}
        if hashes:
            checked += 1
            if len({(h or "").lower() for h in hashes.values()}) > 1 or any(not h for h in hashes.values()):
                bad.append(f"seq {s['seq']} block {fc.get('height')}: {hashes}")
        for e in fc.get("recheck") or []:
            result = e.get("result")
            if result == "changed":
                bad.append(f"seq {s['seq']} block {e.get('height')} hash changed on {e.get('label')} (reorg)")
            elif result == "unchecked":
                deferred += 1
            elif result != "same":
                bad.append(f"seq {s['seq']} re-read result {result!r} on {e.get('label')}")
    last_fc = samples[-1].get("fork_check") or {}
    for label, h in sorted((last_fc.get("unverified") or {}).items()):
        bad.append(f"block {h} on {label} was never re-read after a failed or missed re-read (unverified)")
    put(
        "no_fork",
        "FAIL" if bad or checked == 0 else "ok",
        (
            "; ".join(bad[:5])
            if bad
            else (
                f"{checked} fork checks agreed"
                + (f"; {deferred} re-read deferred and later verified" if deferred else "")
                if checked
                else "no fork check completed"
            )
        ),
    )

    # head advances: on every endpoint, every full window gains >= min blocks.
    win_s = chain_c["head_window_minutes"] * 60.0
    need = chain_c["min_blocks_per_window"]
    bad = []
    windows = 0
    worst_gain = None
    for label in labels:
        pts = [(s["t"], _rpc(s, label)) for s in samples]
        pts = [(t, r["head"]) for t, r in pts if r and r.get("ok") and isinstance(r.get("head"), int)]
        j = 0
        for i, (t, h) in enumerate(pts):
            if t - pts[0][0] < win_s:
                continue
            while j + 1 < i and pts[j + 1][0] <= t - win_s:
                j += 1
            gain = h - pts[j][1]
            windows += 1
            worst_gain = gain if worst_gain is None else min(worst_gain, gain)
            if gain < need:
                bad.append(f"{label} gained {gain} blocks in the {chain_c['head_window_minutes']} min to seq-time {t:.0f}")
    put(
        "head_advances",
        "FAIL" if bad or windows == 0 else "ok",
        (
            "; ".join(bad[:5]) + (f" (+{len(bad) - 5} more)" if len(bad) > 5 else "")
            if bad
            else (
                f"{windows} windows, slowest gained {worst_gain} blocks (need {need})"
                if windows
                else f"no full {chain_c['head_window_minutes']} min window observed"
            )
        ),
    )

    # finality: active when any endpoint reports a finalized height above 0.
    mode = fin_c["mode"]
    active = any(
        isinstance(r.get("finalized"), int) and r["finalized"] > 0
        for s in samples
        for r in s.get("rpcs", [])
        if r.get("ok")
    )
    if mode == "off":
        put("finality", "skip", "finality checks off in criteria")
    elif not active:
        if mode == "required":
            put("finality", "FAIL", "finality required, but no endpoint reported a finalized height above 0")
        else:
            put("finality", "ok", "finality not active on this chain (finalized height 0 or not exposed); mode auto")
    else:
        bad = []
        worst_stall = 0.0
        for label in labels:
            last_f = None
            last_up = None
            for s in samples:
                r = _rpc(s, label)
                if not r or not r.get("ok") or not isinstance(r.get("finalized"), int):
                    continue
                f = r["finalized"]
                if isinstance(r.get("head"), int) and f > r["head"]:
                    bad.append(f"{label} seq {s['seq']} finalized {f} above head {r['head']}")
                if last_f is None or f > last_f:
                    last_f, last_up = f, s["t"]
                elif f < last_f:
                    bad.append(f"{label} seq {s['seq']} finalized height went back {last_f} -> {f}")
                stall = (s["t"] - last_up) / 60.0
                worst_stall = max(worst_stall, stall)
            if last_up is None:
                bad.append(f"{label} never reported a finalized height")
        if worst_stall > fin_c["max_stall_minutes"]:
            bad.append(f"finalized height stalled {worst_stall:.1f} min (limit {fin_c['max_stall_minutes']} min)")
        put(
            "finality",
            "FAIL" if bad else "ok",
            "; ".join(bad[:5]) if bad else f"longest finality stall {worst_stall:.1f} min",
        )

    # peers: below the minimum for no longer than the allowance; unknown counts as below.
    bad = []
    for label in labels:
        flags = []
        for s in samples:
            r = _rpc(s, label)
            if not r or not r.get("ok"):
                continue
            p = r.get("peers")
            flags.append((s["t"], not isinstance(p, int) or p < chain_c["min_peers"]))
        longest = _longest_run(flags)
        if longest > chain_c["max_peers_below_minutes"]:
            bad.append(
                f"{label} below {chain_c['min_peers']} peers (or unknown) for {longest:.1f} min "
                f"(limit {chain_c['max_peers_below_minutes']} min)"
            )
        elif flags and all(f for _, f in flags):
            bad.append(f"{label} never reported >= {chain_c['min_peers']} peers")
    put("peers", "FAIL" if bad else "ok", "; ".join(bad) if bad else f"peers >= {chain_c['min_peers']}")

    # app-side rules.
    app_rows = [(s["t"], s.get("app")) for s in samples]
    if header.get("app") is None:
        for r in ("app_alive", "rss_growth", "fd_growth", "sidecar_restarts"):
            put(r, "skip", "no --pid: chain-only run")
        return _finish(rules, header, samples, criteria)

    dead = [s["seq"] for s in samples if not (s.get("app") or {}).get("alive")]
    put(
        "app_alive",
        "FAIL" if dead else "ok",
        f"app not running at seq {dead[:5]}" if dead else "app running at every sample",
    )

    warm_end = ts[0] + app_c["warmup_minutes"] * 60.0
    post_span = (ts[-1] - warm_end) / 60.0

    def growth(rule, field, limit, unit):
        pts = [(t, a[field]) for t, a in app_rows if a and a.get("alive") and t >= warm_end and isinstance(a.get(field), int)]
        if post_span < app_c["min_post_warmup_minutes"] or len(pts) < 2:
            put(
                rule,
                "FAIL",
                f"not enough {unit} data after the {app_c['warmup_minutes']} min warm-up "
                f"({max(post_span, 0):.0f} min, need {app_c['min_post_warmup_minutes']} min)",
            )
            return
        pct = _slope_pct_per_hour(pts)
        if pct is None:
            put(rule, "FAIL", f"{unit} growth could not be computed")
            return
        put(
            rule,
            "ok" if pct <= limit else "FAIL",
            f"{unit} growth {pct:.2f} %/h after warm-up (limit {limit} %/h)",
        )

    growth("rss_growth", "rss_kb", app_c["max_rss_growth_pct_per_hour"], "RSS")
    growth("fd_growth", "fds", app_c["max_fd_growth_pct_per_hour"], "open-fd")

    # sidecar restarts: crash records since the run began, child processes replaced, and
    # sidecars that vanished. The expected set is the children at the first sample after the
    # warm-up; each later disappearance of one of them counts as a restart (the stopped
    # supervisor case leaves no crash record and no new pid).
    crash = {}
    replaced = {}
    vanished = {}
    expected = None
    present = set()
    prev = None
    for t, a in app_rows:
        if not a or not a.get("alive"):
            continue
        for name, n in (a.get("crash_records") or {}).items():
            crash[name] = max(crash.get(name, 0), n)
        cur = {}
        for c in a.get("children") or []:
            cur.setdefault(c.get("name"), set()).add(c.get("pid"))
        if prev is not None:
            for name, pids in prev.items():
                if name in cur and pids - cur[name]:
                    replaced[name] = replaced.get(name, 0) + len(pids - cur[name])
        prev = cur
        if expected is None and t >= warm_end:
            expected = set(cur)
            present = set(cur)
        elif expected is not None:
            for name in expected:
                if name in present and name not in cur:
                    vanished[name] = vanished.get(name, 0) + 1
            present = expected & set(cur)
    names = set(crash) | set(replaced) | set(vanished)
    per = {n: max(crash.get(n, 0), replaced.get(n, 0) + vanished.get(n, 0)) for n in names}
    per = {n: v for n, v in per.items() if v > 0}
    unexplained = {n: v - explained.get(n, 0) for n, v in per.items() if v - explained.get(n, 0) > 0}
    total_unexplained = sum(unexplained.values())
    limit = app_c["max_unexplained_restarts"]
    put(
        "sidecar_restarts",
        "ok" if total_unexplained <= limit else "FAIL",
        (
            f"restarts {per or 'none'}; unexplained {unexplained or 'none'} (limit {limit})"
            + (f"; vanished after warm-up {vanished}" if vanished else "")
        ),
    )
    return _finish(rules, header, samples, criteria)


def _rpc(sample, label):
    for r in sample.get("rpcs", []):
        if r.get("label") == label:
            return r
    return None


def _finish(rules, header, samples, criteria):
    ordered = [rules[r] for r in RULES if r in rules]
    fails = [f"{r['rule']}: {r['detail']}" for r in ordered if r["status"] == "FAIL"]
    ts = [s.get("t") for s in samples if isinstance(s.get("t"), (int, float))]
    return {
        "verdict": "FAIL" if fails else "PASS",
        "reasons": fails,
        "rules": ordered,
        "run": {
            "samples": len(samples),
            "first_t": ts[0] if ts else None,
            "last_t": ts[-1] if ts else None,
            "span_hours": round((ts[-1] - ts[0]) / 3600.0, 3) if ts else 0,
            "expected": (header or {}).get("expected"),
            "rpcs": (header or {}).get("rpcs"),
            "app_monitored": (header or {}).get("app") is not None,
            "host": (header or {}).get("host"),
        },
        "criteria": criteria,
        "tool": TOOL,
    }


def print_verdict(v, stream=sys.stdout):
    print(f"SOAK {v['verdict']}", file=stream)
    for r in v["rules"]:
        tag = {"ok": "[ok]  ", "FAIL": "[FAIL]", "skip": "[skip]"}[r["status"]]
        print(f"  {tag} {r['rule']}: {r['detail']}", file=stream)


# ---------------------------------------------------------------------------------------------
# Sampling
# ---------------------------------------------------------------------------------------------


def rpc_call(url, method, params):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=RPC_TIMEOUT_S) as resp:
        doc = json.loads(resp.read())
    if not isinstance(doc, dict):
        raise ValueError("not a JSON-RPC response")
    if doc.get("error") is not None:
        raise RuntimeError(f"{method}: {doc['error']}")
    return doc.get("result")


def _hex_int(v):
    if isinstance(v, str) and re.match(r"^0x[0-9a-fA-F]+$", v):
        return int(v, 16)
    raise ValueError(f"not a hex quantity: {v!r}")


def _block_hash(url, height):
    b = rpc_call(url, "eth_getBlockByNumber", [hex(height), False])
    h = (b or {}).get("hash") if isinstance(b, dict) else None
    if not isinstance(h, str) or not HASH_RE.match(h.lower()):
        raise ValueError(f"block {height}: no hash")
    return h.lower()


def sample_rpc(label, url):
    out = {
        "label": label,
        "ok": False,
        "error": None,
        "chain_id": None,
        "genesis_hash": None,
        "head": None,
        "finalized": None,
        "peers": None,
    }
    try:
        out["chain_id"] = _hex_int(rpc_call(url, "eth_chainId", []))
        out["genesis_hash"] = _block_hash(url, 0)
        out["head"] = _hex_int(rpc_call(url, "eth_blockNumber", []))
        out["ok"] = True
    except (OSError, ValueError, RuntimeError, urllib.error.URLError) as e:
        out["error"] = _short_error(e)
        return out
    try:
        fb = rpc_call(url, "eth_getBlockByNumber", ["finalized", False])
        out["finalized"] = _hex_int(fb.get("number")) if isinstance(fb, dict) else None
    except (OSError, ValueError, RuntimeError, urllib.error.URLError):
        out["finalized"] = None
    try:
        out["peers"] = _hex_int(rpc_call(url, "net_peerCount", []))
    except (OSError, ValueError, RuntimeError, urllib.error.URLError):
        out["peers"] = None
    return out


def _short_error(e):
    s = f"{type(e).__name__}: {e}"
    return s[:200]


def fork_check(endpoints, readings, lag, refs):
    """endpoints: {label: url}. readings: the rpc rows of this sample. refs: {label: (height,
    hash)}, each endpoint's reference from an earlier sample, updated in place.

    Each reachable endpoint re-reads its reference height. A re-read that fails is recorded as
    "unchecked" and the reference is KEPT (listed in "unverified"), so a later sample re-reads
    the same height; an unknown is never read as "same". Only a verified (or changed)
    reference moves on to this sample's common height."""
    ok = [r for r in readings if r["ok"]]
    out = {"height": None, "hashes": {}, "recheck": [], "unverified": {}}
    if ok:
        h = max(min(r["head"] for r in ok) - lag, 0)
        out["height"] = h
        for r in ok:
            try:
                out["hashes"][r["label"]] = _block_hash(endpoints[r["label"]], h)
            except (OSError, ValueError, RuntimeError, urllib.error.URLError):
                out["hashes"][r["label"]] = None
    for r in ok:
        label = r["label"]
        if label not in refs:
            continue
        ph, before = refs[label]
        try:
            now = _block_hash(endpoints[label], ph)
        except (OSError, ValueError, RuntimeError, urllib.error.URLError):
            out["recheck"].append({"label": label, "height": ph, "result": "unchecked"})
            continue
        out["recheck"].append({"label": label, "height": ph, "result": "changed" if now != before else "same"})
        del refs[label]
    out["unverified"] = {label: ref[0] for label, ref in sorted(refs.items())}
    for label, hsh in out["hashes"].items():
        if hsh and label not in refs:
            refs[label] = (out["height"], hsh)
    return out


def _ps_table():
    """{pid: (ppid, rss_kb, name)} from ps. Same flags on macOS and Linux."""
    try:
        res = subprocess.run(
            ["ps", "-axo", "pid=,ppid=,rss=,comm="],
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    table = {}
    for line in res.stdout.splitlines():
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        try:
            pid, ppid, rss = int(parts[0]), int(parts[1]), int(parts[2])
        except ValueError:
            continue
        table[pid] = (ppid, rss, os.path.basename(parts[3].strip()))
    return table


def _open_fds(pid):
    proc = Path(f"/proc/{pid}/fd")
    if proc.is_dir():
        try:
            return len(os.listdir(proc))
        except OSError:
            return None
    try:
        res = subprocess.run(
            ["lsof", "-nP", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    lines = [ln for ln in res.stdout.splitlines() if ln.strip()]
    return max(len(lines) - 1, 0) if lines else None


def crash_records_since(data_dirs, since_ms):
    """{sidecar name: crash records with at_unix_ms >= since_ms} across every *crash*.jsonl or
    *crash*.log within CRASH_SCAN_DEPTH levels of each data dir."""
    counts = {}
    for root in data_dirs:
        root = Path(root)
        base_depth = len(root.parts)
        for dirpath, dirnames, filenames in os.walk(root):
            if len(Path(dirpath).parts) - base_depth >= CRASH_SCAN_DEPTH:
                dirnames[:] = []
            for fn in filenames:
                if "crash" not in fn or not fn.endswith((".jsonl", ".log")):
                    continue
                try:
                    lines = (Path(dirpath) / fn).read_text(errors="replace").splitlines()
                except OSError:
                    continue
                for line in lines:
                    try:
                        rec = json.loads(line)
                    except ValueError:
                        continue
                    if not isinstance(rec, dict):
                        continue
                    at = rec.get("at_unix_ms")
                    name = rec.get("name")
                    if isinstance(at, int) and isinstance(name, str) and at >= since_ms:
                        counts[name] = counts.get(name, 0) + 1
    return counts


def sample_app(pid, data_dirs, since_ms):
    table = _ps_table()
    if table is None or pid not in table:
        return {"pid": pid, "alive": False, "rss_kb": None, "fds": None, "children": [], "crash_records": {}}
    _ppid, rss, _name = table[pid]
    children = [
        {"pid": cpid, "name": cname, "rss_kb": crss}
        for cpid, (cppid, crss, cname) in sorted(table.items())
        if cppid == pid
    ]
    return {
        "pid": pid,
        "alive": True,
        "rss_kb": rss,
        "fds": _open_fds(pid),
        "children": children,
        "crash_records": crash_records_since(data_dirs, since_ms),
    }


def parse_rpcs(values):
    """--rpc URL or LABEL=URL. The default label is the host (and port) only, so a token in
    the path or query never reaches the samples file."""
    endpoints = {}
    for v in values:
        if "=" in v.split("://", 1)[0]:
            label, url = v.split("=", 1)
        else:
            label, url = None, v
        u = urllib.parse.urlsplit(url)
        if u.scheme not in ("http", "https") or not u.hostname:
            raise UsageError(f"--rpc {v!r}: want an http(s) URL")
        if label is None:
            label = u.hostname + (f":{u.port}" if u.port else "")
        base, n = label, 2
        while label in endpoints:
            label = f"{base}#{n}"
            n += 1
        endpoints[label] = url
    return endpoints


def parse_duration(s):
    m = re.match(r"^(\d+(?:\.\d+)?)([smh]?)$", s.strip())
    if not m:
        raise UsageError(f"--duration {s!r}: want a number with s, m or h")
    return float(m.group(1)) * {"": 1, "s": 1, "m": 60, "h": 3600}[m.group(2)]


def cmd_run(a):
    criteria = load_criteria(a.criteria)
    explained = load_explanations(a.explanations)
    endpoints = parse_rpcs(a.rpc)
    genesis = a.genesis_hash.lower()
    if not HASH_RE.match(genesis):
        raise UsageError("--genesis-hash must be 0x + 64 hex")
    if a.interval <= 0:
        raise UsageError("--interval must be > 0")
    if a.data_dir and a.pid is None:
        raise UsageError("--data-dir needs --pid")
    duration = parse_duration(a.duration)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    samples_path = out / "samples.jsonl"
    if samples_path.exists() and samples_path.stat().st_size > 0:
        raise UsageError(f"{samples_path} exists: give a fresh --out")

    stop = {"now": False}

    def on_signal(_sig, _frm):
        stop["now"] = True

    signal.signal(signal.SIGINT, on_signal)
    signal.signal(signal.SIGTERM, on_signal)

    started = time.time()
    since_ms = int(started * 1000)
    header = {
        "kind": "header",
        "format": SAMPLE_FORMAT,
        "tool": TOOL,
        "started_unix": round(started, 3),
        "expected": {"chain_id": a.chain_id, "genesis_hash": genesis},
        "interval_s": a.interval,
        "duration_s": duration,
        "lag_blocks": a.lag,
        "rpcs": list(endpoints),
        "app": None if a.pid is None else {"pid": a.pid, "data_dirs": len(a.data_dir)},
        "host": {"os": platform.system().lower(), "arch": platform.machine()},
    }
    refs = {}
    seq = 0
    with samples_path.open("a") as f:
        f.write(json.dumps(header) + "\n")
        f.flush()
        while not stop["now"]:
            t = time.time()
            readings = [sample_rpc(label, url) for label, url in endpoints.items()]
            fc = fork_check(endpoints, readings, a.lag, refs)
            row = {
                "kind": "sample",
                "seq": seq,
                "t": round(t, 3),
                "rpcs": readings,
                "fork_check": fc,
                "app": None if a.pid is None else sample_app(a.pid, a.data_dir, since_ms),
            }
            f.write(json.dumps(row) + "\n")
            f.flush()
            heads = ",".join(f"{r['label']}={r['head'] if r['ok'] else 'down'}" for r in readings)
            print(f"seq {seq} heads {heads} fork-check {fc['height']}", file=sys.stderr)
            seq += 1
            nxt = started + seq * a.interval
            if nxt - started > duration:
                break
            while not stop["now"] and time.time() < nxt:
                time.sleep(min(1.0, max(nxt - time.time(), 0)))

    header2, samples, problems = read_samples(samples_path)
    v = evaluate(header2, samples, problems, criteria, explained)
    v["interrupted"] = stop["now"]
    (out / "verdict.json").write_text(json.dumps(v, indent=2) + "\n")
    print_verdict(v)
    return 0 if v["verdict"] == "PASS" else 1


def cmd_verdict(a):
    criteria = load_criteria(a.criteria)
    explained = load_explanations(a.explanations)
    header, samples, problems = read_samples(a.samples)
    v = evaluate(header, samples, problems, criteria, explained)
    if a.out:
        Path(a.out).write_text(json.dumps(v, indent=2) + "\n")
    print_verdict(v)
    return 0 if v["verdict"] == "PASS" else 1


def main(argv=None):
    p = argparse.ArgumentParser(description="RC52 soak monitor (read-only).")
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="sample, then write samples.jsonl and verdict.json")
    r.add_argument("--rpc", action="append", required=True, help="RPC URL or LABEL=URL (repeat)")
    r.add_argument("--chain-id", type=int, required=True)
    r.add_argument("--genesis-hash", required=True)
    r.add_argument("--interval", type=float, default=60.0, help="seconds between samples (60)")
    r.add_argument("--duration", default="24h", help="e.g. 300s, 90m, 24h (24h)")
    r.add_argument("--lag", type=int, default=6, help="fork-check depth below the lowest head (6)")
    r.add_argument("--pid", type=int, help="the running app's PID (optional)")
    r.add_argument("--data-dir", action="append", default=[], help="app data dir for crash records (repeat)")
    r.add_argument("--criteria", default=str(DEFAULT_CRITERIA))
    r.add_argument("--explanations", help="JSON naming explained sidecar restarts")
    r.add_argument("--out", required=True, help="output dir (samples.jsonl, verdict.json)")
    v = sub.add_parser("verdict", help="recompute the verdict from samples.jsonl")
    v.add_argument("--samples", required=True)
    v.add_argument("--criteria", default=str(DEFAULT_CRITERIA))
    v.add_argument("--explanations")
    v.add_argument("--out", help="write the verdict JSON here")
    a = p.parse_args(argv)
    try:
        return cmd_run(a) if a.cmd == "run" else cmd_verdict(a)
    except UsageError as e:
        print(f"soak-monitor: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
