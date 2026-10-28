#!/usr/bin/env python3
# created: 2026-10-27 | author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
# status: active (K1-L harness tripwire; run by `linux-native-acceptance.sh --self-test`).
"""Structural check: every process the K1-L harness starts after the seal goes through CONFINE.

linux-native-acceptance.sh runs as PID 1 of the sealed namespace. After `seal`, every program it
starts (the app, an AppImage or its extraction, a package's install scripts, the display, bus,
keyring, feed server and every X, bus or Python helper) must be started through the
"${CONFINE[@]}" prefix, so it runs in the nested namespace with no capability and no_new_privs.

The check reads the inner phase of the script (from the "Inner phase" banner to the end) and
splits it into simple commands (quote, command-substitution, pipeline, case and heredoc aware).
A command is a launch when, unconfined, it:
  - runs a program by path or by variable ("$APP_EXE", "$W/x.AppImage", /usr/bin/x), or
  - runs an interpreter, a session program or anything that runs other code (python3, bash, sh,
    perl, busybox, env, Xvfb, dbus-*, gnome-keyring-daemon, nsenter, setpriv, setsid, unshare,
    command, builtin, eval, xargs, coproc, source, `.`, trap, time, stdbuf), find -exec, or dpkg
    with an install action, or
  - passes --appimage-extract, or
  - uses backticks, or
  - is started in the background with `&` (any program, a subshell or a group).
Exempt: `seal` and `mount_attr` (they build the seal; CONFINE does not exist yet), and `probe`
and the `p_*` probes (they check the seal from PID 1's own view, before anything is launched,
and two negative controls need PID 1's pre-seal handles). Even there, package code is flagged:
$APP_EXE, staged packages, the install roots, AppImages, --appimage-extract and dpkg installs.
Anywhere, any change to CONFINE after its one top-level definition is flagged.

Controls: for every confined launch found, the same file with that one CONFINE prefix removed
must be flagged at that line, and each synthetic snippet in SYNTHETIC must be judged as listed
(flagged, or for the must-pass ones not flagged). A check that cannot see a bypass fails
instead of passing.

Usage: confine-check.py [harness.sh]   (default: the linux-native-acceptance.sh next to this dir)
Exit 0 = every post-seal launch is confined and every control was flagged; 1 otherwise.
"""
import os
import re
import sys

CONFINE = '"${CONFINE[@]}"'
BANNER = re.compile(r"^#\s*Inner phase")
FUNC = re.compile(r"^\s*(?:function\s+([A-Za-z_][\w-]*)\s*(?:\(\s*\))?|([A-Za-z_][\w-]*)\s*\(\s*\))\s*\{?")
HEREDOC = re.compile(r"<<-?\s*'?\"?([A-Za-z_][A-Za-z0-9_]*)'?\"?")
ASSIGN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?\+?=")
KEYWORDS = {"if", "then", "else", "elif", "do", "while", "until", "!", "{", "}", "fi", "done"}
LAUNCHERS = {
    "python", "python3", "bash", "sh", "env", "exec", "Xvfb", "dbus-daemon", "dbus-send",
    "dbus-launch", "gnome-keyring-daemon", "nsenter", "setpriv", "setsid", "unshare", "nohup",
    "xdg-open", "timeout", "nice", "chroot",
    # Run another command or code given as text: always a launch when unconfined.
    "command", "builtin", "eval", "xargs", "coproc", "source", ".", "trap", "time", "stdbuf",
    "perl", "busybox",
}
FIND_EXEC = {"-exec", "-execdir", "-ok", "-okdir"}
# Package code: the app, staged packages, the install roots, AppImages and their extractions.
PACKAGE = re.compile(r"\$\{?APP_EXE\b|\$\{?W\}?/(pkgs|apps|root-deb|ref-)|\.AppImage\b|/usr/bin/citrate-core\b")
CONFINE_SET = re.compile(r"^CONFINE(\[[^]]*\])?\+?=")
CONFINE_ARG = re.compile(r"^CONFINE(\b|\[|=|\+=)")
SETTERS = {"unset", "local", "declare", "typeset", "readonly", "export", "read", "mapfile", "readarray"}
DPKG_INSTALL = re.compile(r"^(-i|--install|--unpack|--configure|-[a-zA-Z]*i[a-zA-Z]*)$")


def exempt(func):
    return func in ("seal", "mount_attr", "probe") or func.startswith("p_")


