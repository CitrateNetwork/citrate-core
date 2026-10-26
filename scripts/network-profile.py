#!/usr/bin/env python3
"""Fill and check src-tauri/network/profiles.json, the one network profile file (K6).

Two commands, both fail closed: nothing is written unless every check passes.

fill <profile> --genesis-hash 0x<64 hex> --address-book <file> --min-chain-rev <40 hex>
     [--replace] [--activate]

    Records the values a reroll produces, and only those: `genesisHash`, `addressBookSha256`
    (sha256 of the frozen canonical address book, the citrate-chain
    `contracts/addresses/<chainId>.json` exported at the reroll) and `minChainRev` (the
    citrate-chain rev the bundled node must contain). Their `pending` reasons are removed.
    Every other byte of the file is kept.

    Checks first: the profile exists and every other value of it is already set; the genesis
    hash is 0x + 64 hex and not zero; the chain rev is 40 hex and not zero; the address book is
    the canonical book (its required keys, every section a map of 0x + 40 hex addresses, its
    chainId equal to the profile's, every pin the app needs present: scripts/sync-addresses.py
    --check); a value already set is replaced only with --replace. After the fill the profile
    must be ready (the same rule as network_profile.rs `ready()`).

    --activate also makes the profile the one this build targets. A build is then releasable
    only after the embedded book is regenerated for that genesis (scripts/sync-addresses.py) and
    MIN_CHAIN_REV in scripts/build-sidecar.sh is re-pinned; `check --release` says what is left.

check [--release] [--require-active <profile>] [--canonical-book <file>]

    Every profile parses and its values are well formed where set, and `pending` names exactly
    the unset values. With --release (the RC build gate G0): the active profile has NO unset
    value, the embedded address book has its chain id and genesis, and build-sidecar.sh pins its
    minChainRev. --require-active fails unless the active profile is the one named.
    --canonical-book checks the active profile's addressBookSha256 against that file.

Paths default to this repo; --profiles, --app-book-dir and --sidecar exist for tests.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PROFILES = REPO / "src-tauri" / "network" / "profiles.json"
APP_BOOK_DIR = REPO / "src-tauri"
SIDECAR = REPO / "scripts" / "build-sidecar.sh"
SYNC = REPO / "scripts" / "sync-addresses.py"

# The order network_profile.rs `missing()` and profile.ts `FIELDS` use.
FIELDS = [
    "chainId",
    "genesisHash",
    "nodeNetwork",
    "rpcUrl",
    "explorerUrl",
    "faucetUrl",
    "bootnodes",
    "addressBook",
    "addressBookSha256",
    "minChainRev",
]
KNOWN = set(FIELDS) | {"label", "pending"}
# What a reroll produces; the only values `fill` writes.
FILLED = ["genesisHash", "addressBookSha256", "minChainRev"]

HASH = re.compile(r"^0x[0-9a-fA-F]{64}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
REV = re.compile(r"^[0-9a-f]{40}$")
ADDR = re.compile(r"^0x[0-9a-fA-F]{40}$")
NETWORK = re.compile(r"^[A-Za-z0-9_-]+$")
BOOTNODE = re.compile(r"^noise_[0-9a-fA-F]{64}@[A-Za-z0-9.-]+:([0-9]+)$")
# The canonical book's required keys (citrate-chain contracts/addresses/schema.json).
BOOK_REQUIRED = ["chainId", "chainName", "deployer", "contracts", "aaStack", "genesis"]
BOOK_SECTIONS = ["contracts", "aaStack", "genesis", "planned"]


class Refused(Exception):
    pass


def die(cmd: str, msg: str) -> None:
    print(f"network-profile {cmd}: REFUSED: {msg}", file=sys.stderr)
    sys.exit(1)


def load(path: Path) -> dict:
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError) as e:
        raise Refused(f"{path} cannot be read: {e}")
    if not isinstance(data, dict) or not isinstance(data.get("profiles"), dict):
        raise Refused(f"{path} has no profiles")
    if not isinstance(data.get("active"), str):
        raise Refused(f"{path} has no active profile")
    for pid, p in data["profiles"].items():
        if not isinstance(p, dict):
            raise Refused(f"profile {pid} is not an object")
        unknown = sorted(set(p) - KNOWN)
        if unknown:
            raise Refused(f"profile {pid} has unknown keys {unknown}")
        absent = [f for f in FIELDS + ["label"] if f not in p]
        if absent:
            raise Refused(f"profile {pid} lacks {absent} (an unset value is null, not absent)")
    return data


def unset(p: dict) -> list:
    return [f for f in FIELDS if p.get(f) is None]


def problems(p: dict) -> list:
    """Why a profile is not ready: every unset value, else every malformed one.

    The same rule as network_profile.rs `NetworkProfile::ready` and profile.ts `profileProblems`;
    scripts/fill-network-profile.test.mjs checks the three agree.
    """
    missing = unset(p)
    if missing:
        return [f"{f} not set" for f in missing]
    out = []
    cid = p["chainId"]
    if not isinstance(cid, int) or isinstance(cid, bool) or cid <= 0:
        out.append("chainId is not a chain id")
    g = p["genesisHash"]
    if not (isinstance(g, str) and HASH.match(g)) or set(g[2:]) == {"0"}:
        out.append("genesisHash is not a block hash")
    n = p["nodeNetwork"]
    if not (isinstance(n, str) and NETWORK.match(n)):
        out.append("nodeNetwork is malformed")
    for f in ("rpcUrl", "explorerUrl", "faucetUrl"):
        v = p[f]
        if not (isinstance(v, str) and v.startswith("https://") and len(v) > 8 and not re.search(r"\s", v)):
            out.append(f"{f} is not an https URL")
    b = p["bootnodes"]
    if not isinstance(b, list) or not b:
        out.append("bootnodes is empty")
    elif not all(isinstance(x, str) and (m := BOOTNODE.match(x)) and 0 < int(m.group(1)) <= 65535 for x in b):
        out.append("bootnodes has a malformed entry")
    if not (isinstance(p["addressBook"], str) and p["addressBook"]):
        out.append("addressBook is empty")
    d = p["addressBookSha256"]
    if not (isinstance(d, str) and SHA256.match(d)) or set(d) == {"0"}:
        out.append("addressBookSha256 is not a sha256 digest")
    r = p["minChainRev"]
    if not (isinstance(r, str) and REV.match(r)) or set(r) == {"0"}:
        out.append("minChainRev is not a commit")
    return out


def pending_problems(pid: str, p: dict) -> list:
    pending = p.get("pending") or {}
    if not isinstance(pending, dict):
        return [f"profile {pid}: pending is not an object"]
    out = []
    for f in unset(p):
        why = pending.get(f)
        if not isinstance(why, str) or not why.strip():
            out.append(f"profile {pid}: {f} is unset with no reason in pending")
    for f in pending:
        if p.get(f) is not None:
            out.append(f"profile {pid}: pending has a reason for {f}, which is set")
    return out


def set_problems(p: dict) -> list:
    """Malformed values among those that ARE set (an unset profile may still be shipped inactive)."""
    probe = dict(p)
    # Stand-ins only for the unset values, so problems() checks the set ones.
    stand_in = {
        "chainId": 1,
        "genesisHash": "0x" + "1" * 64,
        "nodeNetwork": "n",
        "rpcUrl": "https://x",
        "explorerUrl": "https://x",
        "faucetUrl": "https://x",
        "bootnodes": ["noise_" + "1" * 64 + "@h:1"],
        "addressBook": "x",
        "addressBookSha256": "1" * 64,
        "minChainRev": "1" * 40,
    }
    for f in unset(p):
        probe[f] = stand_in[f]
    return problems(probe)


def check_book(path: Path, chain_id: int, genesis: str) -> bytes:
    try:
        raw = path.read_bytes()
        book = json.loads(raw)
    except (OSError, ValueError) as e:
        raise Refused(f"address book {path} cannot be read: {e}")
    if not isinstance(book, dict):
        raise Refused(f"address book {path} is not an object")
    absent = [k for k in BOOK_REQUIRED if k not in book]
    if absent:
        raise Refused(f"address book {path} is not a canonical book: it lacks {absent}")
    bid = book["chainId"]
    if not isinstance(bid, int) or isinstance(bid, bool):
        raise Refused(f"address book chainId {bid!r} is not an integer")
    if bid != chain_id:
        raise Refused(f"address book is for chain {bid}, the profile is chain {chain_id}")
    if not (isinstance(book["deployer"], str) and ADDR.match(book["deployer"])):
        raise Refused(f"address book deployer {book['deployer']!r} is not an address")
    for s in BOOK_SECTIONS:
        if s not in book:
            continue
        sec = book[s]
        if not isinstance(sec, dict):
            raise Refused(f"address book section {s} is not a map")
        bad = [f"{s}.{k} = {v!r}" for k, v in sec.items() if not (isinstance(v, str) and ADDR.match(v))]
        if bad:
            raise Refused("not an address in the book: " + ", ".join(bad))
    if not book["contracts"]:
        raise Refused("address book has no contracts")
    if "genesisHash" in book and str(book["genesisHash"]).lower() != genesis:
        raise Refused(f"address book genesisHash {book['genesisHash']} != --genesis-hash {genesis}")
    # The pins the app needs, with its own duplicate and shape checks (no network: no --rpc).
    r = subprocess.run(
        [sys.executable, "-I", str(SYNC), "--book", str(path), "--genesis", genesis, "--check"],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        raise Refused("address book fails scripts/sync-addresses.py --check: " + (r.stderr or r.stdout).strip())
    return raw


def write_atomic(path: Path, data: dict) -> None:
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".profiles.", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(text)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def cmd_fill(a) -> None:
    path = Path(a.profiles)
    data = load(path)
    pid = a.profile
    if pid not in data["profiles"]:
        raise Refused(f"no profile {pid!r} in {path} (known: {', '.join(sorted(data['profiles']))})")
    p = data["profiles"][pid]

    if not isinstance(a.genesis_hash, str) or not HASH.match(a.genesis_hash):
        raise Refused(f"--genesis-hash must be 0x + 64 hex, got {a.genesis_hash!r}")
    genesis = a.genesis_hash.lower()
    if set(genesis[2:]) == {"0"}:
        raise Refused("--genesis-hash is zero")
    rev = a.min_chain_rev
    if not isinstance(rev, str) or not REV.match(rev.lower()):
        raise Refused(f"--min-chain-rev must be a full 40-hex commit, got {rev!r}")
    rev = rev.lower()
    if set(rev) == {"0"}:
        raise Refused("--min-chain-rev is zero")

    others = [f for f in unset(p) if f not in FILLED]
    if others:
        raise Refused(
            f"profile {pid} also has {others} unset; this fills only the reroll values {FILLED}"
        )
    chain_id = p["chainId"]
    raw = check_book(Path(a.address_book), chain_id, genesis)
    digest = hashlib.sha256(raw).hexdigest()

    new = {"genesisHash": genesis, "addressBookSha256": digest, "minChainRev": rev}
    already = [f for f in FILLED if p[f] is not None and p[f] != new[f]]
    if already and not a.replace:
        raise Refused(f"profile {pid} already has {already} set; pass --replace to overwrite")

    filled = dict(p)
    filled.update(new)
    filled["pending"] = {k: v for k, v in (p.get("pending") or {}).items() if k not in FILLED}
    left = problems(filled) + pending_problems(pid, filled)
    if left:
        raise Refused(f"profile {pid} would not be ready after the fill: {left}")

    # Rewrite only the reroll values (key order and every other value are kept).
    for f in FILLED:
        p[f] = new[f]
    p["pending"] = filled["pending"]
    if a.activate:
        data["active"] = pid
    write_atomic(path, data)
    print(f"network-profile fill: {pid}: genesisHash={genesis}")
    print(f"network-profile fill: {pid}: addressBookSha256={digest} ({a.address_book})")
    print(f"network-profile fill: {pid}: minChainRev={rev}")
    if a.activate:
        print(f"network-profile fill: {pid} is now the active profile.")
        print("  Next: scripts/sync-addresses.py --book <that book> --genesis " + genesis + " --rpc <rpc>")
        print("        set MIN_CHAIN_REV in scripts/build-sidecar.sh to " + rev)
        print("        scripts/check-network-profile.sh --release --require-active " + pid)


def cmd_check(a) -> None:
    path = Path(a.profiles)
    data = load(path)
    errors = []
    for pid, p in data["profiles"].items():
        errors += [f"profile {pid}: {e}" for e in set_problems(p)]
        errors += pending_problems(pid, p)
    active = data["active"]
    if active not in data["profiles"]:
        errors.append(f"active profile {active!r} does not exist")
    elif a.release:
        p = data["profiles"][active]
        miss = unset(p)
        if miss:
            errors.append(
                f"active profile {active} has unset values {miss}; a release build must not ship "
                "a profile that refuses to sign (fill it: scripts/fill-network-profile.sh)"
            )
        else:
            errors += [f"active profile {active}: {e}" for e in problems(p)]
            book_path = Path(a.app_book_dir) / p["addressBook"]
            try:
                book = json.loads(book_path.read_text())
                if book.get("chainId") != p["chainId"]:
                    errors.append(f"embedded book {book_path} is chain {book.get('chainId')}, profile is {p['chainId']}")
                if str(book.get("genesisHash", "")).lower() != str(p["genesisHash"]).lower():
                    errors.append(
                        f"embedded book {book_path} genesis {book.get('genesisHash')} != profile {p['genesisHash']} "
                        "(regenerate it: scripts/sync-addresses.py)"
                    )
            except (OSError, ValueError) as e:
                errors.append(f"embedded book {book_path} cannot be read: {e}")
            try:
                m = re.search(r'^MIN_CHAIN_REV="([0-9a-f]{40})"', Path(a.sidecar).read_text(), re.M)
                if not m:
                    errors.append(f"{a.sidecar} has no MIN_CHAIN_REV")
                elif m.group(1) != p["minChainRev"]:
                    errors.append(f"{a.sidecar} MIN_CHAIN_REV {m.group(1)} != profile minChainRev {p['minChainRev']}")
            except OSError as e:
                errors.append(f"{a.sidecar} cannot be read: {e}")
            if a.canonical_book:
                try:
                    d = hashlib.sha256(Path(a.canonical_book).read_bytes()).hexdigest()
                except OSError as e:
                    raise Refused(f"{a.canonical_book} cannot be read: {e}")
                if d != p["addressBookSha256"]:
                    errors.append(f"{a.canonical_book} sha256 {d} != profile addressBookSha256 {p['addressBookSha256']}")
    if a.require_active and active != a.require_active:
        errors.append(f"active profile is {active!r}, required {a.require_active!r}")
    if errors:
        for e in errors:
            print(f"network-profile check: FAIL: {e}", file=sys.stderr)
        sys.exit(1)
    inactive = [pid for pid, p in data["profiles"].items() if pid != active and unset(p)]
    note = f"; not set (inactive): {', '.join(inactive)}" if inactive else ""
    print(f"network-profile check: OK (active {active}{', release' if a.release else ''}{note})")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fill")
    f.add_argument("--profiles", default=str(PROFILES), help=argparse.SUPPRESS)
    f.add_argument("profile")
    f.add_argument("--genesis-hash", required=True)
    f.add_argument("--address-book", required=True)
    f.add_argument("--min-chain-rev", required=True)
    f.add_argument("--replace", action="store_true")
    f.add_argument("--activate", action="store_true")
    c = sub.add_parser("check")
    c.add_argument("--profiles", default=str(PROFILES), help=argparse.SUPPRESS)
    c.add_argument("--release", action="store_true")
    c.add_argument("--require-active")
    c.add_argument("--canonical-book")
    c.add_argument("--app-book-dir", default=str(APP_BOOK_DIR), help=argparse.SUPPRESS)
    c.add_argument("--sidecar", default=str(SIDECAR), help=argparse.SUPPRESS)
    a = ap.parse_args()
    try:
        cmd_fill(a) if a.cmd == "fill" else cmd_check(a)
    except Refused as e:
        die(a.cmd, str(e))


if __name__ == "__main__":
    main()
