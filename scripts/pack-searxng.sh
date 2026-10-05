#!/usr/bin/env bash
# HUP-S5.2 / S5.5: pack the SearXNG component artifact from its hash lock.
#
#   scripts/pack-searxng.sh --out <dir> [--platform macos-arm64] [--work <dir>] [--no-smoke]
#                           [--url <https URL where the artifact will be hosted> [--apply]]
#
# The artifact is self-contained and relocatable: the pinned python-build-standalone runtime
# (the bundle's `python` entry, checked by SHA-256), the hash-locked wheels from the lock named by
# the bundle's `searxng` `build_from`, and the pinned, unmodified SearXNG source archive (the
# bundle's `upstream`, checked by SHA-256), installed into that runtime. `bin/searxng-run`
# starts it with the bundled Python in isolated mode; the settings come from
# SEARXNG_SETTINGS_PATH, which the Hermes supervisor writes (127.0.0.1 only). LICENSES/ carries
# the AGPL text and the source location (g3-licence).
#
# The tarball is deterministic (sorted entries, fixed times, no owners, no hard links, gzip
# without a name or time). The script prints its SHA-256 and size and writes
# <out>/searxng-<platform>.bundle-entry.json, the `artifacts.<platform>` entry that
# `citrate-components manifest-from-bundle` reads once the file is hosted. With --url and
# --apply it writes that entry into components/toolchain-bundle.json (status measured). It never
# signs: signing is the @rule8 component-key ceremony (docs/COMPONENT_UPDATER.md).
#
# Needs: bash, curl, python3 (any 3.9+, for JSON and the tarball), and a host of the target
# platform (wheels are native). Network: the python archive, the wheels and the SearXNG archive.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd -P)
BUNDLE="$REPO/components/toolchain-bundle.json"
PLATFORM=macos-arm64
OUT=""
WORK=""
URL=""
APPLY=0
SMOKE=1

die() { echo "pack-searxng: $*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT=${2:?}; shift 2 ;;
    --platform) PLATFORM=${2:?}; shift 2 ;;
    --work) WORK=${2:?}; shift 2 ;;
    --url) URL=${2:?}; shift 2 ;;
    --apply) APPLY=1; shift ;;
    --no-smoke) SMOKE=0; shift ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) die "unknown argument $1" ;;
  esac
done
[ -n "$OUT" ] || die "--out <dir> is required"
[ "$APPLY" = 0 ] || [ -n "$URL" ] || die "--apply needs --url"
case "$URL" in ""|https://*) ;; *) die "--url must be https" ;; esac

host_platform() {
  local os arch
  os=$(uname -s); arch=$(uname -m)
  case "$os/$arch" in
    Darwin/arm64) echo macos-arm64 ;;
    Darwin/x86_64) echo macos-x64 ;;
    Linux/x86_64) echo linux-x64 ;;
    Linux/aarch64|Linux/arm64) echo linux-arm64 ;;
    *) echo "unknown" ;;
  esac
}
[ "$(host_platform)" = "$PLATFORM" ] || die "build $PLATFORM on a $PLATFORM host (wheels are native); this host is $(host_platform)"

sha256() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1; else sha256sum "$1" | cut -d' ' -f1; fi
}

fetch_checked() { # url sha256 dest
  curl -fsSL --proto '=https' --tlsv1.2 -o "$3.part" "$1" || die "download failed: $1"
  local got; got=$(sha256 "$3.part")
  [ "$got" = "$2" ] || die "sha256 mismatch for $1: got $got, the bundle pins $2"
  mv "$3.part" "$3"
}

