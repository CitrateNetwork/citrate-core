#!/usr/bin/env python3
"""Cold-sync harness for the node sidecar on a K6 network profile (RC52 gate G4, app side).

Starts the node sidecar binary (not the app) in a fresh temp dir, the way NodeManager::build_spec
starts it (src-tauri/src/node.rs), on the network a K6 profile names, and proves:

  endpoint_identity  the profile's RPC serves the profile's chain id and block 0 (the K6 gate
                     RpcClient::verify_network applies the same rule before a signature);
  local_genesis      the node's own block 0 is the profile's genesis (a node on another chain is
                     refused and stopped; the K6 genesis gate keeps a data dir on one genesis);
  node_chain_id      the node reports the profile's chain id;
  reaches_target     the head reaches the target within --timeout: --blocks N (bounded), or the
                     network RPC's head less --tip-slack (to tip, the default);
  agrees             the node and the network RPC are on the same chain up to the anchor, the last
                     safe height (--lag below both heads): walking selected parents
                     (eth_getBlockByHash, parentHash) down from each side's head reaches the same
                     block there, and the node's walk goes on to the profile genesis. Block hashes
                     commit to their parent, so every height 0..anchor is then the same block on
                     both sides. eth_getBlockByNumber is not used for this: on r1006 it can name a
                     block off the selected chain, even deep below the tip (chain#421 class, DGX G4
                     fed#331). A block either side cannot serve on its walk fails the rule, and so
                     does a network walk longer than --walk-limit. On a mismatch the walks go on
                     down to the common ancestor and the first height above it is named;
  peers              the node had at least --min-peers peers;
  at_rest            the data dir is encrypted at rest (encryption.meta);
  node_alive         the node did not exit during the run;
  cleanup            the temp dir (data dir, key file, HOME) is gone afterwards.

What the node is given, and where it comes from (Rule 7):
  argv     --network <profile.nodeNetwork> --data-dir <temp> --config <temp>/node.toml, as
           build_spec (no --mine: a fresh node is a follower).
  config   src-tauri/config/member-node.toml with the profile's chain id, genesis profile and
           bootnodes and the temp data dir. The only change from the app's config: the RPC, WS and P2P ports are free ones,
           so a member node already running on this machine is never touched.
  env      the storage key FILE path (a fresh random key, owner-only file in the data dir), and
           the consensus env from the constants in node.rs (node_consensus_env): the validator
           activation height and the registry from the profile's address book (the app's
           crate::addresses) only when the genesis profile does not compile them
           (nodeGenesisProfiles.<name>.compiledActivations in src-tauri/network/profiles.json). HOME is a temp dir, the metrics
           port a free one, and nothing else of the caller's environment except PATH reaches it.

There is no option to use an existing data dir: every run is a cold start in a new temp dir.
SIGTERM, SIGHUP and SIGINT stop the node, remove the temp dir and write a FAIL verdict.

Usage
  cold-sync.py --node-bin <citrate> --out <dir> [--profiles src-tauri/network/profiles.json
      [--name <profile>] | --profile <one profile.json>] [--blocks N] [--timeout 30m]
      [--every 1000] [--lag 6] [--tip-slack 10] [--min-peers 1] [--rpc <url>]
      [--address-book <file>] [--expect-sha256 <hex>] [--walk-limit 50000]

Writes <out>/verdict.json (verdict PASS or FAIL, reasons, and one entry per rule with status ok,
FAIL or skip, the same shape as scripts/soak/soak-monitor.py) and <out>/node.log.tail.
Exit: 0 PASS · 1 FAIL · 2 refused before the node started (bad profile, address book, binary,
port or arguments). Python 3.9+, standard library only. Read-only towards the network: it sends
no transaction.
"""
import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from pathlib import Path

TOOL = "citrate-core cold-sync"
FORMAT = 1
GATE = "G4"
REPO = Path(__file__).resolve().parents[2]
SRC_TAURI = REPO / "src-tauri"
HASH_RE = re.compile(r"^0x[0-9a-f]{64}$")
ADDR_RE = re.compile(r"^0x[0-9a-fA-F]{40}$")
# <id>@<host>:<port>, nothing that could break out of a TOML string.
BOOTNODE_RE = re.compile(r"^[A-Za-z0-9_]+@[A-Za-z0-9.\-]+:[0-9]{1,5}$")
NETWORK_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
GENESIS_PROFILE_RE = re.compile(r"[a-z0-9_]+")
RPC_TIMEOUT_S = 10
# How long the cleanup waits to see whether a node it did not stop is exiting on its own.
EXIT_GRACE_S = 1.0
# Signals that stop a run the way SIGTERM does: the node is stopped, the temp dir (with the
# storage key) removed and a verdict written. SIGHUP is a closed terminal or a dropped SSH session.
STOP_SIGNALS = (signal.SIGTERM, signal.SIGHUP, signal.SIGINT)
RULES = ["profile", "launch", "node_binary", "endpoint_identity", "local_genesis", "node_chain_id",
         "reaches_target", "agrees", "peers", "at_rest", "node_alive", "cleanup"]


