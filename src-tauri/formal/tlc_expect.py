#!/usr/bin/env python3
"""Run every TLC config of a spec and check it against the config's own EXPECT line.

Each `<Spec>.cfg` and `<Spec>_*.cfg` starts with one of:

    \\* EXPECT: pass                 TLC must report no error
    \\* EXPECT: violated <Name>      TLC must report exactly that invariant, action property
                                    or temporal property violated

Green configs say `pass`; mutant configs name the invariant their mutant must break; reachability
witness configs (RT-17) name the witness invariant that must be violated on green constants.
A mutant that passes, or that fails on a different property, is a mismatch.

Two tripwires run with it (review round 2 on core#345):
  - the frame lint (tla_frame_lint.py): no action assigns a variable a sibling conjunct keeps
    UNCHANGED, which would make the action fire on only part of the states it was written for;
  - action coverage: every action TLC reports for the spec is taken in at least one green config,
    unless the module names it on a `\\* COVERAGE-EXEMPT: <Action> ...` line (mutant-only actions).

Usage: python3 src-tauri/formal/tlc_expect.py <Spec> [<Spec> ...] [--markdown]
       python3 src-tauri/formal/tlc_expect.py --self-test
Env:   JAVA (default: Homebrew openjdk, then `java`), TLA_JAR (default ~/.tla/tla2tools.jar),
       TLA_JAR_SHA256 (optional: refuse to run unless the jar has this digest; CI pins it).
Exit status is non-zero if any config does not match its expectation or a tripwire fires.
"""
import glob, hashlib, os, re, shutil, subprocess, sys, tempfile

FORMAL = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, FORMAL)
import tla_frame_lint  # noqa: E402

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


def properties(cfg_text):
    return [l.split()[1] for l in cfg_text.splitlines() if l.startswith("PROPERTY")]


def classify(out, cfg_text):
    """TLC output -> ("pass", None) | ("violated", Name) | ("error", why)."""
    if "Model checking completed. No error has been found." in out:
        return ("pass", None)
    if m := re.search(r"Error: Invariant (\w+) is violated", out):
        return ("violated", m.group(1))
    if m := re.search(r"Error: Action property (\w+) is violated", out):
        return ("violated", m.group(1))
    if m := re.search(r"Temporal property (\w+) was violated", out):
        return ("violated", m.group(1))
    if re.search(r"Temporal properties were violated", out):
        props = properties(cfg_text)
        return ("violated", props[0] if len(props) == 1 else "<temporal: several PROPERTY lines>")
    tail = [l for l in out.splitlines() if l.startswith("Error")][:2]
    return ("error", "; ".join(tail) or "no TLC verdict")


def coverage(out):
    """Action name -> states generated, from TLC's -coverage report (named actions only)."""
    cov = {}
    for m in re.finditer(r"^<(\w+) line \d+, col \d+ to line \d+, col \d+ of module \w+>: "
                         r"(\d+):(\d+)", out, flags=re.M):
        cov[m.group(1)] = max(cov.get(m.group(1), 0), int(m.group(3)))
    return cov


def exempt(spec):
    text = open(os.path.join(FORMAL, f"{spec}.tla")).read()
    return set(re.findall(r"\\\*\s*COVERAGE-EXEMPT:\s*(\w+)", text))


def run(spec, cfg, meta, with_coverage):
    cmd = [JAVA, "-XX:+UseParallelGC", "-cp", JAR, "tlc2.TLC", "-workers", "auto",
           "-metadir", meta, f"{spec}.tla", "-config", f"{cfg}.cfg"]
    if with_coverage:
        cmd += ["-coverage", "60"]
    r = subprocess.run(cmd, cwd=FORMAL, capture_output=True, text=True)
    out = r.stdout + r.stderr
    states = re.findall(r"([\d,]+) distinct states found", out)
    depth = re.search(r"depth of the complete state graph search is (\d+)", out)
    got = classify(out, open(os.path.join(FORMAL, f"{cfg}.cfg")).read())
    return got, (states[-1] if states else "?"), (depth.group(1) if depth else "-"), coverage(out)


