#!/usr/bin/env python3
"""Mutation check for DeployGate.tla (HUP-S6.4, the D-4 deploy gate).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant (plus
TypeOK) and expects TLC to report a violation. The spec in this directory is never modified:
mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/DeployGate_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-deploygate-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "DeployGate.tla")).read()

def cfg(target):
    s = open(os.path.join(FORMAL, "DeployGate.cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\nINVARIANT {target}\n"

M = [
 # Open re-reads the source instead of the bytes Check read (a second parse).
 ("M01", "NoTOCTOU",
  [("    /\\ code' = [code EXCEPT ![i] = held[i]]\n", "    /\\ code' = [code EXCEPT ![i] = src]\n")]),
 # Same mutant, seen through the per-hash READY invariant.
 ("M02", "OpenOnlyForReadyBytes",
  [("    /\\ code' = [code EXCEPT ![i] = held[i]]\n", "    /\\ code' = [code EXCEPT ![i] = src]\n")]),
 # A NOT_READY evaluation does not reject the open ceremonies for that code.
 ("M03", "NotReadyNeverSigns",
  [("                 IF v = \"NOT_READY\" /\\ cer[i] = \"pending\" /\\ code[i] = c\n",
    "                 IF FALSE\n")]),
 # Evaluate ignores the store lock (lands between Check and Open).
 ("M04", "OpenOnlyForReadyBytes",
  [("Evaluate(c, v) ==\n    /\\ lock = NoLock\n", "Evaluate(c, v) ==\n    /\\ TRUE\n")]),
 # Check accepts any recorded verdict (NOT_READY deploys).
 ("M05", "NotReadyNeverSigns",
  [("    /\\ gate[src] = \"READY\"\n", "    /\\ gate[src] # \"none\"\n")]),
 # Check accepts a code with no record at all.
 ("M06", "NotReadyNeverSigns",
  [("    /\\ gate[src] = \"READY\"\n", "    /\\ gate[src] # \"NOT_READY\"\n")]),
 # Approve signs a ceremony that was already rejected (revocation ignored at signing).
 ("M07", "NotReadyNeverSigns",
  [("Approve(i) ==\n    /\\ cer[i] = \"pending\"\n", "Approve(i) ==\n    /\\ cer[i] \\in {\"pending\", \"rejected\"}\n")]),
]

def run(name, target, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "DeployGate.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "DeployGate.tla", "-config", "M.cfg"],
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
