#!/usr/bin/env python3
"""Mutation check for FlRoundGate.tla (HUP-S9.4, federated round start + LoRA eval gate).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant (plus
TypeOK) and expects TLC to report a violation. The spec in this directory is never modified:
mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/FlRoundGate_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-flroundgate-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "FlRoundGate.tla")).read()

def cfg(target):
    s = open(os.path.join(FORMAL, "FlRoundGate.cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\nINVARIANT {target}\n"

M = [
 # Start without the member's approval.
 ("M01", "StartOnlyApproved",
  [("Start(p) ==\n    /\\ approved[p]\n", "Start(p) ==\n    /\\ TRUE\n")]),
 # Start without re-reading the coordinator (TOCTOU between plan and click).
 ("M02", "StartOnlyWhatWasApproved",
  [("    /\\ coord = \"open\"\n", "    /\\ TRUE\n")]),
 # Start under a settlement mode the member did not see.
 ("M03", "StartOnlyWhatWasApproved",
  [("    /\\ settle = seenSettle[p]\n", "    /\\ TRUE\n")]),
 # A second start for the same plan.
 ("M04", "AtMostOneStart",
  [("    /\\ started[p] = 0\n", "    /\\ TRUE\n")]),
 # Start a plan that was blocked when it was made.
 ("M05", "StartOnlyWhatWasApproved",
  [("    /\\ made[p] /\\ seenCoord[p] = \"open\"\n", "    /\\ made[p]\n")]),
 # Load accepts any recorded verdict.
 ("M06", "LoadedIsAccepted",
  [("    /\\ gateVerdict[v] = \"ACCEPT\"\n", "    /\\ gateVerdict[v] # \"none\"\n")]),
 # Load ignores the base model the scorecards measured.
 ("M07", "LoadedIsAccepted",
  [("    /\\ gateBase[v] = base\n    /\\ IF Refreshed", "    /\\ TRUE\n    /\\ IF Refreshed")]),
 # A new gate record never unloads the served adapter.
 ("M08", "LoadedIsAccepted",
  [("    /\\ IF loaded = v /\\ (verdict = \"REJECT\" \\/ b # base)\n", "    /\\ IF FALSE\n")]),
 # Only a REJECT unloads (the gap TLC found in the first draft).
 ("M09", "LoadedIsAccepted",
  [("    /\\ IF loaded = v /\\ (verdict = \"REJECT\" \\/ b # base)\n", "    /\\ IF loaded = v /\\ verdict = \"REJECT\"\n")]),
 # Switching the base model keeps the adapter.
 ("M10", "LoadedIsAccepted",
  [("    /\\ loaded' = IF b # base THEN NoAdapter ELSE loaded\n", "    /\\ loaded' = loaded\n")]),
 # An existing copy is reused without re-hashing it.
 ("M11", "ServedIsLoaded",
  [("Refreshed(v) == IF copy[v] = v THEN v", "Refreshed(v) == IF copy[v] # \"none\" THEN v"),
   ("          THEN /\\ copy' = [copy EXCEPT ![v] = v]\n",
    "          THEN /\\ copy' = [copy EXCEPT ![v] = IF copy[v] = \"none\" THEN v ELSE copy[v]]\n")]),
 # llama-server is pointed at the member's source file, not the app's copy.
 ("M12", "ServedIsLoaded",
  [("ELSE copy[loaded]\n", "ELSE content[gateSrc[loaded]]\n")]),
 # The copy is not re-hashed after it is made.
 ("M13", "ServedIsLoaded",
  [("    /\\ IF Refreshed(v) = v\n", "    /\\ IF TRUE\n"),
   ("          THEN /\\ copy' = [copy EXCEPT ![v] = v]\n", "          THEN /\\ copy' = [copy EXCEPT ![v] = Refreshed(v)]\n")]),
]

def run(name, target, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "FlRoundGate.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "FlRoundGate.tla", "-config", "M.cfg"],
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