class Refused(Exception):
    """Input that must not reach a node: the run stops before anything starts (exit 2)."""

    def __init__(self, rule, msg):
        super().__init__(msg)
        self.rule = rule


class RpcError(Exception):
    pass


def log(msg):
    print(f"[cold-sync] {msg}", file=sys.stderr, flush=True)


def duration(s):
    m = re.fullmatch(r"\s*([0-9]+(?:\.[0-9]+)?)\s*([smh]?)\s*", str(s))
    if not m:
        raise argparse.ArgumentTypeError(f"not a duration: {s!r} (e.g. 90s, 30m, 6h)")
    return float(m.group(1)) * {"": 1, "s": 1, "m": 60, "h": 3600}[m.group(2)]


def public_url(url):
    """The URL without user info, query or fragment (never write a credential to the verdict)."""
    p = urllib.parse.urlsplit(url)
    host = p.hostname or ""
    if p.port:
        host += f":{p.port}"
    return urllib.parse.urlunsplit((p.scheme, host, p.path, "", ""))


def rpc(url, method, params=None, timeout=RPC_TIMEOUT_S):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or []}).encode()
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            doc = json.loads(resp.read())
    except Exception as e:  # noqa: BLE001 - any transport failure is "unreachable"
        raise RpcError(f"{method}: {type(e).__name__}: {e}") from None
    if not isinstance(doc, dict) or "error" in doc and doc["error"]:
        raise RpcError(f"{method}: error {doc.get('error') if isinstance(doc, dict) else doc!r}")
    return doc.get("result")


def rpc_int(url, method):
    r = rpc(url, method)
    if not isinstance(r, str) or not r.startswith("0x"):
        raise RpcError(f"{method}: not a quantity: {r!r}")
    return int(r, 16)


def block_hash(url, height):
    """The block hash at `height`, lowercase; None when the endpoint has no block there.

    By number: used only for block 0 (unique) and the head. It is not canonical on r1006."""
    b = rpc(url, "eth_getBlockByNumber", [hex(height), False])
    if b is None:
        return None
    h = b.get("hash") if isinstance(b, dict) else None
    if not isinstance(h, str) or not HASH_RE.match(h.lower()):
        raise RpcError(f"eth_getBlockByNumber({height}): no block hash in {b!r}")
    return h.lower()


def block_ref(b, what):
    """(height, hash, selected parent) of a block object; RpcError when it is not one."""
    if not isinstance(b, dict):
        raise RpcError(f"{what}: not a block: {b!r}")
    h, n = b.get("hash"), b.get("number")
    parent = b.get("selectedParentHash") or b.get("parentHash")
    sp, ph = b.get("selectedParentHash"), b.get("parentHash")
    if sp and ph and str(sp).lower() != str(ph).lower():
        raise RpcError(f"{what}: selectedParentHash {sp} != parentHash {ph}")
    if not (isinstance(h, str) and HASH_RE.match(h.lower()) and isinstance(n, str) and n.startswith("0x")
            and isinstance(parent, str) and HASH_RE.match(parent.lower())):
        raise RpcError(f"{what}: no number, hash and parent in {b!r}")
    return int(n, 16), h.lower(), parent.lower()


def head_block(url):
    """(height, hash, parent) of the endpoint's head (`latest`)."""
    return block_ref(rpc(url, "eth_getBlockByNumber", ["latest", False]), "eth_getBlockByNumber(latest)")


class Walk:
    """A selected-parent walk down from one side's head: height -> hash of each block it visits.

    `down_to(h)` extends it until the last visited block is at height <= h (never past block 0) and
    returns that block; `error` says where it stopped when a block could not be served."""

    def __init__(self, url, side, top, limit=None):
        self.url, self.side, self.limit = url, side, limit
        self.chain = {top[0]: top[1]}
        self.hashes = {top[1]}
        self.last = top
        self.steps = 0
        self.error = None

    def down_to(self, height):
        while self.last[0] > height and self.last[0] > 0:
            if self.limit is not None and self.steps >= self.limit:
                self.error = (f"the {self.side}'s selected-parent walk reached --walk-limit {self.limit} blocks at "
                              f"height {self.last[0]} before height {height}")
                return None
            b = rpc(self.url, "eth_getBlockByHash", [self.last[2], False])
            if b is None:
                self.error = (f"the {self.side}'s selected-parent walk stopped at height {self.last[0]}: "
                              f"its parent {self.last[2]} is not served")
                return None
            ref = block_ref(b, f"eth_getBlockByHash({self.last[2]})")
            if ref[1] != self.last[2] or ref[0] >= self.last[0]:
                self.error = (f"the {self.side}'s selected-parent walk stopped at height {self.last[0]}: "
                              f"the parent served is block {ref[1]} at height {ref[0]}")
                return None
            self.steps += 1
            self.chain[ref[0]] = ref[1]
            self.hashes.add(ref[1])
            self.last = ref
        return self.last