SELF_TESTS = [
    # (name, TLC output, cfg text, expected classification)
    ("no error", "Model checking completed. No error has been found.\n", "", ("pass", None)),
    ("invariant", "Error: Invariant INV_X is violated.\n", "", ("violated", "INV_X")),
    ("action property", "Error: Action property X9 is violated.\n", "", ("violated", "X9")),
    ("temporal, TLC 2.19 wording", "Error: Temporal properties were violated.\n",
     "PROPERTY X11_QuitAlwaysTerminates\n", ("violated", "X11_QuitAlwaysTerminates")),
    ("temporal, nightly wording names the property",
     "Error: Temporal property X11_QuitAlwaysTerminates was violated.\n",
     "PROPERTY X11_QuitAlwaysTerminates\nPROPERTY L1_CloseTerminates\n",
     ("violated", "X11_QuitAlwaysTerminates")),
    ("temporal, old wording with several properties is not attributable",
     "Error: Temporal properties were violated.\n", "PROPERTY A\nPROPERTY B\n",
     ("violated", "<temporal: several PROPERTY lines>")),
    ("deadlock is an error", "Error: Deadlock reached.\n", "", ("error", "Error: Deadlock reached.")),
]


def self_test():
    ok = True
    for name, out, cfg_text, want in SELF_TESTS:
        got = classify(out, cfg_text)
        good = got == want
        ok = ok and good
        print(f"{'ok  ' if good else 'FAIL'} classify: {name}: got {got}")
    cov = coverage("<A line 1, col 1 to line 2, col 2 of module M>: 0:0\n"
                   "<B line 3, col 1 to line 4, col 2 of module M>: 2:7\n"
                   "<Next (502 8 502 85)>: 1:1\n")
    good = cov == {"A": 0, "B": 7}
    ok = ok and good
    print(f"{'ok  ' if good else 'FAIL'} coverage parse: got {cov}")
    return tla_frame_lint.self_test() and ok


def tlc_version():
    r = subprocess.run([JAVA, "-cp", JAR, "tlc2.TLC"], capture_output=True, text=True)
    m = re.search(r"TLC2 Version [^\n]*", r.stdout + r.stderr)
    return m.group(0) if m else "unknown TLC version"


def main(argv):
    if argv == ["--self-test"]:
        return 0 if self_test() else 1
    markdown = "--markdown" in argv
    specs = [a for a in argv if not a.startswith("--")]
    if not specs:
        print(__doc__)
        return 2
    pin = os.environ.get("TLA_JAR_SHA256")
    digest = hashlib.sha256(open(JAR, "rb").read()).hexdigest()
    if pin and digest != pin.lower():
        print(f"tla2tools.jar digest {digest} does not match TLA_JAR_SHA256 {pin}")
        return 2
    print(f"{tlc_version()}; jar sha256 {digest}")
    ok = True
    for spec in specs:
        lint = tla_frame_lint.lint_text(open(os.path.join(FORMAL, f"{spec}.tla")).read())
        for f in lint:
            print(f"FRAME LINT {spec}.tla: {f}")
        ok = ok and not lint
    work = tempfile.mkdtemp(prefix="tlc-expect.")
    try:
        if markdown:
            print("| Config | Expected | TLC result | Distinct states | Depth | Match |")
            print("|---|---|---|---|---|---|")
        for spec in specs:
            cfgs = [os.path.join(FORMAL, f"{spec}.cfg")] + sorted(
                glob.glob(os.path.join(FORMAL, f"{spec}_*.cfg")))
            taken = {}
            for path in cfgs:
                if not os.path.exists(path):
                    continue
                cfg = os.path.basename(path)[:-4]
                want = expectation(path)
                if want[0] is None:
                    print(f"{cfg}: no EXPECT line")
                    ok = False
                    continue
                got, states, depth, cov = run(spec, cfg, os.path.join(work, cfg),
                                              with_coverage=want[0] == "pass")
                for a, n in cov.items():
                    taken[a] = max(taken.get(a, 0), n)
                match = got == want
                ok = ok and match
                show = lambda e: "no error" if e[0] == "pass" else f"{e[0]} {e[1]}"
                if markdown:
                    print(f"| `{cfg}` | {show(want)} | {show(got)} | {states} | {depth} | "
                          f"{'yes' if match else '**NO**'} |")
                else:
                    print(f"{'OK      ' if match else 'MISMATCH'} {cfg}: expected {show(want)}, "
                          f"got {show(got)} ({states} distinct states)")
            never = sorted(a for a, n in taken.items() if n == 0 and a != "Init")
            never = [a for a in never if a not in exempt(spec)]
            for a in never:
                print(f"COVERAGE {spec}: action {a} is never taken in any green config")
            ok = ok and not never
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
