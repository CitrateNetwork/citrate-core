#!/usr/bin/env python3
"""Mutation check for DaemonBudget.tla (HUP-S10.3, daemons inside a budget).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant (plus
TypeOK) and expects TLC to report a violation. The spec in this directory is never modified:
mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/DaemonBudget_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-daemonbudget-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "DaemonBudget.tla")).read()

def cfg(target):
    s = open(os.path.join(FORMAL, "DaemonBudget.cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\nINVARIANT {target}\n"

M = [
 # Claim ignores the daily run budget.
 ("M01", "RunsWithinCap", [("    /\\ runs[d] < MaxRuns\n", "    /\\ TRUE\n")]),
 # Claim ignores the daily token budget.
 ("M02", "NoEmptyRun", [("    /\\ tokens[d] < MaxTokens\n", "    /\\ TRUE\n")]),
 # The allowance is the per-run cap even when less of the day is left.
 ("M03", "AllowanceWithinDay", [("Min(PerRun, MaxTokens - tokens[d])", "PerRun")]),
 # The runner does not stop a run at its allowance (a run may use anything).
 ("M04", "TokensBounded", [("    /\\ used <= allowance[d] + Over\n", "    /\\ TRUE\n")]),
 # Claim ignores a run already in flight.
 ("M05", "OneInFlight", [("    /\\ running[d] = 0\n    /\\ runs[d] < MaxRuns", "    /\\ TRUE\n    /\\ runs[d] < MaxRuns")]),
 # Claim ignores a paused daemon.
 ("M06", "NoStartWhilePaused", [("    /\\ ~paused[d] /\\ ~allPaused\n", "    /\\ ~allPaused\n")]),
 # Claim ignores "pause all".
 ("M07", "NoStartWhilePaused", [("    /\\ ~paused[d] /\\ ~allPaused\n", "    /\\ ~paused[d]\n")]),
 # A declined effect runs anyway.
 ("M08", "EffectsOnlyApproved", [("IF verdict = \"approved\" THEN executed \\cup {e.id} ELSE executed", "executed \\cup {e.id}")]),
 # A proposed effect runs at once (an automatic path).
 ("M09", "EffectsOnlyApproved", [("    /\\ nextId' = nextId + 1\n    /\\ UNCHANGED <<due, paused, allPaused, running, allowance, runs, tokens, spent,\n                   startedWhilePaused, executed, day>>",
                                   "    /\\ nextId' = nextId + 1\n    /\\ executed' = executed \\cup {nextId}\n    /\\ UNCHANGED <<due, paused, allPaused, running, allowance, runs, tokens, spent,\n                   startedWhilePaused, day>>")]),
 # A run spends (a spend path appears).
 ("M10", "SpendZero", [("    /\\ runs' = [runs EXCEPT ![d] = runs[d] + 1]\n    /\\ startedWhilePaused'", "    /\\ runs' = [runs EXCEPT ![d] = runs[d] + 1]\n    /\\ spent' = [spent EXCEPT ![d] = 1]\n    /\\ startedWhilePaused'"),
                       ("    /\\ UNCHANGED <<paused, allPaused, tokens, spent, effects, executed, nextId, day>>", "    /\\ UNCHANGED <<paused, allPaused, tokens, effects, executed, nextId, day>>")]),
]

def run(name, target, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "DaemonBudget.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "DaemonBudget.tla", "-config", "M.cfg"],
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