def ancestry(local, remote, anchor, genesis, limit):
    """Compare the node's and the network's selected-parent chains at `anchor` (see `agrees`).

    Returns a dict: anchor {height, hash}, walk {local_blocks, remote_blocks, reaches_genesis,
    error}, mismatch (None or {height, local, remote}) and common_ancestor (None or {height, hash})."""
    lw = Walk(local, "node", head_block(local))
    rw = Walk(remote, "network", head_block(remote), limit)
    out = {"anchor": None, "mismatch": None, "common_ancestor": None, "chain": lw.chain}

    def walk_info():
        return {"local_blocks": lw.steps, "remote_blocks": rw.steps,
                "reaches_genesis": lw.last[0] == 0 and lw.last[1] == genesis,
                "error": lw.error or rw.error}

    ra = rw.down_to(anchor)
    la = lw.down_to(anchor)
    if ra is None or la is None:
        out["walk"] = walk_info()
        return out
    out["anchor"] = {"height": la[0], "hash": la[1]}
    if la[:2] != ra[:2]:
        out["mismatch"] = {"height": la[0], "local": la[1], "remote": ra[1]}
        # Down both walks to the common ancestor: the first network block the node's chain has.
        lw.down_to(0)
        while ra is not None and ra[1] not in lw.hashes and ra[0] > 0:
            ra = rw.down_to(ra[0] - 1)
        if ra is not None and ra[1] in lw.hashes:
            out["common_ancestor"] = {"height": ra[0], "hash": ra[1]}
            above = sorted(h for h in lw.chain if h > ra[0])
            if above:
                h = above[0]
                out["mismatch"] = {"height": h, "local": lw.chain[h], "remote": rw.chain.get(h)}
        out["walk"] = walk_info()
        return out
    lw.down_to(0)
    out["walk"] = walk_info()
    if lw.error is None and not out["walk"]["reaches_genesis"]:
        out["walk"]["error"] = (f"the node's selected-parent walk ends at block {lw.last[1]} (height {lw.last[0]}), "
                                f"not at the profile genesis {genesis}")
    return out


# ── inputs ───────────────────────────────────────────────────────────────────────────────────────


def load_json(path, rule, what):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError) as e:
        raise Refused(rule, f"cannot read {what} {path}: {e}") from None


def load_profile(a):
    """(name, profile, the document it came from) from --profile (one profile, or a profiles.json)
    or --profiles."""
    if a.profile and a.profiles:
        raise Refused("profile", "pass --profile or --profiles, not both")
    path = a.profile or a.profiles or str(SRC_TAURI / "network" / "profiles.json")
    doc = load_json(path, "profile", "the network profile")
    if isinstance(doc, dict) and isinstance(doc.get("profiles"), dict):
        name = a.name or doc.get("active")
        if not isinstance(name, str) or name not in doc["profiles"]:
            raise Refused("profile", f"{path} has no profile {name!r} (known: {sorted(doc['profiles'])})")
        return name, doc["profiles"][name], doc
    if not isinstance(doc, dict):
        raise Refused("profile", f"{path} is not a profile object")
    return a.name or doc.get("name") or Path(path).stem, doc, None


def node_genesis_table(doc=None):
    """The genesis profiles the bundled node builds: `nodeGenesisProfiles` of the --profiles file when
    it has one, else of src-tauri/network/profiles.json (the app's, network_profile.rs)."""
    if isinstance(doc, dict) and isinstance(doc.get("nodeGenesisProfiles"), dict):
        return doc["nodeGenesisProfiles"]
    table = load_json(SRC_TAURI / "network" / "profiles.json", "profile", "the app's network profiles").get(
        "nodeGenesisProfiles")
    if not isinstance(table, dict) or not table:
        raise Refused("profile", "src-tauri/network/profiles.json has no nodeGenesisProfiles")
    return table


def check_profile(p, node_table):
    """The K6 readiness rule (network_profile.rs): every field set, no pending entry, and well
    formed. A profile with any null field is not ready and no node is started against it."""
    bad = []
    pending = p.get("pending")
    if pending:
        bad.append(f"pending: {sorted(pending) if isinstance(pending, dict) else pending}")
    cid = p.get("chainId")
    if not isinstance(cid, int) or isinstance(cid, bool) or cid <= 0:
        bad.append(f"chainId {cid!r} is not a positive integer")
    g = p.get("genesisHash")
    g = g.lower() if isinstance(g, str) else g
    if not isinstance(g, str) or not HASH_RE.match(g) or int(g, 16) == 0:
        bad.append(f"genesisHash {p.get('genesisHash')!r} is not 0x + 64 hex digits (not all zero)")
    gp = p.get("genesisProfile")
    entry = node_table.get(gp) if isinstance(gp, str) and GENESIS_PROFILE_RE.fullmatch(gp) else None
    if not isinstance(entry, dict):
        bad.append(f"genesisProfile {gp!r} is not a genesis profile the bundled node builds "
                   f"(it builds: {', '.join(sorted(node_table))})")
    net = p.get("nodeNetwork")
    if not isinstance(net, str) or not NETWORK_RE.match(net):
        bad.append(f"nodeNetwork {net!r} is not set")
    url = p.get("rpcUrl")
    if not isinstance(url, str) or urllib.parse.urlsplit(url).scheme not in ("http", "https"):
        bad.append(f"rpcUrl {url!r} is not an http(s) URL")
    nodes = p.get("bootnodes")
    if not isinstance(nodes, list) or not nodes:
        bad.append(f"bootnodes {nodes!r}: at least one is needed")
    else:
        for n in nodes:
            if not isinstance(n, str) or not BOOTNODE_RE.match(n):
                bad.append(f"bootnode {n!r} is not <key>@<host>:<port>")
    if bad:
        raise Refused("profile", "profile not ready: " + "; ".join(bad))
    return {"chainId": cid, "genesisHash": g, "genesisProfile": gp,
            "compiledActivations": entry.get("compiledActivations") is True, "nodeNetwork": net,
            "rpcUrl": url, "bootnodes": list(nodes), "addressBook": p.get("addressBook")}


