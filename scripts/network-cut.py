#!/usr/bin/env python3
"""Check and apply the network cut file, release/network-cut.json (0.5.3 CORE-N1/N2).

The cut file is the one place the values of a new genesis are written: block 0, the genesis
profile the node compiles, the bootnode keys, the canonical address book digest, the chain rev
the node and the app's chain crates are built from, the node's consensus fingerprint, and the
node bytes for macOS arm64, Linux x86_64 and Linux aarch64. Every other file that pins one of
them is written from it, so cutting the release is a fill of this file plus `apply`.

check [--release] [--root <dir>] [--cut-only]

    The file is well formed: known keys only; every value null or well formed; the bootnodes are
    exactly boot1-3.citrate.ai and rpc.citrate.ai on 30303 with distinct keys; the macOS node
    asset is named after the chain rev; `pending` names exactly the null values, each with a
    reason; the target and retired profiles exist in profiles.json.
    With --release: nothing is pending, and the cut is applied: profiles.json, build-sidecar.sh,
    components.json, runtime-deps.sha256, the two Cargo.toml files and Cargo.lock all carry its
    values. Unless --cut-only, it then runs scripts/check-network-profile.sh --release
    --require-active <profile> as well (the embedded address book and the rest of the K6 gate).

apply [--root <dir>]

    Refuses unless the file is complete (nothing pending, every value well formed) and the target
    profile has every other value set. Then writes, and writes nothing else:
      src-tauri/network/profiles.json   the target profile's values, `pending` emptied, `active`,
                                        its nodeGenesisProfiles entry, `retired` on each retired
      scripts/build-sidecar.sh          MIN_CHAIN_REV
      release/components.json           citrate-node source.rev, federation, the three artifacts
      src-tauri/runtime-deps.sha256     the macOS node pin line
      src-tauri/Cargo.toml, kit/Cargo.toml   the citrate-chain crate revs
    Cargo.lock is cargo's: the command to regenerate it is printed, and `check --release` fails
    until it is run. So is the embedded address book (scripts/sync-addresses.py).

Every path is relative to --root (default: this repo), so the tests run on a copy.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

CUT = "release/network-cut.json"
PROFILES = "src-tauri/network/profiles.json"
COMPONENTS = "release/components.json"
RUNTIME_DEPS = "src-tauri/runtime-deps.sha256"
SIDECAR = "scripts/build-sidecar.sh"
CARGO_TOMLS = ["src-tauri/Cargo.toml", "kit/Cargo.toml"]
CARGO_LOCK = "Cargo.lock"

VALUES = [
    "genesisHash",
    "genesisProfile",
    "compiledActivations",
    "bootnodes",
    "addressBookSha256",
    "chainRev",
    "consensusFingerprint",
]
TARGETS = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
MAC = "aarch64-apple-darwin"
KEYS = {"_comment", "profile", "retire", "node", "pending"} | set(VALUES)
# The r1007 fleet's P2P entry points (owner decision: boot1-3 + rpc.citrate.ai).
BOOT_HOSTS = ["boot1.citrate.ai:30303", "boot2.citrate.ai:30303", "boot3.citrate.ai:30303", "rpc.citrate.ai:30303"]

HASH = re.compile(r"0x[0-9a-f]{64}")
SHA256 = re.compile(r"[0-9a-f]{64}")
REV = re.compile(r"[0-9a-f]{40}")
FINGERPRINT = re.compile(r"0x[0-9a-f]{32}")
GENESIS_PROFILE = re.compile(r"[a-z0-9_]+")
BOOTNODE = re.compile(r"noise_([0-9a-f]{64})@([a-z0-9.-]+:[0-9]+)")
CHAIN_DEP = re.compile(r'(citrate-chain\.git", rev = ")([0-9a-f]{40})(")')
LOCK_SRC = re.compile(r'source = "git\+https://github\.com/CitrateNetwork/citrate-chain\.git\?rev=([0-9a-f]{40})#([0-9a-f]{40})"')
MIN_REV_LINE = re.compile(r'^MIN_CHAIN_REV="([0-9a-f]{40})"$', re.M)


class Refused(Exception):
    pass


def nonzero(s: str) -> bool:
    return any(c not in "0x" for c in s)


def read_json(root: Path, rel: str) -> dict:
    try:
        return json.loads((root / rel).read_text())
    except (OSError, ValueError) as e:
        raise Refused(f"{rel} cannot be read: {e}")


def mac_asset(rev: str) -> str:
    return f"citrate-{MAC}-{rev[:12]}"


def value_problems(cut: dict, profiles: dict) -> list:
    """Malformed values among those that are set (null = pending, checked separately)."""
    out = []
    pid = cut.get("profile")
    others = {k: p for k, p in profiles.get("profiles", {}).items() if k != pid}
    node_table = profiles.get("nodeGenesisProfiles", {})
    g = cut.get("genesisHash")
    if g is not None:
        if not (isinstance(g, str) and HASH.fullmatch(g) and nonzero(g[2:])):
            out.append(f"genesisHash {g!r} is not a block hash (0x + 64 lowercase hex, not zero)")
        else:
            for oid, o in others.items():
                if isinstance(o.get("genesisHash"), str) and o["genesisHash"].lower() == g:
                    out.append(f"genesisHash {g} is already the genesis of profile {oid}")
    gp = cut.get("genesisProfile")
    if gp is not None:
        if not (isinstance(gp, str) and GENESIS_PROFILE.fullmatch(gp)):
            out.append(f"genesisProfile {gp!r} is not a genesis profile name")
        elif gp in node_table and isinstance(g, str) and node_table[gp].get("genesisHash") != g:
            out.append(f"genesisProfile {gp} is already in nodeGenesisProfiles with another block 0")
    ca = cut.get("compiledActivations")
    if ca is not None and not isinstance(ca, bool):
        out.append("compiledActivations is true or false")
    b = cut.get("bootnodes")
    if b is not None:
        ms = [BOOTNODE.fullmatch(x) if isinstance(x, str) else None for x in b] if isinstance(b, list) else [None]
        if not all(ms):
            out.append("bootnodes has a malformed entry (noise_<64 lowercase hex>@<host>:<port>)")
        else:
            hosts = sorted(m.group(2) for m in ms)
            if hosts != sorted(BOOT_HOSTS):
                out.append(f"bootnodes must be exactly {BOOT_HOSTS}, got {hosts}")
            if len({m.group(1) for m in ms}) != len(ms):
                out.append("bootnodes repeats a Noise key")
    d = cut.get("addressBookSha256")
    if d is not None and not (isinstance(d, str) and SHA256.fullmatch(d) and nonzero(d)):
        out.append("addressBookSha256 is not a sha256 digest (64 lowercase hex)")
    rev = cut.get("chainRev")
    if rev is not None and not (isinstance(rev, str) and REV.fullmatch(rev) and nonzero(rev)):
        out.append(f"chainRev {rev!r} is not a full commit (40 lowercase hex)")
        rev = None
    fp = cut.get("consensusFingerprint")
    if fp is not None and not (isinstance(fp, str) and FINGERPRINT.fullmatch(fp) and nonzero(fp[2:])):
        out.append(f"consensusFingerprint {fp!r} is not a consensus fingerprint (0x + 32 lowercase hex)")
    node = cut.get("node")
    if not isinstance(node, dict) or sorted(node) != sorted(TARGETS):
        out.append(f"node must have exactly the targets {TARGETS}")
        return out
    for t in TARGETS:
        a = node[t]
        if a is None:
            continue
        want = {"runtime_deps_asset", "sha256", "evidence"} if t == MAC else {"sha256", "evidence"}
        if not isinstance(a, dict) or set(a) != want:
            out.append(f"node.{t} must have exactly {sorted(want)}")
            continue
        if not (isinstance(a["sha256"], str) and SHA256.fullmatch(a["sha256"]) and nonzero(a["sha256"])):
            out.append(f"node.{t}.sha256 is not a sha256 digest")
        if not (isinstance(a["evidence"], str) and a["evidence"].strip()):
            out.append(f"node.{t}.evidence is empty (where and how these bytes were built)")
        if t == MAC and rev is not None and a["runtime_deps_asset"] != mac_asset(rev):
            out.append(f"node.{t}.runtime_deps_asset {a['runtime_deps_asset']!r} is not the build of chainRev "
                       f"(expected {mac_asset(rev)})")
    return out


def null_keys(cut: dict) -> list:
    keys = [k for k in VALUES if cut.get(k) is None]
    node = cut.get("node") if isinstance(cut.get("node"), dict) else {}
    return keys + [f"node.{t}" for t in TARGETS if node.get(t) is None]


def shape_problems(cut: dict, profiles: dict) -> list:
    out = []
    unknown = sorted(set(cut) - KEYS)
    absent = sorted(KEYS - {"_comment"} - set(cut))
    if unknown:
        out.append(f"unknown keys {unknown}")
    if absent:
        out.append(f"missing keys {absent} (a value not known yet is null, not absent)")
    pid = cut.get("profile")
    known = profiles.get("profiles", {})
    if pid not in known:
        out.append(f"profile {pid!r} is not in {PROFILES}")
    retire = cut.get("retire")
    if not isinstance(retire, dict):
        out.append("retire must map a profile to why it is retired")
    else:
        for r, why in retire.items():
            if r not in known or r == pid:
                out.append(f"retire names {r!r}, which is not another profile in {PROFILES}")
            if not (isinstance(why, str) and why.strip()):
                out.append(f"retire.{r} needs a reason")
    pending = cut.get("pending")
    if not isinstance(pending, dict):
        return out + ["pending must map a value to what sets it"]
    nulls = null_keys(cut)
    for k in nulls:
        why = pending.get(k)
        if not (isinstance(why, str) and why.strip()):
            out.append(f"{k} is null with no reason in pending")
    for k in pending:
        if k not in nulls:
            out.append(f"pending has a reason for {k}, which is set")
    return out + value_problems(cut, profiles)


def load(root: Path):
    cut = read_json(root, CUT)
    if not isinstance(cut, dict):
        raise Refused(f"{CUT} is not an object")
    return cut, read_json(root, PROFILES)


# ---------------------------------------------------------------------------------------------
# what the cut writes
# ---------------------------------------------------------------------------------------------

def applied_profiles(cut: dict, profiles: dict) -> dict:
    d = json.loads(json.dumps(profiles))
    pid = cut["profile"]
    p = d["profiles"][pid]
    p["genesisHash"] = cut["genesisHash"]
    p["genesisProfile"] = cut["genesisProfile"]
    p["bootnodes"] = list(cut["bootnodes"])
    p["addressBookSha256"] = cut["addressBookSha256"]
    p["minChainRev"] = cut["chainRev"]
    filled = {"genesisHash", "genesisProfile", "bootnodes", "addressBookSha256", "minChainRev"}
    p["pending"] = {k: v for k, v in (p.get("pending") or {}).items() if k not in filled}
    d["nodeGenesisProfiles"][cut["genesisProfile"]] = {
        "genesisHash": cut["genesisHash"],
        "nodeDefault": False,
        "compiledActivations": cut["compiledActivations"],
    }
    for r, why in cut["retire"].items():
        d["profiles"][r]["retired"] = why
    d["active"] = pid
    return d


def node_component(components: dict) -> dict:
    found = [c for c in components.get("components", []) if c.get("id") == "citrate-node"]
    if len(found) != 1:
        raise Refused(f"{COMPONENTS} has no single citrate-node component")
    return found[0]


def applied_components(cut: dict, components: dict) -> dict:
    d = json.loads(json.dumps(components))
    c = node_component(d)
    rev = cut["chainRev"]
    c["source"]["rev"] = rev
    c["federation"] = {
        "rev": rev,
        "status": "match",
        "note": f"Set by scripts/network-cut.py apply from {CUT} ({cut['profile']} genesis commit "
                f"{rev[:12]}). The same cut moves citrate-federation manifest.toml [repos.citrate-chain] and the "
                "citrate-core [[drift]] rows to this rev (Rule 12; docs/release/053-NETWORK.md).",
    }
    arts = {}
    for t in TARGETS:
        a = cut["node"][t]
        arts[t] = ({"runtime_deps_asset": a["runtime_deps_asset"], "evidence": a["evidence"]} if t == MAC
                   else {"sha256": a["sha256"], "evidence": a["evidence"]})
    c["artifacts"] = arts
    c.pop("pending", None)
    return d


def applied_runtime_deps(cut: dict, text: str) -> str:
    a = cut["node"][MAC]
    line = f"{a['sha256']}  {a['runtime_deps_asset']}"
    if line in text.split("\n"):
        return text
    if re.search(rf"^[0-9a-f]{{64}}  {re.escape(a['runtime_deps_asset'])}$", text, re.M):
        raise Refused(f"{RUNTIME_DEPS} already pins {a['runtime_deps_asset']} with another digest")
    head = text if text.endswith("\n") else text + "\n"
    return head + (f"# {cut['profile']} network cut (scripts/network-cut.py): citrate is citrate-chain "
                   f"{cut['chainRev'][:12]} (the {cut['profile']} genesis commit, MIN_CHAIN_REV).\n{line}\n")


def applied_sidecar(cut: dict, text: str) -> str:
    if len(MIN_REV_LINE.findall(text)) != 1:
        raise Refused(f"{SIDECAR} must have exactly one MIN_CHAIN_REV line")
    return MIN_REV_LINE.sub(f'MIN_CHAIN_REV="{cut["chainRev"]}"', text)


def applied_cargo(cut: dict, rel: str, text: str) -> str:
    if not CHAIN_DEP.search(text):
        raise Refused(f"{rel} has no citrate-chain git dependency")
    return CHAIN_DEP.sub(lambda m: m.group(1) + cut["chainRev"] + m.group(3), text)


def dump(d: dict) -> str:
    return json.dumps(d, indent=2, ensure_ascii=False) + "\n"


def plan(root: Path, cut: dict, profiles: dict) -> dict:
    """Every file the cut writes, as rel path -> new text. Raises Refused on any problem."""
    out = {PROFILES: dump(applied_profiles(cut, profiles))}
    out[COMPONENTS] = dump(applied_components(cut, read_json(root, COMPONENTS)))
    out[RUNTIME_DEPS] = applied_runtime_deps(cut, (root / RUNTIME_DEPS).read_text())
    out[SIDECAR] = applied_sidecar(cut, (root / SIDECAR).read_text())
    for rel in CARGO_TOMLS:
        out[rel] = applied_cargo(cut, rel, (root / rel).read_text())
    return out


def write_atomic(path: Path, text: str) -> None:
    mode = path.stat().st_mode & 0o777
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=f".{path.name}.", suffix=".tmp")
    try:
        os.fchmod(fd, mode)
        with os.fdopen(fd, "w") as f:
            f.write(text)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def complete_problems(cut: dict, profiles: dict) -> list:
    out = shape_problems(cut, profiles)
    for k in null_keys(cut):
        out.append(f"{k} is pending: {(cut.get('pending') or {}).get(k, 'no reason')}")
    if isinstance(cut.get("pending"), dict) and cut["pending"]:
        out.append(f"pending is not empty: {sorted(cut['pending'])}")
    return out


def cmd_apply(a) -> None:
    root = Path(a.root)
    cut, profiles = load(root)
    errors = complete_problems(cut, profiles)
    if errors:
        raise Refused("the cut is not complete; nothing was written:\n  " + "\n  ".join(errors))
    p = profiles["profiles"][cut["profile"]]
    other = [f for f in ("chainId", "nodeNetwork", "rpcUrl", "explorerUrl", "faucetUrl", "addressBook")
             if p.get(f) is None]
    if other:
        raise Refused(f"profile {cut['profile']} also has {other} unset; set them in {PROFILES} first")
    writes = plan(root, cut, profiles)
    left = applied_problems(root, cut, overrides=writes, lock=False)
    if left:
        raise Refused("the cut would not apply cleanly; nothing was written:\n  " + "\n  ".join(left))
    for rel, text in writes.items():
        if (root / rel).read_text() != text:
            write_atomic(root / rel, text)
            print(f"network-cut apply: wrote {rel}")
    print(f"network-cut apply: {cut['profile']} is now the active profile (chain rev {cut['chainRev']}).")
    print("  Next (each is checked by `scripts/network-cut.py check --release`):")
    print("    cargo update -p citrate-wallet-core -p citrate-commd   # Cargo.lock to the new chain rev")
    print(f"    scripts/sync-addresses.py --book <the canonical book> --genesis {cut['genesisHash']} --rpc <rpc>")
    print(f"    scripts/check-network-profile.sh --release --require-active {cut['profile']}")


def applied_problems(root: Path, cut: dict, overrides=None, lock=True) -> list:
    """Where the files under `root` (or `overrides`, rel -> text) do not carry the cut's values."""
    overrides = overrides or {}

    def text(rel):
        return overrides[rel] if rel in overrides else (root / rel).read_text()

    out = []
    pid, rev = cut["profile"], cut["chainRev"]
    try:
        prof = json.loads(text(PROFILES))
        if prof.get("active") != pid:
            out.append(f"{PROFILES}: active is {prof.get('active')!r}, the cut targets {pid!r}")
        p = prof["profiles"][pid]
        for k, want in (("genesisHash", cut["genesisHash"]), ("genesisProfile", cut["genesisProfile"]),
                        ("bootnodes", cut["bootnodes"]), ("addressBookSha256", cut["addressBookSha256"]),
                        ("minChainRev", rev)):
            if p.get(k) != want:
                out.append(f"{PROFILES}: {pid}.{k} is {p.get(k)!r}, the cut has {want!r}")
        if p.get("pending"):
            out.append(f"{PROFILES}: {pid} still has pending values {sorted(p['pending'])}")
        want_g = {"genesisHash": cut["genesisHash"], "nodeDefault": False,
                  "compiledActivations": cut["compiledActivations"]}
        if prof["nodeGenesisProfiles"].get(cut["genesisProfile"]) != want_g:
            out.append(f"{PROFILES}: nodeGenesisProfiles.{cut['genesisProfile']} is not {want_g}")
        for r in cut["retire"]:
            if not prof["profiles"].get(r, {}).get("retired"):
                out.append(f"{PROFILES}: {r} is not retired")
    except (KeyError, ValueError, TypeError, OSError) as e:
        out.append(f"{PROFILES} cannot be read: {e}")
    m = MIN_REV_LINE.findall(text(SIDECAR))
    if m != [rev]:
        out.append(f"{SIDECAR}: MIN_CHAIN_REV is {m}, the cut has {rev}")
    try:
        c = node_component(json.loads(text(COMPONENTS)))
        if c["source"]["rev"] != rev or c["federation"]["rev"] != rev:
            out.append(f"{COMPONENTS}: citrate-node rev {c['source']['rev']} / federation {c['federation']['rev']}, "
                       f"the cut has {rev}")
        for t in TARGETS:
            a, have = cut["node"][t], c.get("artifacts", {}).get(t, {})
            key = "runtime_deps_asset" if t == MAC else "sha256"
            if have.get(key) != a[key] or have.get("evidence") != a["evidence"]:
                out.append(f"{COMPONENTS}: citrate-node {t} is not the cut's pin")
        if c.get("pending"):
            out.append(f"{COMPONENTS}: citrate-node still has pending targets {sorted(c['pending'])}")
    except (KeyError, ValueError, TypeError, Refused) as e:
        out.append(f"{COMPONENTS} cannot be read: {e}")
    a = cut["node"][MAC]
    if f"{a['sha256']}  {a['runtime_deps_asset']}" not in text(RUNTIME_DEPS).split("\n"):
        out.append(f"{RUNTIME_DEPS}: no pin line for {a['runtime_deps_asset']}")
    for rel in CARGO_TOMLS:
        revs = sorted({m.group(2) for m in CHAIN_DEP.finditer(text(rel))})
        if revs != [rev]:
            out.append(f"{rel}: citrate-chain crate revs {revs}, the cut has {rev}")
    if lock:
        lock_revs = sorted({x for m in LOCK_SRC.finditer(text(CARGO_LOCK)) for x in m.groups()})
        if lock_revs != [rev]:
            out.append(f"{CARGO_LOCK}: citrate-chain sources at {lock_revs}, the cut has {rev} "
                       "(run: cargo update -p citrate-wallet-core -p citrate-commd)")
    return out


