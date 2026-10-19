#!/usr/bin/env python3
"""Run every TLC config of a spec and check it against the config's own EXPECT line.

Each `<Spec>.cfg` and `<Spec>_*.cfg` starts with one of:

    \\* EXPECT: pass                 TLC must report no error
    \\* EXPECT: violated <Name>      TLC must report exactly that invariant, action property
                                    or (sole) temporal property violated

Green configs say `pass`; mutant configs name the invariant their mutant must break; reachability
witness configs (RT-17) name the witness invariant that must be violated on the green constants.
A mutant that passes, or that fails on a different property, is a mismatch.

Usage: python3 src-tauri/formal/tlc_expect.py <Spec> [<Spec> ...] [--markdown]
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar)
Exit status is non-zero if any config does not match its expectation.
"""
import glob, os, re, shutil, subprocess, sys, tempfile

FORMAL = os.path.dirname(os.path.abspath(__file__))
JAVA = os.environ.get("JAVA") or next(
    (c for c in ("/opt/homebrew/opt/openjdk/bin/java", shutil.which("java")) if c and os.path.exists(c)),
    "java")
JAR = os.environ.get("TLA_JAR") or os.path.expanduser("~/.tla/tla2tools.jar")


def expectation(cfg_path):
    for line in open(cfg_path):
        m = re.match(r"\\\*\s*EXPECT:\s*(pass|violated\s+(\w+))\s*$", line.strip())
        if m:
            return ("pass", None) if m.group(1) == "pass" else ("violated", m.group(2))
    return (None, None)


def properties(cfg_path):
    return [l.split()[1] for l in open(cfg_path) if l.startswith("PROPERTY")]


def run(spec, cfg, meta):
    r = subprocess.run([JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
                        "-metadir", meta, f"{spec}.tla", "-config", f"{cfg}.cfg"],
                       cwd=FORMAL, capture_output=True, text=True)
    out = r.stdout + r.stderr
    states = re.findall(r"([\d,]+) distinct states found", out)
    depth = re.search(r"depth of the complete state graph search is (\d+)", out)
    if "Model checking completed. No error has been found." in out:
        got = ("pass", None)
    elif m := re.search(r"Error: Invariant (\w+) is violated", out):
        got = ("violated", m.group(1))
    elif m := re.search(r"Error: Action property (\w+) is violated", out):
        got = ("violated", m.group(1))
    elif "Temporal properties were violated" in out:
        props = properties(os.path.join(FORMAL, f"{cfg}.cfg"))
        got = ("violated", props[0] if len(props) == 1 else "<temporal: several PROPERTY lines>")
    else:
        tail = [l for l in out.splitlines() if l.startswith("Error")][:2]
        got = ("error", "; ".join(tail) or "no TLC verdict")
    return got, (states[-1] if states else "?"), (depth.group(1) if depth else "-")


def main(argv):
    markdown = "--markdown" in argv
    specs = [a for a in argv if not a.startswith("--")]
    if not specs:
        print(__doc__)
        return 2
    ok = True
    work = tempfile.mkdtemp(prefix="tlc-expect.")
    try:
        if markdown:
            print("| Config | Expected | TLC result | Distinct states | Depth | Match |")
            print("|---|---|---|---|---|---|")
        for spec in specs:
            cfgs = [os.path.join(FORMAL, f"{spec}.cfg")] + sorted(
                glob.glob(os.path.join(FORMAL, f"{spec}_*.cfg")))
            for path in cfgs:
                if not os.path.exists(path):
                    continue
                cfg = os.path.basename(path)[:-4]
                want = expectation(path)
                if want[0] is None:
                    print(f"{cfg}: no EXPECT line")
                    ok = False
                    continue
                got, states, depth = run(spec, cfg, os.path.join(work, cfg))
                match = got == want
                ok = ok and match
                show = lambda e: "no error" if e[0] == "pass" else f"{e[0]} {e[1]}"
                if markdown:
                    print(f"| `{cfg}` | {show(want)} | {show(got)} | {states} | {depth} | "
                          f"{'yes' if match else '**NO**'} |")
                else:
                    print(f"{'OK      ' if match else 'MISMATCH'} {cfg}: expected {show(want)}, "
                          f"got {show(got)} ({states} distinct states)")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