def load_registry(book_path, genesis, chain_id):
    """The validator registry the app hands the node (crate::addresses), from the profile's book.
    The book must be for the profile's genesis and chain id, as the K6 gate requires."""
    book = load_json(book_path, "profile", "the address book")
    bg = str(book.get("genesisHash", "")).lower()
    if bg != genesis:
        raise Refused("profile", f"address book {book_path} is for genesis {bg or 'none'}, the profile is {genesis}")
    if book.get("chainId") != chain_id:
        raise Refused("profile", f"address book {book_path} is for chain {book.get('chainId')!r}, the profile is {chain_id}")
    reg = (book.get("addresses") or {}).get("ValidatorRegistry")
    if not isinstance(reg, str) or not ADDR_RE.match(reg):
        raise Refused("profile", f"address book {book_path} has no ValidatorRegistry")
    return reg


def rust_const(src, name):
    m = re.search(rf'const {name}: &str = "([^"]*)";', src)
    if not m:
        raise Refused("launch", f"src-tauri/src/node.rs no longer declares {name}; update cold-sync.py with it")
    return m.group(1)


def launch_constants(node_rs):
    """The sidecar launch constants, read from node.rs so this harness cannot drift from the app."""
    try:
        src = Path(node_rs).read_text()
    except OSError as e:
        raise Refused("launch", f"cannot read {node_rs}: {e}") from None
    if not re.search(r"fn node_validator_registry_value\(\)[^{]*\{\s*crate::addresses::validator_registry\(\)", src):
        raise Refused("launch", "node.rs no longer takes the validator registry from crate::addresses; update cold-sync.py")
    # The overrides are passed only without compiled activations (node.rs node_consensus_env); this
    # harness applies the same rule, so it refuses to guess if node.rs no longer states it.
    if not re.search(r"fn node_consensus_env\([^{]*\{[^}]*if !network\.is_some_and\(\|n\| n\.node_genesis\.compiled_activations\)", src):
        raise Refused("launch", "node.rs node_consensus_env no longer gates the validator overrides on "
                                "compiled_activations; update cold-sync.py")
    return {
        "key_file_env": rust_const(src, "NODE_STORAGE_KEY_FILE_ENV"),
        "key_file": rust_const(src, "NODE_STORAGE_KEY_FILE"),
        "consensus": [
            (rust_const(src, "NODE_BLOCK_V2_ENV"), rust_const(src, "NODE_BLOCK_V2_VALUE")),
        ],
        "activation": (rust_const(src, "NODE_VALIDATOR_ACTIVATION_HEIGHT_ENV"),
                       rust_const(src, "NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE")),
        "registry_env": rust_const(src, "NODE_VALIDATOR_REGISTRY_ENV"),
        "never_env": rust_const(src, "NODE_DAG_PRUNE_RETAIN_ENV"),
    }


def set_in_section(text, section, key, value):
    """Set `key = value` inside `[section]` of a TOML text; refuse when the key is not there."""
    m = re.search(rf"(?ms)^\[{re.escape(section)}\]\s*$(.*?)(?=^\[|\Z)", text)
    if not m:
        raise Refused("launch", f"member-node.toml has no [{section}] section")
    body, n = re.subn(rf"(?m)^{re.escape(key)}\s*=.*$", lambda _: f"{key} = {value}", m.group(1), count=1)
    if n != 1:
        raise Refused("launch", f"member-node.toml has no {key} in [{section}]")
    return text[: m.start(1)] + body + text[m.end(1):]


