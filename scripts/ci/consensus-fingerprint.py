#!/usr/bin/env python3
# created: 2026-11-24 | branch: feat/059-windows
# author: Larry Klosowski + Claude Opus 5.5 (Windows lane, 0.5.9)
# status: active
"""Is a node built from another rev the same consensus as the base rev?

The node's consensus fingerprint (citrate-chain node/src/consensus_manifest.rs, fingerprint_of over
fingerprint_preimage, "citrate-consensus-v4") hashes git_sha together with every consensus-affecting
input. So a node built from the base rev plus cfg-only commits (Windows 0.5.9: citrate-chain 98c513ec
= 68434069 + #494) reports a different fingerprint even when its consensus is identical.

This script reads the JSON `citrate --config <node.toml> consensus --json` printed (log lines before
it are skipped), rebuilds the pre-image from it, and prints:

  reported     the fingerprint the binary printed
  own          the pre-image recomputed with the binary's own git_sha
  base         the same pre-image with git_sha replaced by the base rev's 12-character short sha

Exit 0 only when own == reported (the reconstruction is exact, so the pre-image is the one this
script mirrors) and, with --want, base == want (every input but git_sha equals the base rev's).
A change to the chain's pre-image makes own != reported, and this fails closed.
"""
import argparse
import hashlib
import json
import sys

OPTIONAL_HEIGHTS = [
    "agent_precompiles_height",
    "exact_create_code_height",
    "vrf_canonical_encoding_height",
    "supply_hardening_height",
    "storage_root_hardening_height",
    "mpt_state_root_height",
]


def load(path):
    raw = open(path, "rb").read().decode("utf-8-sig", errors="replace")
    start, end = raw.find("{"), raw.rfind("}")
    if start < 0 or end < start:
        raise SystemExit(f"consensus-fingerprint: no JSON object in {path}")
    return json.loads(raw[start : end + 1])


def preimage(d, git_sha):
    p = (
        "citrate-consensus-v3\n"
        f"git_sha={git_sha}\n"
        f"halo2_verifier={'true' if d['feat_halo2_verifier'] else 'false'}\n"
        f"commd_fold_verify={'gated' if d['feat_commd_fold_verify'] else 'absent'}\n"
        f"activation_gated={'|'.join(d['activation_gated'])}\n"
        f"pba_hardening_height={'unset' if d.get('pba_hardening_height') is None else d['pba_hardening_height']}\n"
        f"value_transfer_height={d['value_transfer_activation_height']}\n"
        f"create_nonce_fix_height={d['create_nonce_fix_activation_height']}\n"
        f"merge_depth_height={d['merge_depth_activation_height']}\n"
        f"base_fee={d['canonical_base_fee_per_gas']}\n"
        f"epoch={d['epoch']}\n"
        f"snapshot_lag={d['snapshot_lag']}\n"
    )
    for k in OPTIONAL_HEIGHTS:
        if d.get(k) is not None:
            p += f"{k}={d[k]}\n"
    if d.get("genesis_hash") and d.get("genesis_rules") is not None:
        p = "citrate-consensus-v4" + p[len("citrate-consensus-v3") :]
        p += f"genesis_hash={d['genesis_hash']}\ngenesis_rules={d['genesis_rules']}\n"
    return p


def fingerprint(d, git_sha):
    return "0x" + hashlib.sha256(preimage(d, git_sha).encode()).hexdigest()[:32]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("consensus_json")
    ap.add_argument("--base-sha", required=True, help="the base rev (full or 12+ hex); its first 12 characters are used")
    ap.add_argument("--want", help="the fingerprint the base rev reports (release/network-cut.json consensusFingerprint)")
    a = ap.parse_args()
    d = load(a.consensus_json)
    out = {
        "git_sha": d.get("git_sha"),
        "git_dirty": d.get("git_dirty"),
        "build_target": d.get("build_target"),
        "reported": d.get("fingerprint"),
        "own": fingerprint(d, d.get("git_sha", "")),
        "base_sha": a.base_sha[:12],
        "base": fingerprint(d, a.base_sha[:12]),
        "want": a.want,
    }
    out["reconstruction_exact"] = out["own"] == out["reported"]
    out["base_equivalent"] = a.want is None or out["base"] == a.want.lower()
    print(json.dumps(out, indent=2))
    if d.get("git_dirty"):
        print("consensus-fingerprint: the binary was built from a dirty tree", file=sys.stderr)
        return 1
    if not out["reconstruction_exact"]:
        print("consensus-fingerprint: the recomputed pre-image does not give the reported fingerprint "
              "(the chain's fingerprint_preimage changed; update this script from it)", file=sys.stderr)
        return 1
    if not out["base_equivalent"]:
        print(f"consensus-fingerprint: with git_sha {out['base_sha']} the fingerprint is {out['base']}, not {a.want}: "
              "this node is NOT the base rev's consensus", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