def logical_lines(text):
    """Inner-phase logical lines: (first line number, function, text). Comments, heredoc
    bodies and lines outside the inner phase are dropped; continuations are joined."""
    out, func, inner, heredoc, buf, start = [], "", False, None, "", 0
    for n, raw in enumerate(text.split("\n"), 1):
        if heredoc is not None:
            if raw.strip() == heredoc:
                heredoc = None
            continue
        if not inner:
            inner = bool(BANNER.match(raw))
            continue
        line = raw
        if not buf:
            start = n
            if line.lstrip().startswith("#"):
                continue
            m = FUNC.match(line)
            if m:
                func = m.group(1) or m.group(2)
        m = HEREDOC.search(line)
        if m and "<<<" not in line:
            heredoc = m.group(1)
        if line.endswith("\\"):
            buf += line[:-1] + " "
            continue
        joined = buf + line
        if "=(" in joined and joined.count("(") > joined.count(")"):  # array assignment goes on
            buf = joined + " "
            continue
        out.append((start, func, buf + line))
        buf = ""
        # A function ends at a `}` in column 0, or on its header line for a one-line body; the
        # code after it is top level again, not part of the function above.
        if line.rstrip() == "}" or (FUNC.match(line) and line.rstrip().endswith("}")):
            func = ""
    if not inner:
        raise SystemExit("confine-check: no 'Inner phase' banner found; refusing to pass")
    return out


class Seg:
    def __init__(self):
        self.words, self.cur, self.bg = [], "", False

    def end_word(self):
        if self.cur:
            self.words.append(self.cur)
            self.cur = ""


def split_commands(s):
    """Simple commands of one logical line: list of (words, background)."""
    segs = []
    i = 0

    def parse(i, stop):  # parse commands until `stop` (")" or None); returns index after it
        seg = Seg()
        case_state = []  # stack: "pat" (reading a case pattern) or "cmd"

        def close(bg=False):
            nonlocal seg
            seg.end_word()
            seg.bg = bg
            if seg.words:
                if seg.words[0] == "case":
                    case_state.append("pat")
                elif seg.words[0] == "esac" and case_state:
                    case_state.pop()
                segs.append((seg.words, seg.bg))
            seg = Seg()

        while i < len(s):
            c = s[i]
            two = s[i:i + 2]
            if c == "\\":
                seg.cur += s[i:i + 2]; i += 2; continue
            if c == "`":  # legacy command substitution: always a launch
                j = s.find("`", i + 1); j = len(s) if j < 0 else j + 1
                segs.append((["`…`"], False)); seg.cur += "`…`"; i = j; continue
            if c == "'":
                j = s.index("'", i + 1) if "'" in s[i + 1:] else len(s) - 1
                seg.cur += s[i:j + 1]; i = j + 1; continue
            if c == '"':
                i += 1; seg.cur += '"'
                while i < len(s) and s[i] != '"':
                    if s[i] == "\\":
                        seg.cur += s[i:i + 2]; i += 2; continue
                    if s[i] == "`":
                        j = s.find("`", i + 1); j = len(s) if j < 0 else j + 1
                        segs.append((["`…`"], False)); seg.cur += "`…`"; i = j; continue
                    if s[i:i + 3] == "$((":
                        j = s.find("))", i); j = len(s) if j < 0 else j + 2
                        seg.cur += s[i:j]; i = j; continue
                    if s[i:i + 2] == "$(":
                        i = parse(i + 2, ")"); seg.cur += "$(…)"; continue
                    if s[i:i + 2] == "${":
                        j = s.find("}", i); j = len(s) if j < 0 else j + 1
                        seg.cur += s[i:j]; i = j; continue
                    seg.cur += s[i]; i += 1
                seg.cur += '"'; i += 1; continue
            if s[i:i + 3] == "$((":
                j = s.find("))", i); j = len(s) if j < 0 else j + 2
                seg.cur += s[i:j]; i = j; continue
            if two in ("$(", "<(", ">("):
                i = parse(i + 2, ")"); seg.cur += "$(…)"; continue
            if two == "${":
                j = s.find("}", i); j = len(s) if j < 0 else j + 1
                seg.cur += s[i:j]; i = j; continue
            if c in " \t":
                seg.end_word()
                if seg.words[:1] == ["case"] and seg.words[-1] == "in":
                    close()
                i += 1; continue
            # redirections: >&2, 2>&1, <&-, &>file, >>file, {fd}<&-
            if c in "<>":
                j = i + 1
                while j < len(s) and s[j] in "<>&-0123456789":
                    j += 1
                seg.cur += s[i:j]; i = j; continue
            if two == "&>":
                seg.cur += "&>"; i += 2; continue
            if two in ("&&", "||", ";;"):
                close()
                if two == ";;" and case_state:
                    case_state[-1] = "pat"
                i += 2; continue
            if c == "&":
                close(bg=True); i += 1; continue
            if c in ";|\n":
                close(); i += 1; continue
            if c == "(" and seg.cur.endswith("="):  # array assignment: name=( ... ) / name+=( ... )
                # Its elements are words, not commands; a $(...) inside one is still a command.
                j, depth, q = i + 1, 1, None
                while j < len(s) and depth:
                    ch = s[j]
                    if q:
                        if ch == "\\":
                            j += 1
                        elif ch == q:
                            q = None
                        elif q == '"' and s[j:j + 2] == "$(":
                            j = parse(j + 2, ")"); continue
                    elif ch in "'\"":
                        q = ch
                    elif s[j:j + 2] == "$(":
                        j = parse(j + 2, ")"); continue
                    elif ch == "(":
                        depth += 1
                    elif ch == ")":
                        depth -= 1
                    j += 1
                seg.cur += "(…)"; i = j; continue
            if c == "(" and not seg.cur:
                if s[i:i + 2] == "((":  # arithmetic command
                    j = s.find("))", i); j = len(s) if j < 0 else j + 2
                    seg.cur += s[i:j]; i = j; continue
                close()
                j = parse(i + 1, ")")
                # a subshell started in the background: `( ... ) &`
                k = j
                while k < len(s) and s[k] in " \t":
                    k += 1
                if k < len(s) and s[k] == "&" and s[k:k + 2] != "&&":
                    segs.append((["(subshell)"], True))
                    j = k + 1
                i = j; continue
            if c == ")":
                if case_state and case_state[-1] == "pat":  # end of a case pattern
                    seg = Seg(); case_state[-1] = "cmd"; i += 1; continue
                if stop == ")":
                    close(); return i + 1
                # Unmatched at the top of a logical line: the end of a case pattern on its own
                # line, or of a subshell opened on an earlier line (`( ...` newline `... ) &`).
                close()
                k = i + 1
                while k < len(s) and s[k] in " \t":
                    k += 1
                if k < len(s) and s[k] == "&" and s[k:k + 2] != "&&":
                    segs.append((["(subshell)"], True))
                    k += 1
                i = k; continue
            seg.cur += c; i += 1
        close()
        return i

    parse(i, None)
    return segs


