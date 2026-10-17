#!/usr/bin/env python3
"""Fail unless every crate that depends on a zip older than 9 is tauri-plugin-updater.

RUSTSEC-2026-0367/0368/0369 are ignored in the CI audit step only because the updater's
Windows-only zip unpacks nothing but signature-verified update payloads. Any other crate pulling
an old zip would not be covered by that reasoning, so it must fail CI here.
"""
import json
import subprocess
import sys

ALLOWED = {"tauri-plugin-updater"}

meta = json.loads(
    subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
)
pkgs = {p["id"]: p for p in meta["packages"]}
old_zip = {
    pid
    for pid, p in pkgs.items()
    if p["name"] == "zip" and int(p["version"].split(".")[0]) < 9
}
bad = []
for node in meta["resolve"]["nodes"]:
    user = pkgs[node["id"]]["name"]
    for dep in node["deps"]:
        if dep["pkg"] in old_zip and user not in ALLOWED:
            bad.append(f"{user} -> zip {pkgs[dep['pkg']]['version']}")
if bad:
    print("zip < 9 pulled by a crate the audit ignore does not cover:", *bad, sep="\n  ")
    sys.exit(1)
print(f"zip < 9 users: only {sorted(ALLOWED)} ({len(old_zip)} old zip package(s))")
