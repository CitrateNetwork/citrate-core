#!/usr/bin/env python3
"""Mutation check for WebSigningBudget.tla (HUP-S2.3, ADR-2026-09-30 Rule-3 D9).

Each mutant breaks one guard of the spec, then runs TLC with only the target invariant or
property (plus TypeOK) and expects TLC to report a violation. The spec in this directory is
never modified: mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/WebSigningBudget_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, re, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-wsb-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "WebSigningBudget.tla")).read()

def cfg(base, target, kind="INVARIANT"):
    s = open(os.path.join(FORMAL, base + ".cfg")).read()
    s = "\n".join(l for l in s.splitlines() if not l.startswith(("INVARIANT", "PROPERTY")))
    return s + f"\nINVARIANT TypeOK\n{kind} {target}\n"

M = [
 ("M01", "OnlyClosedList", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.kind = "X402"\n', '    /\\ rq.kind \\in {"X402", "Permit"}\n')]),
 ("M02", "OriginBound", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.uri = rq.origin                       \\* D2 #8 URI binding\n', '')]),
 ("M03", "OriginBound", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.origin \\in Allowlist                  \\* D2 #4\n', ''),
   ('    /\\ s[1] = "web" => s[2] \\in Allowlist\n', '')]),
 ("M04", "TopFrameOnly", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.prov = "top"                          \\* D2 #1-3: attested top frame, managed browser\n', '')]),
 ("M05", "NonceUnique", "WebSigningBudget_Siwe", "INVARIANT",
  [('    /\\ rq.nonce \\notin ledger[rq.origin]        \\* D2 #12 nonce ledger\n', '')]),
 ("M06", "NoCapabilityDelegation", "WebSigningBudget", "INVARIANT",
  [('    /\\ ~rq.resource                             \\* D2 #17 no Resources / ReCap\n', '')]),
 ("M07", "RecipientPinned", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.to = rq.recip                         \\* recipient pinning\n', '')]),
 ("M08", "NeverExceedsCaps", "WebSigningBudget", "INVARIANT",
  [('    /\\ rq.value <= PerSigMax                    \\* per_signature_max\n', '')]),
 ("M09", "NeverExceedsCaps", "WebSigningBudget_X402", "INVARIANT",
  [('    /\\ budget[s].used < MaxCount\n', '')]),
 ("M10", "NeverExceedsCaps", "WebSigningBudget_X402", "INVARIANT",
  [('    /\\ RecipWin(rq.recip, clock) + rq.value <= PerRecipMax  \\* rolling per-recipient\n',
    '    /\\ SumSet({i \\in Idx(resv) : resv[i].kind = "X402" /\\ resv[i].key = rq.recip /\\ resv[i].t \\div W = clock \\div W}, [i \\in Idx(resv) |-> resv[i].v]) + rq.value <= PerRecipMax\n')]),
 ("M11", "NeverExceedsCaps", "WebSigningBudget_X402", "INVARIANT",
  [('    /\\ GlobalWin(clock) + rq.value <= GlobalMax              \\* rolling global\n', '')]),
 ("M12", "NeverExceedsCaps", "WebSigningBudget_Siwe", "INVARIANT",
  [('    /\\ SiweGapOk(rq.origin)                     \\* D2 #21 burst gap\n', '')]),
 ("M13", "RevokeImmediate", "WebSigningBudget", "INVARIANT",
  [('Revoke(s) ==\n    /\\ Up /\\ Idle\n', 'Revoke(s) ==\n    /\\ Up\n')]),
 ("M14", "ExpiredInert", "WebSigningBudget", "INVARIANT",
  [('    /\\ IF TaintOk(lock.rq) /\\ clock < budget[lock.slot].exp\n', '    /\\ IF TaintOk(lock.rq)\n')]),
 ("M15", "TaintDowngrade", "WebSigningBudget", "INVARIANT",
  [('    /\\ IF TaintOk(lock.rq) /\\ clock < budget[lock.slot].exp\n', '    /\\ IF clock < budget[lock.slot].exp\n')]),
 ("M16", "TaintDowngrade", "WebSigningBudget", "INVARIANT",
  [('    /\\ TaintOk(rq)                              \\* D2 #19\n', ''),
   ('    /\\ TaintOk(rq)                              \\* always strict for x402\n', ''),
   ('    /\\ IF TaintOk(lock.rq) /\\ clock < budget[lock.slot].exp\n', '    /\\ IF clock < budget[lock.slot].exp\n')]),
 ("M17", "TaintDowngrade", "WebSigningBudget_O3", "INVARIANT",
  [('       /\\ rq.kind = "Siwe"\n       /\\ taint \\subseteq {rq.origin}\n', '       /\\ taint \\subseteq Origins\n')]),
 ("M18", "RecordBeforeSignature", "WebSigningBudget", "INVARIANT",
  [('            /\\ records\' = Append(records, [slot |-> s, gen |-> budget[s].gen, status |-> "reserved"])\n',
    '            /\\ records\' = records\n'),
   ('    /\\ records\' = [records EXCEPT ![lock.rid].status = "signed"]\n',
    '    /\\ records\' = Append(records, [slot |-> lock.slot, gen |-> lock.gen, status |-> "signed"])\n')]),
 ("M19", "NoFalseNegative", "WebSigningBudget", "INVARIANT",
  [('IF i \\in open THEN [records[i] EXCEPT !.status = "outcome_unknown"]',
    'IF i \\in open THEN [records[i] EXCEPT !.status = "not_signed"]')]),
 ("M20", "NoDrop", "WebSigningBudget", "INVARIANT",
  [('       ELSE /\\ cnt\' = [cnt EXCEPT !.pending = @ + 1]\n            /\\ UNCHANGED <<budget, ledger, resv, records, lock>>\n',
    '       ELSE /\\ cnt\' = cnt\n            /\\ UNCHANGED <<budget, ledger, resv, records, lock>>\n')]),
 ("M21", "FallThroughLive", "WebSigningBudget_Live", "PROPERTY",
  [('Decide ==\n    /\\ Up /\\ Idle /\\ req # NoReq\n', 'Decide ==\n    /\\ Up /\\ Idle /\\ req # NoReq /\\ Eligible(req)\n')]),
 ("M22", "BudgetMonotone", "WebSigningBudget", "PROPERTY",
  [('    /\\ UNCHANGED <<clock, budget, ledger, resv, records, signed, revokes, taint>>\n\n\\* Every record still',
    '    /\\ budget\' = [s \\in Slots |-> [budget[s] EXCEPT !.used = 0]]\n    /\\ UNCHANGED <<clock, ledger, resv, records, signed, revokes, taint>>\n\n\\* Every record still')]),
]
only = set(sys.argv[1:])
failed = 0
for mid, target, base, kind, edits in M:
    if only and mid not in only: continue
    s = SPEC
    for old, new in edits:
        if s.count(old) != 1:
            print(f"{mid}: patch anchor not unique/found ({s.count(old)}): {old[:60]!r}")
            s = None
            break
        s = s.replace(old, new)
    if s is None:
        failed += 1
        continue
    d = os.path.join(OUT, mid); shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
    open(os.path.join(d, "WebSigningBudget.tla"), "w").write(s)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(base, target, kind))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "WebSigningBudget.tla", "-config", "M.cfg"],
                       cwd=d, capture_output=True, text=True)
    out = r.stdout + r.stderr
    open(os.path.join(d, "tlc.out"), "w").write(out)
    viol = re.findall(r"(Invariant \w+ is violated|Temporal properties were violated|Action property \w+ .*violated|Error: .*)", out)
    depth = re.findall(r"^State (\d+):", out, re.M)
    status = "CAUGHT" if any(target in v or "Temporal" in v or "Action property" in v for v in viol) else "SURVIVED"
    failed += status != "CAUGHT"
    print(f"{mid} {target:24s} cfg={base:24s} {status}  {viol[:1]}  trace_len={depth[-1] if depth else '?'}")
    shutil.rmtree(os.path.join(d, "meta"), ignore_errors=True)
shutil.rmtree(OUT, ignore_errors=True)
sys.exit(1 if failed else 0)
