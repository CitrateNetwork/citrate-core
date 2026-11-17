#!/usr/bin/env python3
"""Keep the zip audit ignores honest: only tauri-plugin-updater may use an unpatched zip.

The CI audit step ignores six zip advisories workspace-wide:

- RUSTSEC-2026-0367/0368/0369/0429/0545, fixed in zip 9;
- RUSTSEC-2026-0548 (ZipStreamReader names can differ from the central directory), fixed only
  in zip 9.0.5, so a zip 9.0.0 to 9.0.4 is still affected.

The ignores are justified only for tauri-plugin-updater's Windows-only zip, which unpacks
nothing but a minisign-verified update payload, is built without AES and extracts through
ZipArchive (see the comment at the audit step in .github/workflows/ci.yml). That reasoning
covers no other crate, so this check fails CI when:

1. a crate other than tauri-plugin-updater depends on a zip below 9.0.5 (the floor is the
   newest fix among the ignored advisories, 0548's; a zip 9.0.0 to 9.0.4 outside the updater
   fails here although it is on the major the older five ignores were written against);
2. no zip below 9.0.5 is left in Cargo.lock (the ignores are stale: remove them, and this
   check with them);
3. today is after EXPIRES, the window of the RUSTSEC-2026-0545/0548 ignores published
   2026-11-17 (decide again; do not renew by reflex). The older four are removed with the
   move to tauri-plugin-updater >= 2.14.1 (zip ^9), which also ends the two new ones.

Usage: check-zip-users.py [--lock Cargo.lock] [--self-test]
"""
import argparse
import sys
import tomllib
from datetime import date, timedelta

ADVISORIES = "RUSTSEC-2026-0367/0368/0369/0429/0545/0548"
EXPIRES = date(2027, 1, 31)
FIXED = (9, 0, 5)
ALLOWED_USER = "tauri-plugin-updater"


def vtuple(v: str) -> tuple[int, ...]:
    return tuple(int(p) for p in v.split("-")[0].split("+")[0].split(".")[:3])


def check(lock: dict, today: date) -> list[str]:
    """Return the failures for one parsed Cargo.lock (empty list = pass)."""
    pkgs = lock.get("package", [])
    versions = [p["version"] for p in pkgs if p.get("name") == "zip"]
    old = {v for v in versions if vtuple(v) < FIXED}
    errors = []
    if today > EXPIRES:
        errors.append(f"the RUSTSEC-2026-0545/0548 ignores expired on {EXPIRES}: decide again")
    if not old:
        errors.append(
            f"no zip below {'.'.join(map(str, FIXED))} in Cargo.lock: "
            f"the {ADVISORIES} ignores are stale, remove them and this check"
        )
        return errors
    for p in pkgs:
        for dep in p.get("dependencies", []):
            parts = dep.split()
            if parts[0] != "zip":
                continue
            # A bare name means the lock holds exactly one zip.
            dv = parts[1] if len(parts) > 1 else (versions[0] if len(versions) == 1 else None)
            if dv is not None and dv not in old:
                continue
            if p["name"] != ALLOWED_USER:
                errors.append(
                    f"{p['name']} {p['version']} -> zip {dv or '?'}: "
                    f"below {'.'.join(map(str, FIXED))} and not covered by the {ADVISORIES} ignores"
                )
    return errors


def _lock(*pkgs: tuple[str, str, list[str]]) -> dict:
    return {"package": [{"name": n, "version": v, "dependencies": d} for n, v, d in pkgs]}


def self_test() -> None:
    # Dates derive from EXPIRES, so renewing the window can never leave the
    # expiry case testing a day that is no longer past it.
    ok_day = EXPIRES - timedelta(days=30)
    today_lock = [
        ("zip", "4.6.1", []),
        ("zip", "9.0.5", []),
        ("tauri-plugin-updater", "2.10.1", ["zip 4.6.1"]),
        ("citrate-components", "0.5.4", ["zip 9.0.5"]),
    ]
    cases = [
        ("updater on zip 4, components on 9.0.5", _lock(*today_lock), ok_day, 0),
        ("last day of the window", _lock(*today_lock), EXPIRES, 0),
        ("expired", _lock(*today_lock), EXPIRES + timedelta(days=1), 1),
        ("components back on 9.0.4 (0548 unpatched)",
         _lock(("zip", "4.6.1", []), ("zip", "9.0.4", []),
               ("tauri-plugin-updater", "2.10.1", ["zip 4.6.1"]),
               ("citrate-components", "0.5.4", ["zip 9.0.4"])), ok_day, 1),
        ("new crate on zip 2", _lock(*today_lock, ("zip", "2.6.1", []), ("x", "1.0.0", ["zip 2.6.1"])), ok_day, 1),
        ("new crate on the updater's zip 4", _lock(*today_lock, ("x", "1.0.0", ["zip 4.6.1"])), ok_day, 1),
        ("only one old zip, bare name, non-updater user",
         _lock(("zip", "9.0.4", []), ("citrate-components", "0.5.4", ["zip"])), ok_day, 1),
        ("only one old zip, bare name, updater user",
         _lock(("zip", "4.6.1", []), ("tauri-plugin-updater", "2.10.1", ["zip"])), ok_day, 0),
        ("updater on 9.0.4 is still covered",
         _lock(("zip", "9.0.4", []), ("tauri-plugin-updater", "2.14.1", ["zip"])), ok_day, 0),
        ("stale: only patched zips",
         _lock(("zip", "9.0.5", []), ("tauri-plugin-updater", "2.14.1", ["zip"]),
               ("citrate-components", "0.5.4", ["zip"])), ok_day, 1),
        ("new crate on a patched zip", _lock(*today_lock, ("x", "1.0.0", ["zip 9.0.5"])), ok_day, 0),
    ]
    bad = []
    for name, lock, day, want in cases:
        got = len(check(lock, day))
        if got != want:
            bad.append(f"{name}: expected {want} failure(s), got {got}")
    if bad:
        print("self-test FAILED:", *bad, sep="\n  ")
        sys.exit(1)
    print(f"self-test: {len(cases)} cases pass")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lock", default="Cargo.lock")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        self_test()
        return
    with open(args.lock, "rb") as f:
        lock = tomllib.load(f)
    errors = check(lock, date.today())
    if errors:
        print(f"zip ignore check ({ADVISORIES}) failed:", *errors, sep="\n  ")
        sys.exit(1)
    print(
        f"zip < {'.'.join(map(str, FIXED))} only via {ALLOWED_USER}; "
        f"RUSTSEC-2026-0545/0548 ignores expire {EXPIRES}"
    )


if __name__ == "__main__":
    main()