def program_of(words):
    """The program a simple command runs, after assignments and keywords; None if none."""
    k = 0
    while k < len(words) and (words[k] in KEYWORDS or ASSIGN.match(words[k])):
        k += 1
    if k >= len(words):
        return None, []
    return words[k], words[k + 1:]


def is_launch(prog, args, bg):
    if prog in ("local", "declare", "export", "readonly", "[", "[[", "test", "case", "esac",
                "for", "in", "return", "exit", "break", "continue", ":"):
        return bg  # builtins launch nothing (a backgrounded one still forks)
    if prog == "command" and args[:1] in (["-v"], ["-V"]):
        return False  # a lookup, runs nothing
    if prog == "exec" and (not args or re.match(r"^\{?[A-Za-z0-9_]*\}?[<>]", args[0])):
        return False  # fd redirection only
    if bg:
        return True
    if prog in ("(subshell)", "`…`"):
        return True
    if prog == "find" and any(a in FIND_EXEC for a in args):
        return True
    if any(a == "--appimage-extract" for a in args):
        return True
    if prog in LAUNCHERS:
        return True
    if prog == "dpkg" and any(DPKG_INSTALL.match(a) for a in args):
        return True
    p = prog.strip('"')
    if p.startswith("$") or p.startswith("/") or p.startswith("./") or p.startswith("~"):
        return True
    return False


def is_package_launch(prog, args):
    """Runs package code: the program is package code, or it extracts or installs a package, or
    an interpreter or launcher is handed package code."""
    if PACKAGE.search(prog) or "--appimage-extract" in args:
        return True
    if prog == "dpkg" and any(DPKG_INSTALL.match(a) for a in args):
        return True
    runs_args = prog in LAUNCHERS or prog.strip('"').startswith(("$", "/", "./", "~"))
    return runs_args and any(PACKAGE.search(a) for a in args)


