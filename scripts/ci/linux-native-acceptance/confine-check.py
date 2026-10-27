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
  - runs an interpreter or one of the session programs (python3, bash, sh, env, Xvfb, dbus-*,
    gnome-keyring-daemon, nsenter, setpriv, setsid, unshare), or dpkg with an install action, or
  - passes --appimage-extract, or
  - is started in the background with `&` (any program, or a subshell).
Exempt: `seal` and `mount_attr` (they build the seal; CONFINE does not exist yet), and `probe`
and the `p_*` probes (they check the seal from PID 1's own view, before anything is launched,
and two negative controls need PID 1's pre-seal handles).

Controls: for every confined launch found, the same file with that one CONFINE prefix removed
must be flagged at that line; a synthetic unconfined background command must be flagged too. A
check that cannot see a bypass fails instead of passing.

Usage: confine-check.py [harness.sh]   (default: the linux-native-acceptance.sh next to this dir)
Exit 0 = every post-seal launch is confined and every control was flagged; 1 otherwise.
"""
import os
import re
import sys

CONFINE = '"${CONFINE[@]}"'
BANNER = re.compile(r"^#\s*Inner phase")
FUNC = re.compile(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*\(\)\s*\{")
HEREDOC = re.compile(r"<<-?\s*'?\"?([A-Za-z_][A-Za-z0-9_]*)'?\"?")
ASSIGN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(\[[^]]*\])?\+?=")
KEYWORDS = {"if", "then", "else", "elif", "do", "while", "until", "!", "{", "}", "time", "fi", "done"}
LAUNCHERS = {
    "python", "python3", "bash", "sh", "env", "exec", "Xvfb", "dbus-daemon", "dbus-send",
    "dbus-launch", "gnome-keyring-daemon", "nsenter", "setpriv", "setsid", "unshare", "nohup",
    "xdg-open", "timeout", "nice", "chroot",
}
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
                func = m.group(1)
        m = HEREDOC.search(line)
        if m and "<<<" not in line:
            heredoc = m.group(1)
        if line.endswith("\\"):
            buf += line[:-1] + " "
            continue
        out.append((start, func, buf + line))
        buf = ""
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
            if c == "'":
                j = s.index("'", i + 1) if "'" in s[i + 1:] else len(s) - 1
                seg.cur += s[i:j + 1]; i = j + 1; continue
            if c == '"':
                i += 1; seg.cur += '"'
                while i < len(s) and s[i] != '"':
                    if s[i] == "\\":
                        seg.cur += s[i:i + 2]; i += 2; continue
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
    if prog == "exec" and (not args or re.match(r"^\{?[A-Za-z0-9_]*\}?[<>]", args[0])):
        return False  # fd redirection only
    if bg:
        return True
    if prog == "(subshell)":
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


def scan(text):
    """(launches, bypasses): launches = [(line, func, program)] through CONFINE;
    bypasses = [(line, func, command)] that start a process without it."""
    launches, bypasses = [], []
    for n, func, line in logical_lines(text):
        if exempt(func):
            continue
        for words, bg in split_commands(line):
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


def controls(text, launches):
    """Remove each CONFINE prefix in turn; each such copy must be flagged at that line."""
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
            missed.append(n)
    synthetic = text + "\nk1l_control_fn() {\n  sleep 300 </dev/null >/dev/null 2>&1 &\n}\n"
    _, byp = scan(synthetic)
    if not any("sleep 300" in b[2] for b in byp):
        missed.append("synthetic background sleep")
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
        print(f"confine-check: FAIL: controls not flagged: {missed}")
    if ok:
        print(f"confine-check: PASS: {len(launches)} post-seal launch(es) at {len(sites)} site(s), "
              f"all through CONFINE; {len(sites) + 1}/{len(sites) + 1} unconfined controls flagged")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
