#!/usr/bin/env python3
"""Mutation check for DeviceLinkShare.tla (HUP-S8.1 follow-on: DeviceLinks over the group relay).

Each mutant breaks one guard of the spec, then runs TLC on the MCDeviceLinkShare model with only the
target invariant (plus TypeOK) and expects a violation. The spec in this directory is never
modified: mutants are written to a temporary directory.

Usage: python3 src-tauri/formal/DeviceLinkShare_mutants.py [M01 M02 ...]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any mutant survives or a patch anchor no longer matches.
"""
import os, subprocess, sys, shutil, tempfile
FORMAL = os.path.dirname(os.path.abspath(__file__))
OUT = tempfile.mkdtemp(prefix="tlc-devicelinkshare-mutants.")
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)), "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")
SPEC = open(os.path.join(FORMAL, "DeviceLinkShare.tla")).read()
MC = open(os.path.join(FORMAL, "MCDeviceLinkShare.tla")).read()

def cfg(target):
    s = open(os.path.join(FORMAL, "MCDeviceLinkShare.cfg")).read()
    head = s.split("INVARIANTS")[0]
    return head + f"INVARIANT TypeOK\nINVARIANT {target}\n"

M = [
 # a revocation is accepted from whoever relays it (not only from its member).
 ("M01", "OnlyOthers",
  [("revs == {r \\in msg.revs : r[1] = msg.from /\\ ValidRev(r)}", "revs == {r \\in msg.revs : ValidRev(r)}")]),
 # a link is accepted from whoever relays it.
 ("M02", "OnlyOthers",
  [("links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l) /\\ l \\notin allRevs}",
    "links == {l \\in msg.links : ValidLink(l) /\\ l \\notin allRevs}")]),
 # link signatures are not checked.
 ("M03", "NoForgedLink",
  [("links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l) /\\ l \\notin allRevs}",
    "links == {l \\in msg.links : l[1] = msg.from /\\ l \\notin allRevs}")]),
 # revocation signatures are not checked.
 ("M04", "NoForgedRevocation",
  [("revs == {r \\in msg.revs : r[1] = msg.from /\\ ValidRev(r)}", "revs == {r \\in msg.revs : r[1] = msg.from}")]),
 # an old announcement re-adds a link after its revocation arrived.
 ("M05", "StoreNeverHoldsRevoked",
  [("links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l) /\\ l \\notin allRevs}",
    "links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l)}")]),
 # the daemon update does not drop revoked links AND the store keeps them. (Dropping only the
 # update's filter survives: the store invariant already keeps revoked links out, so that filter is
 # defence in depth; in Rust it also covers own-store revocations, unit-tested.)
 ("M06", "RevokedNeverSent",
  [("    ({<<n, d>> : d \\in ownLinks[n]} \\cup peerLinks[n]) \\ Revoked(n)",
    "    ({<<n, d>> : d \\in ownLinks[n]} \\cup peerLinks[n])"),
   ("peerLinks' = [peerLinks EXCEPT ![n] = (@ \\ allRevs) \\cup links]", "peerLinks' = [peerLinks EXCEPT ![n] = @ \\cup links]"),
   ("links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l) /\\ l \\notin allRevs}",
    "links == {l \\in msg.links : l[1] = msg.from /\\ ValidLink(l)}")]),
 # a node ingests its own announcements (its links come back through the relay).
 ("M07", "OnlyOthers",
  [("    /\\ msg.from # n\n", "")]),
 # a revocation in the store does not remove the stored link it revokes.
 ("M08", "StoreNeverHoldsRevoked",
  [("peerLinks' = [peerLinks EXCEPT ![n] = (@ \\ allRevs) \\cup links]", "peerLinks' = [peerLinks EXCEPT ![n] = @ \\cup links]")]),
]

def run(name, target, patches):
    spec = SPEC
    for a, b in patches:
        if a not in spec:
            print(f"{name}: ANCHOR MISSING: {a!r}")
            return False
        spec = spec.replace(a, b, 1)
    d = os.path.join(OUT, name); os.makedirs(d)
    open(os.path.join(d, "DeviceLinkShare.tla"), "w").write(spec)
    open(os.path.join(d, "MCDeviceLinkShare.tla"), "w").write(MC)
    open(os.path.join(d, "M.cfg"), "w").write(cfg(target))
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", os.path.join(d, "meta"), "MCDeviceLinkShare.tla", "-config", "M.cfg"],
                       cwd=d, capture_output=True, text=True)
    killed = f"Invariant {target} is violated" in r.stdout
    print(f"{name} {'KILLED  ' if killed else 'SURVIVED'} {target}", flush=True)
    return killed

want = set(sys.argv[1:])
ok = True
for name, target, patches in M:
    if want and name not in want:
        continue
    ok = run(name, target, patches) and ok
shutil.rmtree(OUT, ignore_errors=True)
sys.exit(0 if ok else 1)