# Everything the build needs, read from the bundle (one source of truth).
VARS=$(python3 - "$BUNDLE" "$PLATFORM" <<'PY'
import json, shlex, sys
b = json.load(open(sys.argv[1])); plat = sys.argv[2]
tools = {t["name"]: t for t in b["tools"]}
s, py = tools["searxng"], tools["python"]
a, pa = s["artifacts"][plat], py["artifacts"][plat]
if not a.get("build_from"):
    sys.exit(f"searxng {plat}: no build_from lock in the bundle")
if pa["status"] != "measured":
    sys.exit(f"python {plat}: not measured in the bundle")
out = {
    "SX_VERSION": s["version"], "SX_URL": s["upstream"]["url"], "SX_SHA": s["upstream"]["sha256"],
    "SX_LOCK": a["build_from"], "SX_EP": s["entrypoints"][0],
    "PY_VERSION": py["version"], "PY_URL": pa["url"], "PY_SHA": pa["sha256"],
}
for k, v in out.items():
    print(f"{k}={shlex.quote(v)}")
PY
) || die "cannot read the searxng and python entries of $BUNDLE"
eval "$VARS"
[ "$SX_EP" = "bin/searxng-run" ] || die "the bundle's searxng entrypoint is $SX_EP; this script builds bin/searxng-run"
LOCK="$REPO/$SX_LOCK"
[ -f "$LOCK" ] || die "missing lock $SX_LOCK"

mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd -P)
if [ -z "$WORK" ]; then WORK=$(mktemp -d "${TMPDIR:-/tmp}/pack-searxng.XXXXXX"); fi
mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd -P)
STAGE="$WORK/stage"
rm -rf "$STAGE" "$WORK/wheels"
mkdir -p "$STAGE" "$WORK/wheels" "$WORK/dl"
echo "pack-searxng: SearXNG $SX_VERSION for $PLATFORM, work dir $WORK"

# 1. The pinned Python runtime.
[ -f "$WORK/dl/python.tar.gz" ] && [ "$(sha256 "$WORK/dl/python.tar.gz")" = "$PY_SHA" ] || fetch_checked "$PY_URL" "$PY_SHA" "$WORK/dl/python.tar.gz"
tar -xzf "$WORK/dl/python.tar.gz" -C "$STAGE"
PY="$STAGE/python/bin/python3"
[ -x "$PY" ] || die "the python archive has no python/bin/python3"

# 2. The pinned, unmodified SearXNG source.
[ -f "$WORK/dl/searxng.tar.gz" ] && [ "$(sha256 "$WORK/dl/searxng.tar.gz")" = "$SX_SHA" ] || fetch_checked "$SX_URL" "$SX_SHA" "$WORK/dl/searxng.tar.gz"

# 3. The hash-locked wheels, then the install, with nothing taken from an index after download.
# No bytecode is written anywhere in the tree (pip runs build steps in child processes).
export PYTHONDONTWRITEBYTECODE=1
"$PY" -I -B -m pip download --quiet --disable-pip-version-check --no-deps --only-binary=:all: \
  --require-hashes -r "$LOCK" -d "$WORK/wheels"
"$PY" -I -B -m pip install --quiet --disable-pip-version-check --no-index --no-compile \
  --no-warn-script-location --require-hashes --find-links "$WORK/wheels" -r "$LOCK"
"$PY" -I -B -m pip install --quiet --disable-pip-version-check --no-index --no-compile \
  --no-warn-script-location --no-deps --no-build-isolation "$WORK/dl/searxng.tar.gz"
# Build-only packages and pip itself are not part of the runtime.
"$PY" -I -B -m pip uninstall --quiet --disable-pip-version-check -y setuptools wheel packaging pip

# 4. No trace of this machine: drop the install record that names the local archive path and
# every console script whose shebang points into the work dir (the launcher below replaces them).
python3 - "$STAGE" "$WORK" <<'PY'
import os, sys
stage, work = sys.argv[1], sys.argv[2]
site = None
for root, dirs, files in os.walk(os.path.join(stage, "python", "lib")):
    if root.endswith("site-packages"):
        site = root
        break
if site is None:
    sys.exit("no site-packages")
for d in os.listdir(site):
    if d.endswith(".dist-info"):
        p = os.path.join(site, d, "direct_url.json")
        if os.path.exists(p):
            os.remove(p)
            rec = os.path.join(site, d, "RECORD")
            lines = [l for l in open(rec) if not l.startswith(f"{d}/direct_url.json,")]
            open(rec, "w").writelines(lines)
