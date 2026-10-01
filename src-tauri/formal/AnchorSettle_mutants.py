#!/usr/bin/env python3
"""Mutation check for AnchorSettle.tla (HUP-S7.3 core half: batched day to anchored day).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant (plus
TypeOK) and expects TLC to report a violation. The spec in this directory is never modified:
mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/AnchorSettle_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-anchorsettle-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "AnchorSettle.tla")).read()

def cfg(target):
    s = open(os.path.join(FORMAL, "AnchorSettle.cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\nINVARIANT {target}\n"

M = [
 # settle marks a day anchored on any receipt (pending or reverted).
 ("M01", "AnchoredOnlyOnConfirmedReceipt",
  [("Settle(d) ==\n    /\\ receipt[d] = \"ok\"\n", "Settle(d) ==\n    /\\ receipt[d] # \"none\"\n")]),
 # approve signs without consuming the card (a second approval signs again).
 ("M02", "SingleUseCard",
  [("    /\\ card' = [card EXCEPT ![d] = \"consumed\"]\n    /\\ sigs'", "    /\\ card' = card\n    /\\ sigs'")]),
 # the setting can be turned on before the registry is deployed.
 ("M03", "NoSignatureBeforeDeploy",
  [("Enable ==\n    /\\ deployed /\\ ~enabled\n", "Enable ==\n    /\\ ~enabled\n"),
   ("Raise(d) ==\n    /\\ deployed /\\ enabled\n", "Raise(d) ==\n    /\\ enabled\n")]),
 # turning anchoring off keeps pending cards.
 ("M04", "NoPendingCardWhileOff",
  [("    /\\ card' = [d \\in Days |-> IF card[d] = \"pending\" THEN \"consumed\" ELSE card[d]]\n", "    /\\ card' = card\n")]),
 # same mutant, seen as a signature while off.
 ("M05", "NothingSignedWhileOff",
  [("    /\\ card' = [d \\in Days |-> IF card[d] = \"pending\" THEN \"consumed\" ELSE card[d]]\n", "    /\\ card' = card\n")]),
 # the scheduler raises a card for a day already anchored.
 ("M06", "NoCardAfterAnchored",
  [("    /\\ ~anchored[d]\n    /\\ card[d] # \"pending\"\n", "    /\\ card[d] # \"pending\"\n"),
   ("    /\\ receipt[d] \\in {\"none\", \"reverted\"}\n    /\\ cardNo[d] < MaxCards\n", "    /\\ receipt[d] # \"pending\"\n    /\\ cardNo[d] < MaxCards\n")]),
 # the scheduler raises a new card while the previous anchor's receipt is still pending.
 ("M07", "AtMostOneInFlight",
  [("    /\\ receipt[d] \\in {\"none\", \"reverted\"}\n    /\\ cardNo[d] < MaxCards\n", "    /\\ cardNo[d] < MaxCards\n")]),
]

def run(name, target, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "AnchorSettle.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "AnchorSettle.tla", "-config", "M.cfg"],
                       cwd=d, capture_output=True, text=True)
    killed = f"Invariant {target} is violated" in r.stdout
    print(f"{name} {'KILLED  ' if killed else 'SURVIVED'} {target}")
    return killed

want = set(sys.argv[1:])
ok = True
for name, target, patches in M:
    if want and name not in want:
        continue
    ok = run(name, target, patches) and ok
shutil.rmtree(OUT, ignore_errors=True)
sys.exit(0 if ok else 1)