def cmd_check(a) -> None:
    root = Path(a.root)
    cut, profiles = load(root)
    errors = shape_problems(cut, profiles)
    nulls = null_keys(cut)
    if a.release:
        errors += [f"{k} is pending: {(cut.get('pending') or {}).get(k, 'no reason')}" for k in nulls]
        if not errors:
            errors += applied_problems(root, cut)
            if not a.cut_only:
                r = subprocess.run(["bash", str(root / "scripts" / "check-network-profile.sh"), "--release",
                                    "--require-active", cut["profile"]], capture_output=True, text=True)
                if r.returncode != 0:
                    errors += (r.stderr or r.stdout).strip().splitlines()
    if errors:
        for e in errors:
            print(f"network-cut check: FAIL: {e}", file=sys.stderr)
        sys.exit(1)
    state = f"{len(nulls)} value(s) pending: {', '.join(nulls)}" if nulls else "complete"
    print(f"network-cut check: OK ({cut['profile']}, {state}{', release' if a.release else ''})")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("--release", action="store_true")
    c.add_argument("--cut-only", action="store_true", help="skip the delegated check-network-profile.sh")
    c.add_argument("--root", default=str(REPO))
    p = sub.add_parser("apply")
    p.add_argument("--root", default=str(REPO))
    a = ap.parse_args()
    try:
        cmd_check(a) if a.cmd == "check" else cmd_apply(a)
    except Refused as e:
        print(f"network-cut {a.cmd}: REFUSED: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