def changes_confine(words):
    if any(CONFINE_SET.match(w) for w in words):
        return True
    prog, args = (words[0], words[1:]) if words else ("", [])
    return prog in SETTERS and any(CONFINE_ARG.match(a) for a in args)


def scan(text):
    """(launches, bypasses): launches = [(line, func, program)] through CONFINE;
    bypasses = [(line, func, command)] that start a process without it, change CONFINE after
    its definition, or (in an exempt function) start package code."""
    launches, bypasses = [], []
    defined = False
    for n, func, line in logical_lines(text):
        for words, bg in split_commands(line):
            if changes_confine(words):
                # Only its one definition, at top level, is allowed.
                if defined or func:
                    bypasses.append((n, func, "changes CONFINE: " + " ".join(words)))
                defined = True
                continue
            if exempt(func):
                prog, args = program_of(words)
                if prog is not None and prog != CONFINE and is_package_launch(prog, args):
                    bypasses.append((n, func, "package code in a probe: " + " ".join(words)))
                continue
            prog, args = program_of(words)
            if prog is None:
                if bg:  # a backgrounded `{ ...; }` group
                    bypasses.append((n, func, " ".join(words) + " &"))
                continue
            if prog == CONFINE:
                launches.append((n, func, args[0] if args else "?"))
                continue
            if is_launch(prog, args, bg):
                bypasses.append((n, func, " ".join(words) + (" &" if bg else "")))
    return launches, bypasses


# Synthetic controls: (name, snippet appended after the harness, must it be flagged?). A snippet
# is a whole function (or two) so it is scanned in that function's context.
def _fn(name, *body):
    return f"{name}() {{\n" + "\n".join("  " + b for b in body) + "\n}"


SYNTHETIC = [
    # Unconfined launches in an ordinary (non-probe) function: each must be flagged.
    ("background sleep", _fn("k1l_ctl", "sleep 300 </dev/null >/dev/null 2>&1 &"), True),
    ("command", _fn("k1l_ctl", "command python3 -I -c pass"), True),
    ("builtin", _fn("k1l_ctl", "builtin command python3 -I -c pass"), True),
    ("eval", _fn("k1l_ctl", 'eval "$K1L_CTL"'), True),
    ("backticks in an assignment", _fn("k1l_ctl", "K1L_CTL=`id`"), True),
    ("backticks in a string", _fn("k1l_ctl", 'echo "x `id` y"'), True),
    ("xargs", _fn("k1l_ctl", "printf '%s\\0' \"$W/x\" | xargs -0 sh"), True),
    ("coproc", _fn("k1l_ctl", "coproc python3 -I -c pass"), True),
    ("source", _fn("k1l_ctl", 'source "$W/ctl.sh"'), True),
    ("dot", _fn("k1l_ctl", '. "$W/ctl.sh"'), True),
    ("trap", _fn("k1l_ctl", "trap 'python3 -I -c pass' EXIT"), True),
    ("find -exec", _fn("k1l_ctl", 'find "$W" -name ctl -exec sh {} \\;'), True),
    ("find -execdir", _fn("k1l_ctl", 'find "$W" -name ctl -execdir sh {} +'), True),
    ("time", _fn("k1l_ctl", "time python3 -I -c pass"), True),
    ("stdbuf", _fn("k1l_ctl", "stdbuf -oL ls"), True),
    ("perl", _fn("k1l_ctl", "perl -e 1"), True),
    ("busybox", _fn("k1l_ctl", "busybox sh -c true"), True),
    ("`function name {` after a probe",
     _fn("p_k1l_ctl", ":") + "\nfunction k1l_ctl {\n  python3 -I -c pass\n}", True),
    ("`function name() {` after a probe",
     _fn("p_k1l_ctl", ":") + "\nfunction k1l_ctl() {\n  python3 -I -c pass\n}", True),
    ("top-level code after a probe", _fn("p_k1l_ctl", ":") + "\npython3 -I -c pass", True),
    # CONFINE changed after its definition, anywhere (probes included).
    ("CONFINE reassigned", _fn("k1l_ctl", "CONFINE=(env)"), True),
    ("CONFINE appended to", _fn("k1l_ctl", "CONFINE+=(env)"), True),
    ("CONFINE element set", _fn("k1l_ctl", "CONFINE[0]=env"), True),
    ("CONFINE unset", _fn("k1l_ctl", "unset CONFINE"), True),
    ("CONFINE shadowed by a local", _fn("k1l_ctl", "local -a CONFINE=()"), True),
    ("CONFINE read into", _fn("k1l_ctl", 'read -ra CONFINE <<<"env"'), True),
    ("CONFINE reassigned at top level", "CONFINE=(env)", True),
    ("CONFINE reassigned in a probe", _fn("p_k1l_ctl", "CONFINE=(env)"), True),
    # Package code started from a probe (probes are otherwise exempt): each must be flagged.
    ("probe runs $APP_EXE", _fn("p_k1l_ctl", 'env HOME=/x "$APP_EXE"'), True),
    ("probe runs a staged AppImage", _fn("p_k1l_ctl", '"$W/pkgs/to.AppImage"'), True),
    ("probe extracts an AppImage", _fn("p_k1l_ctl", 'cp "$W/pkgs/to.AppImage" "$W/x"; "$W/x" --appimage-extract'), True),
    ("probe installs a .deb", _fn("p_k1l_ctl", 'dpkg --root=/x -i "$W/pkgs/to.deb"'), True),
    ("probe runs the installed binary", _fn("p_k1l_ctl", '"$W/root-deb/usr/bin/citrate-core"'), True),
    ("probe runs the placed AppImage", _fn("p_k1l_ctl", '"$W/apps/Citrate-Core.AppImage" </dev/null &'), True),
    ("probe runs a staged file with python", _fn("p_k1l_ctl", 'python3 -I "$W/pkgs/x.py"'), True),
    # Must NOT be flagged: confined launches, quoted text, and probes that only read package files.
    ("confined app launch", _fn("k1l_ctl", '"${CONFINE[@]}" env HOME=/x "$APP_EXE" </dev/null >/dev/null 2>&1 &'), False),
    ("launcher words in single quotes", _fn("k1l_ctl", "echo 'time eval command `id` python3 &'"), False),
    ("probe reads package paths", _fn("p_k1l_ctl", 'printf \'%s\\n\' "$W/pkgs" | writable_of',
                                      '[ -e "$W/apps/Citrate-Core.AppImage" ] && sha256sum <"$W/pkgs/to.deb"'), False),
    ("command -v lookup", _fn("k1l_ctl", "command -v python3 >/dev/null"), False),
    ("probe runs its own helper", _fn("p_k1l_ctl", "python3 -I -c 'pass'"), False),
]


