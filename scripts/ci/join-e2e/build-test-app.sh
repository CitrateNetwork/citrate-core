#!/usr/bin/env bash
# created: 2026-10-23 | branch: test/join-e2e-gate | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active. Join-E2E harness only. Makes a TEST build of Citrate Core for the gate; the
# result must never be published, signed or installed outside the gate's namespace.
#
# Until the join-key ceremony, a release build trusts no resolve key (kit/src/join.rs
# JOIN_PINNED_KEYS is empty) and talks to the production join service (JOIN_API_BASE). The gate
# needs a build that trusts a THROWAWAY key and talks to the loopback join service, so this script:
#
#   1. makes a throwaway Ed25519 resolve key (join_crypto.py keygen) in <out>/keys (0700);
#   2. in a throwaway citrate-core worktree at the ref under test, replaces exactly those two
#      constants (each must occur exactly once, as on core#353's head, or it stops);
#   3. builds the app binary (`tauri build --no-bundle`, frontend included);
#   4. repacks a release AppImage with that binary in place of usr/bin/citrate-core, so every
#      sidecar and library is the released one and only the app under test changes.
#
# Usage: build-test-app.sh --core DIR --base-appimage FILE --out DIR [--api-base URL]
#   --core           a citrate-core LINKED WORKTREE at the ref under test, e.g.
#                    git -C citrate-core worktree add --detach /tmp/x origin/beta/join-11-12-ui
#                    (refuses a main clone: the constants are edited in place)
#   --base-appimage  a release AppImage of the same arch (the gate's pinned one)
#   --api-base       default http://127.0.0.1:3317/api/join (join-e2e.sh's loopback landing)
# Output: <out>/Citrate-Core-test.AppImage, <out>/keys/{signing.jwk,pin.txt}, <out>/provenance.txt.
# Then: join-e2e.sh ... --appimage <out>/Citrate-Core-test.AppImage --signing-key <out>/keys
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CORE="" BASE="" OUT="" API="http://127.0.0.1:3317/api/join"
while [ $# -gt 0 ]; do
  case "$1" in
    --core) CORE="$2"; shift 2 ;;
    --base-appimage) BASE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --api-base) API="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$CORE" ] && [ -n "$BASE" ] && [ -n "$OUT" ] || { echo "need --core, --base-appimage, --out" >&2; exit 2; }
CORE="$(cd "$CORE" && pwd)"
[ "$(git -C "$CORE" rev-parse --git-dir)" != "$(git -C "$CORE" rev-parse --git-common-dir)" ] ||
  { echo "refusing: $CORE is not a linked worktree (this script edits sources in place)" >&2; exit 2; }
case "$API" in http://127.0.0.1:*/api/join) ;; *) echo "refusing a non-loopback --api-base: $API" >&2; exit 2 ;; esac
J="$CORE/kit/src/join.rs"
ref="$(git -C "$CORE" rev-parse HEAD)"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
[ ! -e "$OUT/keys" ] || { echo "refusing: $OUT/keys exists (one key per build)" >&2; exit 2; }
mkdir -m 0700 "$OUT/keys"
/usr/bin/python3 -I "$HERE/join_crypto.py" keygen "$OUT/keys"
read -r kid x <"$OUT/keys/pin.txt"

pins='pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[];'
base='pub const JOIN_API_BASE: &str = "https://www.citrate.ai/api/join";'
[ "$(grep -cxF "$pins" "$J")" = 1 ] && [ "$(grep -cxF "$base" "$J")" = 1 ] ||
  { echo "kit/src/join.rs at $ref does not have the expected constants exactly once; update this script" >&2; exit 2; }
python3 -I - "$J" "$pins" "pub const JOIN_PINNED_KEYS: &[(&str, &str)] = &[(\"$kid\", \"$x\")]; // JOIN-E2E TEST BUILD" \
  "$base" "pub const JOIN_API_BASE: &str = \"$API\"; // JOIN-E2E TEST BUILD" <<'PY'
import sys
p, a, a2, b, b2 = sys.argv[1:]
s = open(p).read()
assert s.count(a) == 1 and s.count(b) == 1
open(p, "w").write(s.replace(a, a2).replace(b, b2))
PY
echo "patched kit/src/join.rs at ${ref:0:10}: pin $kid ${x:0:12}..., api $API"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$CORE/target}" CARGO_PROFILE_RELEASE_DEBUG=0
( cd "$CORE" && { [ -d node_modules ] || npm ci --no-audit --no-fund; } && npx tauri build --no-bundle --ci )
bin="$CARGO_TARGET_DIR/release/citrate-core"
[ -x "$bin" ] || { echo "no app binary at $bin" >&2; exit 1; }
grep -qaF "$x" "$bin" || { echo "the built binary does not carry the throwaway pin" >&2; exit 1; }

work="$(mktemp -d "$OUT/repack.XXXXXX")"
cp "$BASE" "$work/base.AppImage" && chmod +x "$work/base.AppImage"
( cd "$work" && ./base.AppImage --appimage-extract >/dev/null )
[ -x "$work/squashfs-root/usr/bin/citrate-core" ] || { echo "base AppImage has no usr/bin/citrate-core" >&2; exit 1; }
install -m 0755 "$bin" "$work/squashfs-root/usr/bin/citrate-core"
tool="${APPIMAGETOOL:-$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage}"
[ -x "$tool" ] || { echo "no appimage packer (set APPIMAGETOOL; tauri caches one in ~/.cache/tauri)" >&2; exit 1; }
( cd "$work" && OUTPUT="$OUT/Citrate-Core-test.AppImage" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --appdir squashfs-root >"$OUT/repack.log" 2>&1 ) ||
  { echo "repack failed (see $OUT/repack.log)" >&2; exit 1; }
rm -rf -- "${work:?}"
{
  echo "JOIN-E2E TEST BUILD. Never publish, sign or install outside the gate's namespace."
  echo "core ref:       $ref ($(git -C "$CORE" log -1 --format=%s | cut -c1-80))"
  echo "base AppImage:  $(sha256sum <"$BASE" | cut -d' ' -f1)"
  echo "test AppImage:  $(sha256sum <"$OUT/Citrate-Core-test.AppImage" | cut -d' ' -f1)"
  echo "pinned key:     $kid $x (throwaway, private half in keys/signing.jwk)"
  echo "join api base:  $API"
} >"$OUT/provenance.txt"
cat "$OUT/provenance.txt"
