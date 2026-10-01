#!/usr/bin/env python3
"""Mutation check for ComponentSwap.tla (HUP-S5.5, the signed component updater).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant or
property (plus TypeOK) and expects TLC to report a violation. The spec in this directory is
never modified: mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/ComponentSwap_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile

FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-componentswap-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "ComponentSwap.tla")).read()


def cfg(target, kind):
    # The wide bound: three good versions, so pruning and the disk bound are reachable.
    s = open(os.path.join(FORMAL, "ComponentSwap_Wide.cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\n{kind} {target}\n"


M = [
    # Verify lets a tampered artifact through.
    ("M01", "CurrentVerified", "INVARIANT",
     [("    /\\ IF job.ver \\in Good\n", "    /\\ IF TRUE\n")]),
    ("M02", "OnlyVerifiedOnDisk", "INVARIANT",
     [("    /\\ IF job.ver \\in Good\n", "    /\\ IF TRUE\n")]),
    # Commit straight after Verify, before the tree is renamed into place.
    ("M03", "InstalledOnDisk", "INVARIANT",
     [("Commit ==\n    /\\ phase = \"renamed\"\n", "Commit ==\n    /\\ phase \\in {\"verified\", \"renamed\"}\n")]),
    # Pruning removes the old current instead of the old previous.
    ("M04", "InstalledOnDisk", "INVARIANT",
     [("                 THEN dirs \\ {prev}\n", "                 THEN dirs \\ {cur}\n")]),
    # Recovery removes the previous version too.
    ("M05", "InstalledOnDisk", "INVARIANT",
     [("    /\\ dirs' = {d \\in dirs : d = cur \\/ d = prev}\n", "    /\\ dirs' = {d \\in dirs : d = cur}\n")]),
    # Begin accepts an older manifest (no anti-rollback).
    ("M06", "SeenMonotone", "PROPERTY",
     [("    /\\ m.seq >= seen\n", "    /\\ TRUE\n")]),
    # Begin does not record the manifest it installs from.
    ("M07", "JobIsNewest", "INVARIANT",
     [("    /\\ seen' = m.seq\n", "    /\\ seen' = seen\n")]),
    # Commit never prunes: the disk grows without bound.
    ("M08", "BoundedDisk", "INVARIANT",
     [("    /\\ dirs' = IF prev # None /\\ prev # job.ver /\\ prev # cur\n", "    /\\ dirs' = IF FALSE\n")]),
    # Rollback to a previous version whose directory is gone (with recovery that forgets it).
    ("M09", "InstalledOnDisk", "INVARIANT",
     [("    /\\ prev \\in dirs\n    /\\ cur' = prev\n", "    /\\ cur' = prev\n"),
      ("    /\\ dirs' = {d \\in dirs : d = cur \\/ d = prev}\n", "    /\\ dirs' = {d \\in dirs : d = cur}\n")]),
]


def run(name, target, kind, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name)
    os.makedirs(d)
    open(os.path.join(d, "ComponentSwap.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target, kind))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "ComponentSwap.tla", "-config", "M.cfg"],
                       cwd=d, capture_output=True, text=True)
    word = "Invariant" if kind == "INVARIANT" else "Action property"
    killed = f"{word} {target} is violated" in r.stdout or (kind == "PROPERTY" and f"{target} is violated" in r.stdout)
    print(f"{name} {'KILLED  ' if killed else 'SURVIVED'} {target}")
    return killed


want = set(sys.argv[1:])
ok = True
for name, target, kind, patches in M:
    if want and name not in want:
        continue
    ok = run(name, target, kind, patches) and ok
shutil.rmtree(OUT, ignore_errors=True)
sys.exit(0 if ok else 1)