def controls(text, launches):
    """(1) Remove each CONFINE prefix in turn; each such copy must be flagged at that line.
    (2) Each synthetic snippet must be flagged (or, for the must-pass ones, must not be)."""
    lines = text.split("\n")
    missed = []
    sites = sorted({n for n, _, _ in launches})
    for n in sites:
        # The logical line may start before the physical line holding the prefix.
        k = n - 1
        while CONFINE not in lines[k]:
            k += 1
        mutated = lines[:k] + [lines[k].replace(CONFINE + " ", "", 1)] + lines[k + 1:]
        _, byp = scan("\n".join(mutated))
        if not any(b[0] == n for b in byp):
            missed.append(f"line {n} without CONFINE")
    first = len(lines) + 1  # the snippet's first line number in text + "\n" + snippet
    for name, snippet, must_flag in SYNTHETIC:
        _, byp = scan(text + "\n" + snippet + "\n")
        flagged = any(b[0] >= first for b in byp)
        if flagged != must_flag:
            missed.append(f"{name} ({'not flagged' if must_flag else 'flagged'})")
    return sites, missed


def main(argv):
    path = argv[1] if len(argv) > 1 else os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "linux-native-acceptance.sh")
    with open(path, encoding="utf-8") as f:
        text = f.read()
    launches, bypasses = scan(text)
    ok = True
    if bypasses:
        ok = False
        print(f"confine-check: FAIL: {len(bypasses)} post-seal launch(es) without CONFINE in {path}:")
        for n, func, cmd in bypasses:
            print(f"  line {n} ({func or 'top level'}): {cmd}")
    if not launches:
        ok = False
        print("confine-check: FAIL: no CONFINE launch found; the scan cannot see the harness")
    sites, missed = controls(text, launches)
    if missed:
        ok = False
        print(f"confine-check: FAIL: {len(missed)} control(s) misjudged:")
        for m in missed:
            print(f"  {m}")
    if ok:
        print(f"confine-check: PASS: {len(launches)} post-seal launch(es) at {len(sites)} site(s), "
              f"all through CONFINE; {len(sites)}/{len(sites)} unconfined copies flagged; "
              f"{len(SYNTHETIC)}/{len(SYNTHETIC)} synthetic controls judged right")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
