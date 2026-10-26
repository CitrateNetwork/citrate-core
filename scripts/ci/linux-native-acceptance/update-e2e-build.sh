#!/usr/bin/env bash
# update-e2e-build.sh — RC52 G8: build the two test AppImages the K1-L `update-e2e` scenario needs.
#
#   N    (--from-version, default 0.5.1): pins a THROWAWAY updater key and a loopback feed
#        (http://127.0.0.1:<port>/latest.json); built with the `updater-e2e` cargo feature, so it
#        checks, installs and restarts by itself at start-up when CITRATE_UPDATER_E2E is set.
#   N+1  (--to-version, default 0.5.2): the same build at the next version; its AppImage is signed
#        with the throwaway key (`tauri signer sign`, as the release signs its Linux artifacts)
#        and, for the negative case, with a second throwaway key.
#
# Both keys are generated here, into the output dir, and the private halves are deleted before the
# script exits. The release updater keys are never read: TAURI_SIGNING_* is cleared, and the
# shipping tauri.conf.json is not modified (the key and feed come from a --config overlay).
# Output (--out DIR): from.AppImage, to.AppImage, to.AppImage.sig, to.AppImage.other.sig,
# overlay-from.json, overlay-to.json, keys/{a,b}.key.pub, versions.
#
# Usage: scripts/ci/linux-native-acceptance/update-e2e-build.sh --out DIR
#          [--from-version V] [--to-version V] [--port P] [--overlay-only]
#   --overlay-only  write the keys' public halves and the two overlays, build nothing (any OS)
# Docs: docs/UPDATER_E2E.md. Runs in CI (.github/workflows/linux-native-acceptance.yml).
set -euo pipefail
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PATH TAURI_SIGNING_PRIVATE_KEY_PASSWORD

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
OUT="" FROM_V=0.5.1 TO_V=0.5.2 PORT=18477 OVERLAY_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    --from-version) FROM_V="$2"; shift 2 ;;
    --to-version) TO_V="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --overlay-only) OVERLAY_ONLY=1; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$OUT" ] || { echo "--out DIR is required" >&2; exit 2; }
mkdir -p "$OUT/keys" && chmod 0700 "$OUT/keys"
OUT="$(cd "$OUT" && pwd)"
TAURI="$ROOT/node_modules/.bin/tauri"
[ -x "$TAURI" ] || { echo "missing $TAURI (run npm ci)" >&2; exit 2; }

# Throwaway keys. The password only exists in this process.
PW="$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')"
trap 'rm -f -- "$OUT/keys/a.key" "$OUT/keys/b.key"' EXIT
for k in a b; do
  "$TAURI" signer generate --ci -f -p "$PW" -w "$OUT/keys/$k.key" >/dev/null
done
PUB_A="$(tr -d '\n' <"$OUT/keys/a.key.pub")"
SHIPPING_PUB="$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["plugins"]["updater"]["pubkey"])' \
  "$ROOT/src-tauri/tauri.conf.json")"
[ "$PUB_A" != "$SHIPPING_PUB" ] || { echo "generated key equals the release key; refusing" >&2; exit 2; }

overlay() { # version out
  python3 -I - "$1" "$PUB_A" "$PORT" >"$2" <<'PY'
import json, sys
version, pub, port = sys.argv[1:4]
print(json.dumps({
    "version": version,
    "plugins": {"updater": {
        "pubkey": pub,
        "endpoints": [f"http://127.0.0.1:{port}/latest.json"],
        # Plain http, only to the loopback feed inside the namespace. The shipping config never
        # carries this flag (tripwire: upgrade_updater_feed_tests).
        "dangerousInsecureTransportProtocol": True,
    }},
    "bundle": {"targets": ["appimage"], "createUpdaterArtifacts": False},
}, indent=2))
PY
}
overlay "$FROM_V" "$OUT/overlay-from.json"
overlay "$TO_V" "$OUT/overlay-to.json"
printf 'from=%s\nto=%s\nport=%s\n' "$FROM_V" "$TO_V" "$PORT" >"$OUT/versions"
echo "overlays: $OUT/overlay-from.json $OUT/overlay-to.json (throwaway key $(base64 -d <<<"$PUB_A" | head -1 | awk '{print $NF}'))"
[ "$OVERLAY_ONLY" -eq 1 ] && exit 0

[ "$(uname -s)" = Linux ] || { echo "AppImages build on Linux only (use --overlay-only here)" >&2; exit 2; }
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
build() { # version overlay out
  local v="$1" conf="$2" out="$3" ai
  rm -rf -- "${TARGET_DIR:?}/release/bundle/appimage"
  (cd "$ROOT" && APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 "$TAURI" build --bundles appimage \
    --config "$conf" --features updater-e2e)
  ai="$(find "$TARGET_DIR/release/bundle/appimage" -maxdepth 1 -name "*_${v}_*.AppImage" | head -1)"
  [ -n "$ai" ] || { echo "no AppImage for $v under $TARGET_DIR/release/bundle/appimage" >&2; exit 1; }
  cp -- "$ai" "$out"
  echo "built $out ($v, sha256 $(sha256sum <"$out" | cut -c1-16))"
}
build "$FROM_V" "$OUT/overlay-from.json" "$OUT/from.AppImage"
build "$TO_V" "$OUT/overlay-to.json" "$OUT/to.AppImage"

# `tauri signer sign` writes <file>.sig: sign with B first and move that aside, then with A.
sign() { "$TAURI" signer sign -f "$OUT/keys/$1.key" -p "$PW" "$OUT/to.AppImage" >/dev/null; }
rm -f -- "$OUT/to.AppImage.sig"
sign b && mv -- "$OUT/to.AppImage.sig" "$OUT/to.AppImage.other.sig"
sign a
echo "signed to.AppImage with throwaway key A (and B for the other-key case); private keys removed on exit"