def toml_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def render_config(template, prof, data_dir, ports):
    """The app's member config (ensure_node_config) for this profile and data dir, on free ports."""
    t = template.replace("{{DATA_DIR}}", toml_str(str(data_dir))[1:-1])
    t = set_in_section(t, "chain", "chain_id", str(prof["chainId"]))
    t = set_in_section(t, "chain", "genesis_profile", toml_str(prof["genesisProfile"]))
    m = re.search(r"(?ms)^bootstrap_nodes\s*=\s*\[.*?^\]", t)
    if not m:
        raise Refused("launch", "member-node.toml has no bootstrap_nodes list")
    nodes = "bootstrap_nodes = [\n" + "".join(f"    {toml_str(n)},\n" for n in prof["bootnodes"]) + "]"
    t = t[: m.start()] + nodes + t[m.end():]
    p2p_host = re.search(r'(?ms)^\[network\].*?^listen_addr\s*=\s*"([^":]+):', t)
    t = set_in_section(t, "network", "listen_addr", toml_str(f"{p2p_host.group(1) if p2p_host else '0.0.0.0'}:{ports['p2p']}"))
    t = set_in_section(t, "rpc", "listen_addr", toml_str(f"127.0.0.1:{ports['rpc']}"))
    t = set_in_section(t, "rpc", "ws_addr", toml_str(f"127.0.0.1:{ports['ws']}"))
    # Read back what the node will be given: mining must stay off (a fresh node is a follower).
    mining = re.search(r"(?ms)^\[mining\].*?^enabled\s*=\s*(\w+)", t)
    if not mining or mining.group(1) != "false":
        raise Refused("launch", "member-node.toml does not keep [mining] enabled = false")
    if "{{" in t:
        raise Refused("launch", "member-node.toml has a placeholder this harness does not fill: "
                      + ", ".join(sorted(set(re.findall(r"\{\{[A-Z_]+\}\}", t)))))
    return t


def port_free(port):
    """True when nothing answers on 127.0.0.1:port and it can be bound on every interface."""
    with socket.socket() as c:
        c.settimeout(0.5)
        if c.connect_ex(("127.0.0.1", port)) == 0:
            return False
    for host in ("127.0.0.1", "0.0.0.0"):
        with socket.socket() as s:
            try:
                s.bind((host, port))
            except OSError:
                return False
    return True


def pick_ports(rpc_port):
    taken = set()

    def free():
        for _ in range(50):
            with socket.socket() as s:
                s.bind(("127.0.0.1", 0))
                p = s.getsockname()[1]
            if p not in taken and port_free(p):
                taken.add(p)
                return p
        raise Refused("launch", "could not find a free port")

    if rpc_port:
        if not port_free(rpc_port):
            raise Refused("launch", f"port {rpc_port} is in use; another node may be running there")
        taken.add(rpc_port)
    return {"rpc": rpc_port or free(), "ws": free(), "p2p": free(), "metrics": free()}


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


# ── the run ──────────────────────────────────────────────────────────────────────────────────────


class Verdict:
    def __init__(self):
        self.rules = {r: ("skip", "not reached") for r in RULES}
        self.doc = {"tool": TOOL, "format": FORMAT, "gate": GATE}

    def set(self, rule, ok, detail):
        self.rules[rule] = ("ok" if ok else "FAIL", detail)
        if not ok:
            log(f"FAIL {rule}: {detail}")
        return ok

    def write(self, out):
        rules = [{"rule": r, "status": s, "detail": d} for r, (s, d) in self.rules.items()]
        fails = [f"{r['rule']}: {r['detail']}" for r in rules if r["status"] == "FAIL"]
        doc = {**self.doc, "verdict": "FAIL" if fails else "PASS", "reasons": fails, "rules": rules}
        out.mkdir(parents=True, exist_ok=True)
        (out / "verdict.json").write_text(json.dumps(doc, indent=2) + "\n")
        print(f"COLD-SYNC {doc['verdict']}")
        for r in rules:
            print(f"  {r['status']:4}  {r['rule']}: {r['detail']}")
        return doc


def stop(proc):
    if proc is None or proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        proc.terminate()
    try:
        proc.wait(timeout=15)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            proc.kill()
        proc.wait(timeout=10)