bindir = os.path.join(stage, "python", "bin")
for n in os.listdir(bindir):
    p = os.path.join(bindir, n)
    if os.path.islink(p) or not os.path.isfile(p):
        continue
    with open(p, "rb") as f:
        head = f.read(2)
        rest = f.read()
    if head == b"#!" and work.encode() in rest:
        os.remove(p)
for root, dirs, files in os.walk(stage):
    for n in files:
        p = os.path.join(root, n)
        if os.path.islink(p):
            continue
        try:
            data = open(p, "rb").read()
        except OSError:
            continue
        if work.encode() in data:
            sys.exit(f"{os.path.relpath(p, stage)} still names the work dir")
PY

# 5. The launcher and the licence material.
mkdir -p "$STAGE/bin" "$STAGE/LICENSES"
cat > "$STAGE/bin/searxng-run" <<'SH'
#!/bin/sh
# Starts the bundled SearXNG (unmodified upstream, AGPL-3.0-or-later; see LICENSES/) with the
# bundled Python in isolated mode, writing no bytecode into the installed component. The
# settings come from SEARXNG_SETTINGS_PATH (Hermes writes them: 127.0.0.1 only).
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
exec "$here/python/bin/python3" -I -B -c 'from searx.webapp import run; run()'
SH
chmod 0755 "$STAGE/bin/searxng-run"
python3 - "$WORK/dl/searxng.tar.gz" "$STAGE/LICENSES/SearXNG-AGPL-3.0-or-later.txt" <<'PY' \
  || die "no top-level LICENSE in the SearXNG archive"
import sys, tarfile
with tarfile.open(sys.argv[1]) as t:
    m = next(m for m in t.getmembers() if m.isfile() and m.name.count("/") == 1 and m.name.endswith("/LICENSE"))
    data = t.extractfile(m).read()
if b"GNU AFFERO GENERAL PUBLIC LICENSE" not in data:
    sys.exit("the SearXNG LICENSE is not the AGPL")
open(sys.argv[2], "wb").write(data)
PY
cat > "$STAGE/LICENSES/SOURCE.txt" <<EOF
SearXNG $SX_VERSION, AGPL-3.0-or-later, installed unmodified from:
  $SX_URL
  sha256 $SX_SHA
Python $PY_VERSION (python-build-standalone, PSF-2.0 and the licences in python/):
  $PY_URL
  sha256 $PY_SHA
Python packages: the hash lock $SX_LOCK in citrate-core; each package's licence is in its
.dist-info folder under python/lib.
Hermes runs this SearXNG bound to 127.0.0.1 for this computer only.
EOF

# 6. Smoke test: start it on loopback with no engines and wait for /healthz.
if [ "$SMOKE" = 1 ]; then
  SM="$WORK/smoke"; rm -rf "$SM"; mkdir -p "$SM"
  PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  SECRET=$(python3 -c 'import secrets; print(secrets.token_hex(32))')
  cat > "$SM/settings.yml" <<EOF
use_default_settings:
  engines:
    keep_only: []
server:
  bind_address: "127.0.0.1"
  port: $PORT
  secret_key: "$SECRET"
  limiter: false
  public_instance: false
  image_proxy: false
search:
  formats:
    - json
EOF
  env -i PATH=/usr/bin:/bin HOME="$SM" LANG=C.UTF-8 SEARXNG_SETTINGS_PATH="$SM/settings.yml" \
    "$STAGE/bin/searxng-run" >"$SM/searxng.log" 2>&1 &
  PID=$!
  ok=0
  for _ in $(seq 1 120); do
    if curl -fs "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1; then ok=1; break; fi
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.5
  done
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  [ "$ok" = 1 ] || { tail -20 "$SM/searxng.log" >&2; die "the packed SearXNG did not answer /healthz"; }
  # The smoke run must not have written into the tree that is packed.
  if find "$STAGE" -name '__pycache__' -newer "$STAGE/bin/searxng-run" | grep -q .; then
    die "the smoke run wrote bytecode into the component tree"
  fi
  echo "pack-searxng: smoke ok (/healthz on 127.0.0.1:$PORT)"
fi

