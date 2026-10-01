#!/usr/bin/env python3
"""Mutation check for SpendBudget.tla (HUP-S1.5, the escalation spend budget).

Each mutant breaks one guard of the spec, then runs TLC on SpendBudget_TwoEndpoints.cfg with only
the target invariant or property (plus TypeOK) and expects TLC to report that it is violated. The
spec in this directory is never modified: mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/SpendBudget_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-spendbudget-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "SpendBudget.tla")).read()
BASE = "SpendBudget_TwoEndpoints"

def cfg(target, kind):
    s = open(os.path.join(FORMAL, BASE + ".cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    # The target first, so a state that also breaks TypeOK is reported against the target. A
    # property mutant is checked alone: a broken state invariant found earlier in the search would
    # otherwise end the run before the transition that breaks the property.
    extra = "INVARIANT TypeOK\n" if kind == "INVARIANT" else ""
    return s + f"\n{kind} {target}\n{extra}"

M = [
 # RunBudget forgets the cap check.
 ("M01", "SpendWithinCap", "INVARIANT",
  [("    /\\ ~tainted\n    /\\ Fits(q)\n", "    /\\ ~tainted\n")]),
 # SetCap may lower the cap below what today already used.
 ("M02", "SpendWithinCap", "INVARIANT",
  [("    /\\ c >= committed + reserved\n", "")]),
 # Settle may charge more than the reservation.
 ("M03", "SpendWithinCap", "INVARIANT",
  [("    /\\ c \\in 0..r.amt\n", "    /\\ c \\in 0..(r.amt + 1)\n")]),
 # A run does not have to echo the shown price.
 ("M04", "NoEscalationWithoutShownPrice", "INVARIANT",
  [("    /\\ shown[q.id] = q.cost            \\* NoEscalationWithoutShownPrice\n", "")]),
 # Removing an endpoint keeps its quotes AND the run no longer re-checks the endpoint.
 ("M05", "EgressOptInOnly", "INVARIANT",
  [("    /\\ quotes' = {q \\in quotes : q.ep # e}\n    /\\ UNCHANGED <<now, period, cap, committed, reserved, shown,",
    "    /\\ UNCHANGED <<quotes, now, period, cap, committed, reserved, shown,"),
   ("    /\\ q.ep \\in added                  \\* EgressOptInOnly\n", "")]),
 # Untrusted context no longer forces confirmation.
 ("M06", "OverBudgetOrTaintedNeedsHic1", "INVARIANT",
  [("    /\\ ~tainted\n    /\\ Fits(q)\n", "    /\\ Fits(q)\n")]),
 # Over budget no longer forces confirmation.
 ("M07", "OverBudgetOrTaintedNeedsHic1", "INVARIANT",
  [("    /\\ ~tainted\n    /\\ Fits(q)\n", "    /\\ ~tainted\n")]),
 # A reservation from an earlier period is settled against today's counters.
 ("M08", "ReservedIsConsistent", "INVARIANT",
  [("    /\\ IF r.per = period /\\ r.mode = \"budget\"\n", "    /\\ IF r.mode = \"budget\"\n")]),
 # The clock moving backwards rolls the period (and resets spend).
 ("M09", "ResetOnlyAtPeriodBoundary", "PROPERTY",
  [("Roll ==\n    /\\ now > period\n", "Roll ==\n    /\\ now # period\n")]),
 # Changing the cap also resets today's spend.
 ("M10", "ResetOnlyAtPeriodBoundary", "PROPERTY",
  [("    /\\ cap' = c\n    /\\ UNCHANGED <<now, period, committed, reserved,",
    "    /\\ cap' = c\n    /\\ committed' = 0\n    /\\ UNCHANGED <<now, period, reserved,")]),
 # Same as M09, seen through period monotonicity.
 ("M11", "PeriodMonotone", "PROPERTY",
  [("Roll ==\n    /\\ now > period\n", "Roll ==\n    /\\ now # period\n")]),
]

def run(name, target, kind, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "SpendBudget.tla"), "w").write(spec)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target, kind))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "1",
                        "-metadir", os.path.join(d, "meta"), "SpendBudget.tla", "-config", "M.cfg"],
                       cwd=d, capture_output=True, text=True)
    killed = f"{target} is violated" in r.stdout
    print(f"{name} {'KILLED  ' if killed else 'SURVIVED'} {target}")
    if not killed:
        print("\n".join(l for l in r.stdout.splitlines() if "rror" in l or "violated" in l))
    return killed

want = set(sys.argv[1:])
ok = True
for name, target, kind, patches in M:
    if want and name not in want:
        continue
    ok = run(name, target, kind, patches) and ok
shutil.rmtree(OUT, ignore_errors=True)
sys.exit(0 if ok else 1)