def run(a, v):
    out = Path(a.out)
    name, raw, doc = load_profile(a)
    prof = check_profile(raw, node_genesis_table(doc))
    book = a.address_book or (str(SRC_TAURI / prof["addressBook"]) if prof["addressBook"] else None)
    if not book:
        raise Refused("profile", "profile not ready: addressBook is not set (or pass --address-book)")
    registry = load_registry(book, prof["genesisHash"], prof["chainId"])
    remote = a.rpc or prof["rpcUrl"]
    v.doc["profile"] = {"name": name, "chainId": prof["chainId"], "genesisHash": prof["genesisHash"],
                        "genesisProfile": prof["genesisProfile"],
                        "compiledActivations": prof["compiledActivations"],
                        "nodeNetwork": prof["nodeNetwork"], "rpcUrl": public_url(remote),
                        "bootnodes": prof["bootnodes"], "addressBook": book}
    v.set("profile", True, f"{name}: chain {prof['chainId']}, genesis {prof['genesisHash']} "
                           f"(genesis profile {prof['genesisProfile']}), "
                           f"{len(prof['bootnodes'])} bootnodes, book genesis matches")

    consts = launch_constants(a.node_rs)
    try:
        template = Path(a.member_config).read_text()
    except OSError as e:
        raise Refused("launch", f"cannot read the member config {a.member_config}: {e}") from None
    ports = pick_ports(a.rpc_port)
    v.set("launch", True, f"node.rs launch constants and member-node.toml read; ports rpc {ports['rpc']} "
                          f"ws {ports['ws']} p2p {ports['p2p']} metrics {ports['metrics']}")

    # Absolute: the node runs with the temp dir as its working directory.
    binp = Path(os.path.abspath(a.node_bin))
    if not binp.is_file() or not os.access(binp, os.X_OK):
        raise Refused("node_binary", f"{binp} is not an executable file")
    digest = sha256_file(binp)
    if a.expect_sha256 and digest != a.expect_sha256.lower():
        raise Refused("node_binary", f"{binp} sha256 {digest} is not the expected {a.expect_sha256}")
    v.doc["node"] = {"bin": str(binp), "sha256": digest}
    v.set("node_binary", True, f"sha256 {digest}" + (" (as expected)" if a.expect_sha256 else ""))

    # Preflight: the network RPC is on the profile's chain (the K6 RPC gate).
    endpoint_ok = False
    try:
        rid = rpc_int(remote, "eth_chainId")
        rg = block_hash(remote, 0)
        if rid != prof["chainId"]:
            v.set("endpoint_identity", False, f"{public_url(remote)} serves chain {rid}, the profile is {prof['chainId']}")
        elif rg != prof["genesisHash"]:
            v.set("endpoint_identity", False, f"{public_url(remote)} block 0 is {rg}, the profile genesis is {prof['genesisHash']}")
        else:
            endpoint_ok = v.set("endpoint_identity", True, f"{public_url(remote)}: chain {rid}, block 0 {rg}")
    except RpcError as e:
        v.set("endpoint_identity", False, f"{public_url(remote)} unreachable: {e}")

    proc = tmp = None
    try:
        tmp = Path(tempfile.mkdtemp(prefix="citrate-cold-sync-", dir=a.work_root))
        data, home = tmp / "node", tmp / "home"
        v.doc["data_dir"] = str(data)
        data.mkdir(mode=0o700)
        home.mkdir(mode=0o700)
        key_path = data / consts["key_file"]
        fd = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(secrets.token_hex(32))
        cfg = data / "node.toml"
        cfg.write_text(render_config(template, prof, data, ports))
        args = [str(binp), "--network", prof["nodeNetwork"], "--data-dir", str(data), "--config", str(cfg)]
        env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(home), "TMPDIR": str(tmp),
               consts["key_file_env"]: str(key_path), "NO_COLOR": "1", "CLICOLOR": "0",
               **dict(consts["consensus"]), "CITRATE_METRICS_ADDR": f"127.0.0.1:{ports['metrics']}"}
        if not prof["compiledActivations"]:
            env[consts["activation"][0]] = consts["activation"][1]
            env[consts["registry_env"]] = registry
        assert consts["never_env"] not in env
        try:
            ver = subprocess.run([str(binp), "--version"], env=env, cwd=str(tmp), capture_output=True,
                                 text=True, timeout=30).stdout.strip().splitlines()
            v.doc["node"]["version"] = ver[0] if ver else None
        except (OSError, subprocess.TimeoutExpired):
            v.doc["node"]["version"] = None
        v.doc["node"]["args"] = args[1:]
        v.doc["node"]["env"] = sorted(k for k in env if k not in ("PATH", "HOME", "TMPDIR"))
        local = f"http://127.0.0.1:{ports['rpc']}"
        logf = open(tmp / "node.log", "wb")
        log(f"starting the node: {' '.join(args)}")
        try:
            proc = subprocess.Popen(args, env=env, cwd=str(tmp), stdout=logf, stderr=subprocess.STDOUT,
                                    stdin=subprocess.DEVNULL, start_new_session=True)
        except OSError as e:
            v.set("node_alive", False, f"the node could not be started: {e}")
            return
        v.doc["node"]["pid"] = proc.pid
        sync_node(a, v, prof, remote, local, proc, endpoint_ok, data)
    finally:
        # A second signal must not cut the cleanup short.
        for s in STOP_SIGNALS:
            signal.signal(s, signal.SIG_IGN)
        # Whether the node was still running when the harness came to stop it: a node that exited
        # on its own, even after the last check, did not survive the run (#524).
        exited = proc.poll() if proc is not None else None
        if exited is None and proc is not None and v.rules["node_alive"][0] == "skip":
            # A node that is exiting has not always been reaped yet: give it a moment (#526 review).
            try:
                exited = proc.wait(timeout=EXIT_GRACE_S)
            except subprocess.TimeoutExpired:
                exited = None
        stop(proc)
        if proc is not None and v.rules["node_alive"][0] == "skip":
            if exited is None:
                v.set("node_alive", True, "running until stopped by the harness")
            else:
                v.set("node_alive", False, f"the node exited on its own with code {exited} before the harness stopped it")
        if tmp is not None:
            try:
                lines = (tmp / "node.log").read_bytes().decode("utf-8", "replace").splitlines()[-200:]
                out.mkdir(parents=True, exist_ok=True)
                (out / "node.log.tail").write_text("\n".join(lines) + "\n")
            except OSError:
                pass
            shutil.rmtree(tmp, ignore_errors=True)
            gone = not tmp.exists()
            v.doc["data_dir_removed"] = gone
            v.set("cleanup", gone, f"temp dir {tmp} " + ("removed" if gone else "could NOT be removed"))