# 7. The deterministic tarball and the bundle entry.
NAME="searxng-${SX_VERSION}-${PLATFORM}.tar.gz"
python3 - "$STAGE" "$OUT/$NAME" <<'PY'
import gzip, io, os, sys, tarfile
stage, dest = sys.argv[1], sys.argv[2]
EPOCH = 1_700_000_000  # fixed, so the same inputs give the same bytes
entries = []
for root, dirs, files in os.walk(stage):
    dirs.sort()
    rel = os.path.relpath(root, stage)
    for n in sorted(dirs) + sorted(files):
        entries.append(os.path.normpath(os.path.join(rel, n)))
entries.sort()
buf = io.BytesIO()
with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as tar:
    for rel in entries:
        p = os.path.join(stage, rel)
        st = os.lstat(p)
        ti = tarfile.TarInfo(rel)
        ti.mtime = EPOCH
        ti.uid = ti.gid = 0
        ti.uname = ti.gname = ""
        if os.path.islink(p):
            target = os.readlink(p)
            if os.path.isabs(target):
                sys.exit(f"{rel}: absolute symlink {target}")
            ti.type = tarfile.SYMTYPE
            ti.linkname = target
            ti.mode = 0o777
            tar.addfile(ti)
        elif os.path.isdir(p):
            ti.type = tarfile.DIRTYPE
            ti.mode = 0o755
            tar.addfile(ti)
        elif os.path.isfile(p):
            # Always a regular file, never a hard link (the updater refuses hard links).
            ti.type = tarfile.REGTYPE
            ti.mode = 0o755 if st.st_mode & 0o111 else 0o644
            ti.size = st.st_size
            with open(p, "rb") as f:
                tar.addfile(ti, f)
        else:
            sys.exit(f"{rel}: not a file, directory or symlink")
raw = buf.getvalue()
with open(dest, "wb") as out:
    with gzip.GzipFile(filename="", mode="wb", fileobj=out, mtime=0, compresslevel=9) as gz:
        gz.write(raw)
PY
SHA=$(sha256 "$OUT/$NAME")
SIZE=$(wc -c < "$OUT/$NAME" | tr -d ' ')
ENTRY="$OUT/searxng-$PLATFORM.bundle-entry.json"
python3 - "$BUNDLE" "$PLATFORM" "$SHA" "$SIZE" "$URL" "$ENTRY" "$APPLY" "$NAME" "$SMOKE" <<'PY'
import json, sys, datetime
bundle, plat, sha, size, url, entry_path, apply, name, smoke = sys.argv[1:10]
text = open(bundle).read()
b = json.loads(text)
tool = next(t for t in b["tools"] if t["name"] == "searxng")
old = tool["artifacts"][plat]
today = datetime.date.today().isoformat()
entry = {
    "url": url or None,
    "format": "tar.gz",
    "status": "measured" if url else "to_be_built",
    "sha256": sha if url else None,
    "size": int(size) if url else None,
    "build_from": old["build_from"],
    "note": f"Packed by scripts/pack-searxng.sh on {today} ({name}): the pinned Python runtime, the hash-locked wheels and the unmodified SearXNG source; "
    + ("bin/searxng-run answered /healthz on 127.0.0.1." if smoke == "1" else "packed with --no-smoke, so not started."),
}
json.dump({"artifact": name, "sha256": sha, "size": int(size), "entry": entry}, open(entry_path, "w"), indent=2)
if apply == "1":
    if json.dumps(b, indent=2, ensure_ascii=False) + "\n" != text:
        sys.exit("the bundle file is not in the canonical layout; edit it by hand")
    tool["artifacts"][plat] = entry
    open(bundle, "w").write(json.dumps(b, indent=2, ensure_ascii=False) + "\n")
    print(f"pack-searxng: wrote the {plat} entry into {bundle}")
PY
echo "pack-searxng: $OUT/$NAME"
echo "pack-searxng: sha256 $SHA size $SIZE"
echo "pack-searxng: bundle entry $ENTRY"
if [ -z "$URL" ]; then
  echo "pack-searxng: host the file, then rerun with --url <https URL> --apply (or paste the entry) and run check-bundle"
fi