def sync_node(a, v, prof, remote, local, proc, endpoint_ok, data):
    # The node's block 0 must be the profile's genesis before anything else is believed.
    deadline = time.time() + a.genesis_wait
    lg, last_err = None, "no answer"
    while time.time() < deadline:
        if proc.poll() is not None:
            v.set("node_alive", False, f"the node exited with code {proc.returncode} before serving block 0")
            v.set("local_genesis", False, "the node exited before serving block 0")
            return
        try:
            lg = block_hash(local, 0)
            if lg:
                break
        except RpcError as e:
            last_err = str(e)
        time.sleep(min(a.poll, 1.0))
    v.doc["node"]["local_genesis"] = lg
    if not lg:
        v.set("local_genesis", False, f"the node served no block 0 within {a.genesis_wait:.0f}s ({last_err})")
        return
    if lg != prof["genesisHash"]:
        v.set("local_genesis", False, f"the node's block 0 is {lg}, the profile genesis is "
                                      f"{prof['genesisHash']}: wrong chain, refused and stopped")
        return
    v.set("local_genesis", True, f"block 0 {lg}")
    try:
        nid = rpc_int(local, "eth_chainId")
    except RpcError as e:
        v.set("node_chain_id", False, f"the node did not report a chain id: {e}")
        return
    if not v.set("node_chain_id", nid == prof["chainId"], f"node chain {nid}, profile {prof['chainId']}"):
        return
    if not endpoint_ok:
        v.rules["reaches_target"] = ("skip", "not attempted: the network RPC failed endpoint_identity")
        return

    mode = "bounded" if a.blocks is not None else "tip"
    v.doc["mode"] = mode
    start = time.time()
    deadline = start + a.timeout
    head = remote_head = start_head = None
    max_peers = 0
    last_log = 0.0
    reached = False

    while True:
        if proc.poll() is not None:
            v.set("node_alive", False, f"the node exited with code {proc.returncode} at head {head}")
            break
        try:
            head = rpc_int(local, "eth_blockNumber")
            start_head = head if start_head is None else start_head
            max_peers = max(max_peers, rpc_int(local, "net_peerCount"))
            remote_head = rpc_int(remote, "eth_blockNumber")
            target = a.blocks + a.lag if mode == "bounded" else remote_head - a.tip_slack
            if head >= target:
                reached = True
                break
        except RpcError as e:
            log(f"poll: {e}")
        now = time.time()
        if now - last_log >= 30:
            log(f"head {head} / network {remote_head}, peers {max_peers}, {now - start:.0f}s")
            last_log = now
        if now >= deadline:
            break
        time.sleep(a.poll)

    # Agreement by selected-parent ancestry (see `agrees`): from both heads down to the anchor, the
    # last safe height, then the node's walk on to block 0. A pass covers every height 0..anchor.
    checked, mismatches, walk, anchor, ancestor = [], [], None, None, None
    elapsed = time.time() - start
    if head is not None and remote_head is not None and proc.poll() is None:
        try:
            lh, rh = head_block(local), head_block(remote)
            head, remote_head = lh[0], rh[0]
            target_anchor = min(head, remote_head) - a.lag
            if target_anchor < 0:
                v.set("agrees", False, f"no height to compare: heads {head} and {remote_head} are within --lag {a.lag}")
            else:
                log(f"agreement: walking selected parents down to height {target_anchor}")
                res = ancestry(local, remote, target_anchor, prof["genesisHash"], a.walk_limit)
                walk, anchor, ancestor = res["walk"], res["anchor"], res["common_ancestor"]
                if res["mismatch"]:
                    mismatches.append(res["mismatch"])
                elif anchor and not walk["error"]:
                    checked = sorted({h for h in res["chain"] if h <= anchor["height"] and h % a.every == 0}
                                     | {anchor["height"]})
        except RpcError as e:
            v.set("agrees", False, f"agreement check could not read a block: {e}")
    v.doc["sync"] = {"mode": mode, "target": a.blocks if mode == "bounded" else
                     (remote_head - a.tip_slack if remote_head is not None else None),
                     "start_head": start_head, "final_head": head,
                     "remote_head": remote_head, "elapsed_s": round(elapsed, 1),
                     "blocks_per_min": round((head - (start_head or 0)) / elapsed * 60, 1) if head and elapsed > 0 else None,
                     "every": a.every, "lag": a.lag, "anchor": anchor, "walk": walk,
                     "checked_heights": checked, "mismatches": mismatches, "common_ancestor": ancestor,
                     "max_peers": max_peers}
    if mismatches:
        m = mismatches[0]
        where = (f"common ancestor at height {ancestor['height']}, first height that disagrees {m['height']}"
                 if ancestor else f"no common ancestor found below height {m['height']}")
        v.set("agrees", False, f"height {m['height']}: node {m['local']}, network {m['remote']} ({where})")
    elif walk and walk["error"]:
        v.set("agrees", False, walk["error"])
    elif v.rules["agrees"][0] != "FAIL":
        if anchor:
            v.set("agrees", True, f"selected-parent ancestry agrees at height {anchor['height']} (block {anchor['hash']}): "
                                  f"every height 0..{anchor['height']} is the same chain on the node and the network; "
                                  f"the node's walk reaches the profile genesis ({walk['local_blocks']} node and "
                                  f"{walk['remote_blocks']} network blocks walked)")
        else:
            v.set("agrees", False, "no height could be compared")
    if mode == "bounded":
        what = f"head {head} >= {a.blocks} + lag {a.lag}"
    else:
        what = f"head {head}, network head {remote_head} (slack {a.tip_slack})"
    if mismatches and not reached:
        v.rules["reaches_target"] = ("skip", f"stopped at {what}: the node is on another branch")
    else:
        v.set("reaches_target", reached,
              f"{what} in {elapsed:.0f}s" if reached else f"{what}: not reached in {a.timeout:.0f}s")
    v.set("peers", max_peers >= a.min_peers, f"max peers {max_peers} (need >= {a.min_peers})")
    enc = (data / "encryption.meta").is_file()
    v.set("at_rest", enc, "encryption.meta present" if enc else "no encryption.meta: the data dir is not encrypted")
    # Liveness after the last check (#524): the node must still be running and answering, or a node
    # that died during the final agreement reads would pass.
    if v.rules["node_alive"][0] == "skip":
        if proc.poll() is not None:
            v.set("node_alive", False, f"the node exited with code {proc.returncode} during the final checks")
        else:
            try:
                rpc_int(local, "eth_blockNumber")
            except RpcError as e:
                try:
                    code = proc.wait(timeout=2)
                    v.set("node_alive", False, f"the node exited with code {code} during the final checks")
                except subprocess.TimeoutExpired:
                    v.set("node_alive", False, f"the node stopped answering after the final checks: {e}")


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Cold-sync the node sidecar on a K6 network profile in a fresh temp dir (RC52 G4).")
    p.add_argument("--node-bin", required=True, help="the node sidecar binary (citrate)")
    p.add_argument("--out", required=True, help="where verdict.json and node.log.tail go")
    p.add_argument("--profiles", help="a profiles.json (default: src-tauri/network/profiles.json)")
    p.add_argument("--name", help="the profile in --profiles (default: its `active`)")
    p.add_argument("--profile", help="one profile object (or a profiles.json) instead of --profiles")
    p.add_argument("--address-book", help="the address book (default: the profile's addressBook under src-tauri/)")
    p.add_argument("--rpc", help="the network RPC to compare with (default: the profile's rpcUrl)")
    p.add_argument("--blocks", type=int, help="bounded run: pass once the node is past this height (default: to tip)")
    p.add_argument("--timeout", type=duration, default=duration("30m"), help="sync time limit (30m)")
    p.add_argument("--every", type=int, default=1000, help="list every K-th height of the agreeing walk in the verdict (1000)")
    p.add_argument("--walk-limit", type=int, default=50000,
                   help="most network blocks the selected-parent walk may read (50000; about 0.4 s each on the public RPC); a longer walk fails `agrees`")
    p.add_argument("--lag", type=int, default=6, help="compare only heights this far below both heads (6)")
    p.add_argument("--tip-slack", type=int, default=10, help="to-tip: blocks behind the network head that count as at tip (10)")
    p.add_argument("--min-peers", type=int, default=1, help="peers the node must have had (1)")
    p.add_argument("--genesis-wait", type=duration, default=duration("120s"), help="time for the node to serve block 0 (120s)")
    p.add_argument("--poll", type=float, default=5.0, help="seconds between polls (5)")
    p.add_argument("--rpc-port", type=int, default=0, help="the node's RPC port (default: a free one; refused if in use)")
    p.add_argument("--expect-sha256", help="refuse a node binary with another sha256")
    p.add_argument("--work-root", help="where the temp dir is made (default: the system temp dir)")
    p.add_argument("--node-rs", default=str(SRC_TAURI / "src" / "node.rs"), help=argparse.SUPPRESS)
    p.add_argument("--member-config", default=str(SRC_TAURI / "config" / "member-node.toml"), help=argparse.SUPPRESS)
    a = p.parse_args(argv)
    if a.every < 1 or a.lag < 0 or a.tip_slack < 0 or (a.blocks is not None and a.blocks < 1) or a.walk_limit < 1:
        p.error("--every, --blocks and --walk-limit must be >= 1, --lag and --tip-slack >= 0")

    v = Verdict()
    def interrupted(*_):
        raise KeyboardInterrupt()

    for s in STOP_SIGNALS:
        signal.signal(s, interrupted)
    code = 0
    try:
        run(a, v)
    except Refused as e:
        v.set(e.rule, False, str(e))
        code = 2
    except KeyboardInterrupt:
        v.set("node_alive", False, "the harness was interrupted")
        code = 1
    except Exception as e:  # noqa: BLE001 - any harness error still ends in a FAIL verdict
        v.set("launch", False, f"harness error: {type(e).__name__}: {e}")
        code = 1
    doc = v.write(Path(a.out))
    return code or (0 if doc["verdict"] == "PASS" else 1)


if __name__ == "__main__":
    sys.exit(main())
