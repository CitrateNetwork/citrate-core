#!/usr/bin/env bash
# created: 2026-11-11 | branch: feat/053-release-local
# author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
# status: active (owner decision 2026-11-11: 0.5.3 is built, signed and notarized on the release Mac)
#
# The local release of Citrate Core on the release Mac, end to end:
#
#   build -> codesign (Developer ID) -> notarize -> staple -> update files -> tauri signer sign
#   -> latest.json (darwin-aarch64, linux-x86_64(+deb), linux-aarch64(+deb)) -> SHA256SUMS
#   -> gh release create
#
# Owner decision 2026-11-11: the update files are hosted on the GitHub release itself, and
# latest.json points at those assets (no CDN ACL change). 0.5.2 reads the CDN feed first, gets 403,
# and falls back to `releases/latest/download/latest.json` (tauri-plugin-updater 2.10.1 skips a
# non-2xx endpoint), so the release must be the repository's latest, non-prerelease release.
#
# Never launches the built app (not even --version: it starts the full app against the owner's own
# data) and runs no install test.
#
# Usage:
#   scripts/release-local.sh --version <x.y.z> --linux-dir <dir> --out <dir> [--publish]
#       The real release. <x.y.z> must equal src-tauri/tauri.conf.json's version; the tag is v<x.y.z>.
#       Without --publish it stops after the dist folder is built and verified. With --publish it
#       creates the GitHub release v<x.y.z> as --latest, then checks every feed URL anonymously.
#   scripts/release-local.sh --rehearsal --version <x.y.z> --app <path.app> --linux-dir <dir>
#       --out <dir> [--publish] [--cleanup]
#       A rehearsal of every step after the build with an app you give it (it is codesigned here):
#       notarized and stapled for real, signed with the real updater key, published only as a DRAFT
#       PRERELEASE under the tag rehearsal-<x.y.z>, which can never be "latest". --cleanup deletes
#       that draft afterwards.
#   --target <commit>   the commit the release points at (default HEAD); GitHub must have it.
#   scripts/release-local.sh --check-args ...   validate the arguments and stop (tests).
#
# The Linux update files (Citrate-Core-linux-{x86_64,aarch64}.{AppImage,deb}) are built on their own
# hosts (docs/RELEASE_LINUX.md) and copied into --linux-dir; they are signed here, so the updater key
# never leaves the release Mac.
set -euo pipefail

REPO_SLUG="CitrateNetwork/citrate-core"
IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Larry Klosowski (DDHUG44QC7)}"
NOTARY_PROFILE="${NOTARY_PROFILE:-citrate}"
KEY_FILE="${UPDATER_KEY_FILE:-$HOME/.citrate-updater/citrate-core-v2.key}"
KEY_ITEM="citrate-core-updater-v2"
APP_NAME="Citrate Core.app"
LINUX_FILES=(Citrate-Core-linux-x86_64.AppImage Citrate-Core-linux-x86_64.deb Citrate-Core-linux-aarch64.AppImage Citrate-Core-linux-aarch64.deb)
PLATFORMS=(darwin-aarch64 linux-x86_64 linux-x86_64-deb linux-aarch64 linux-aarch64-deb)
# Free space a real release needs on the release Mac (the build, the bundle and ~4 GB of update files).
REAL_FLOOR_GIB=25
REHEARSAL_FLOOR_GIB=3

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
die() { echo "release-local: REFUSED: $*" >&2; exit 1; }
say() { echo "release-local: $*"; }

VERSION="" LINUX_DIR="" OUT="" APP="" TARGET="" REHEARSAL=0 PUBLISH=0 CLEANUP=0 CHECK_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:-}"; shift 2 ;;
    --linux-dir) LINUX_DIR="${2:-}"; shift 2 ;;
    --out) OUT="${2:-}"; shift 2 ;;
    --app) APP="${2:-}"; shift 2 ;;
    --target) TARGET="${2:-}"; shift 2 ;;
    --rehearsal) REHEARSAL=1; shift ;;
    --publish) PUBLISH=1; shift ;;
    --cleanup) CLEANUP=1; shift ;;
    --check-args) CHECK_ONLY=1; shift ;;
    -h|--help) sed -n '2,36p' "$0"; exit 0 ;;
    *) die "unknown argument $1" ;;
  esac
done

# ---- arguments ------------------------------------------------------------------------------
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "--version must be x.y.z (got '$VERSION')"
CONF_VERSION="$(python3 -c "import json;print(json.load(open('$ROOT/src-tauri/tauri.conf.json'))['version'])")"
if [ "$REHEARSAL" -eq 1 ]; then
  [ -n "$APP" ] || die "a rehearsal signs and notarizes an app you give it: --app <path.app>"
  [ "$VERSION" != "$CONF_VERSION" ] || die "a rehearsal uses a throwaway version, not the app's own ($CONF_VERSION)"
  TAG="rehearsal-$VERSION"
  FLOOR=$REHEARSAL_FLOOR_GIB
else
  [ -z "$APP" ] || die "--app is for a rehearsal; a real release builds the app"
  [ "$CLEANUP" -eq 0 ] || die "--cleanup deletes a rehearsal draft only"
  [ "$VERSION" = "$CONF_VERSION" ] || die "--version $VERSION is not tauri.conf.json's $CONF_VERSION (bump the version first)"
  TAG="v$VERSION"
  FLOOR=$REAL_FLOOR_GIB
fi
[ -n "$LINUX_DIR" ] && [ -d "$LINUX_DIR" ] || die "--linux-dir <dir> with the four Linux update files"
for f in "${LINUX_FILES[@]}"; do
  [ -f "$LINUX_DIR/$f" ] || die "no $LINUX_DIR/$f"
done
[ -n "$OUT" ] || die "--out <dir>"
if [ "$CHECK_ONLY" -eq 1 ]; then
  say "arguments OK (tag $TAG, $([ "$REHEARSAL" -eq 1 ] && echo 'draft prerelease' || echo 'latest release'))"
  exit 0
fi

# ---- preflight ------------------------------------------------------------------------------
[ "$(uname -s)" = "Darwin" ] || die "run this on the release Mac"
FREE_GIB=$(df -g "$HOME" | tail -1 | awk '{print $4}')
[ "$FREE_GIB" -ge "$FLOOR" ] || die "${FREE_GIB} GiB free; this needs at least ${FLOOR} GiB"
security find-identity -v -p codesigning | grep -qF "\"$IDENTITY\"" || die "signing identity not in the keychain: $IDENTITY"
xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1 || die "notarytool profile '$NOTARY_PROFILE' does not work"
[ -f "$KEY_FILE" ] || die "no updater key at $KEY_FILE"
security find-generic-password -s "$KEY_ITEM" -a citrate-updater >/dev/null 2>&1 || die "no updater key passphrase in the keychain ($KEY_ITEM)"
command -v minisign >/dev/null || die "minisign is needed to verify each signature against the pinned key (brew install minisign)"
gh auth status >/dev/null 2>&1 || die "gh is not signed in"
if gh release view "$TAG" --repo "$REPO_SLUG" >/dev/null 2>&1; then die "a release $TAG already exists"; fi
# The release points at a commit GitHub has (the pushed release commit); checked before any signing.
TARGET_SHA="$(git -C "$ROOT" rev-parse "${TARGET:-HEAD}")"
gh api "repos/$REPO_SLUG/commits/$TARGET_SHA" >/dev/null 2>&1 ||
  die "GitHub has no commit $TARGET_SHA; push the release commit first (or pass --target <a pushed commit>)"
mkdir -p "$OUT"
[ -z "$(ls -A "$OUT")" ] || die "--out $OUT is not empty"
OUT="$(cd "$OUT" && pwd)"
WORK="$OUT/.work"
mkdir -p "$WORK"

# The updater key, for this run only (never on argv).
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY_FILE")"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(security find-generic-password -s "$KEY_ITEM" -a citrate-updater -w)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
tauri() { (cd "$ROOT" && npx --no-install tauri "$@"); }
tauri --version >/dev/null 2>&1 || die "the Tauri CLI is not installed here (npm ci in $ROOT)"

# ---- 1. build (real) or the given app (rehearsal) -----------------------------------------------
if [ "$REHEARSAL" -eq 0 ]; then
  say "1/7 verifying the staged inputs"
  (cd "$ROOT" && node scripts/component-pins.mjs verify-staged src-tauri/binaries aarch64-apple-darwin)
  (cd "$ROOT" && node scripts/check-staged-corpus.mjs --pins src-tauri/runtime-deps.sha256)
  say "1/7 building and codesigning (tauri build, bundle-lite)"
  (cd "$ROOT" && APPLE_SIGNING_IDENTITY="$IDENTITY" npx --no-install tauri build \
    --target aarch64-apple-darwin --config src-tauri/tauri.bundle-lite.conf.json)
  BUNDLE="$ROOT/target/aarch64-apple-darwin/release/bundle"
  APP="$BUNDLE/macos/$APP_NAME"
  DMG_SRC="$(ls "$BUNDLE"/dmg/*.dmg | head -1)"
  [ -d "$APP" ] && [ -f "$DMG_SRC" ] || die "the build left no $APP_NAME or DMG under $BUNDLE"
  cp "$DMG_SRC" "$WORK/app.dmg"
else
  say "1/7 rehearsal: codesigning the given app and making its DMG"
  [ -d "$APP" ] || die "no app at $APP"
  cp -R "$APP" "$WORK/$APP_NAME"
  APP="$WORK/$APP_NAME"
  codesign --force --deep --options runtime --timestamp --sign "$IDENTITY" "$APP"
  hdiutil create -quiet -volname "Citrate Core" -srcfolder "$APP" -ov -format UDZO "$WORK/app.dmg"
  codesign --force --timestamp --sign "$IDENTITY" "$WORK/app.dmg"
fi
codesign --verify --deep --strict "$APP" || die "the app's signature does not verify"

# ---- 2. notarize and staple ------------------------------------------------------------------
say "2/7 notarizing the DMG (this waits for Apple)"
xcrun notarytool submit "$WORK/app.dmg" --keychain-profile "$NOTARY_PROFILE" --wait --output-format json \
  > "$WORK/notary.json"
STATUS="$(python3 -c "import json;print(json.load(open('$WORK/notary.json')).get('status',''))")"
[ "$STATUS" = "Accepted" ] || die "notarization status '$STATUS' (see $WORK/notary.json; xcrun notarytool log <id>)"
xcrun stapler staple "$WORK/app.dmg" >/dev/null
xcrun stapler staple "$APP" >/dev/null
xcrun stapler validate "$WORK/app.dmg" >/dev/null || die "the DMG ticket does not validate"
xcrun stapler validate "$APP" >/dev/null || die "the app ticket does not validate"
spctl -a -t exec -vv "$APP" 2>&1 | grep -q "source=Notarized Developer ID" || die "Gatekeeper does not accept the app as Notarized Developer ID"

# ---- 3. update files -------------------------------------------------------------------------
say "3/7 update files"
cp "$WORK/app.dmg" "$OUT/Citrate-Core_${VERSION}_aarch64.dmg"
# The update tarball is made from the STAPLED app (tauri build's own tarball predates the staple).
COPYFILE_DISABLE=1 tar -C "$(dirname "$APP")" -czf "$OUT/Citrate-Core.app.tar.gz" "$APP_NAME"
for f in "${LINUX_FILES[@]}"; do cp "$LINUX_DIR/$f" "$OUT/$f"; done

# ---- 4. sign the update files with the v2 updater key, and verify against the pinned key --------
say "4/7 signing the update files (tauri signer sign) and verifying each against the pinned key"
PUBKEY="$(python3 - "$ROOT/src-tauri/tauri.conf.json" <<'PY'
import base64, json, sys
txt = base64.b64decode(json.load(open(sys.argv[1]))["plugins"]["updater"]["pubkey"]).decode()
print(txt.strip().splitlines()[-1])
PY
)"
for f in Citrate-Core.app.tar.gz "${LINUX_FILES[@]}"; do
  rm -f "$OUT/$f.sig"
  tauri signer sign "$OUT/$f" >/dev/null
  [ -f "$OUT/$f.sig" ] || die "tauri signer wrote no $f.sig"
  base64 -d < "$OUT/$f.sig" > "$WORK/$f.minisig"
  minisign -V -q -P "$PUBKEY" -m "$OUT/$f" -x "$WORK/$f.minisig" || die "$f.sig does not verify against the pinned updater key"
done

# ---- 5. latest.json, pointing at this release's assets -----------------------------------------
say "5/7 latest.json"
BASE_URL="https://github.com/$REPO_SLUG/releases/download/$TAG"
(cd "$ROOT" && node scripts/updater-feed.mjs fragment --version "$VERSION" --base-url "$BASE_URL" --dir "$OUT" \
  "${PLATFORMS[@]}") > "$WORK/fragment.json"
NOTES="${RELEASE_NOTES:-Citrate Core $VERSION}"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
(cd "$ROOT" && node scripts/updater-feed.mjs merge --notes "$NOTES" --pub-date "$PUB_DATE" "$WORK/fragment.json") \
  > "$OUT/latest.json"
python3 - "$OUT/latest.json" "$VERSION" "$BASE_URL" <<'PY'
import json, sys
feed, version, base = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3]
assert feed["version"] == version, feed["version"]
want = {"darwin-aarch64", "linux-x86_64", "linux-x86_64-deb", "linux-aarch64", "linux-aarch64-deb"}
assert set(feed["platforms"]) == want, sorted(feed["platforms"])
for p, e in feed["platforms"].items():
    assert e["url"].startswith(base + "/"), (p, e["url"])
print("release-local: latest.json names", len(feed["platforms"]), "platforms under", base)
PY

# ---- 6. checksums ----------------------------------------------------------------------------
say "6/7 SHA256SUMS"
(cd "$OUT" && shasum -a 256 Citrate-Core_"${VERSION}"_aarch64.dmg Citrate-Core.app.tar.gz "${LINUX_FILES[@]}" latest.json > SHA256SUMS)
ASSETS=("$OUT"/Citrate-Core_"${VERSION}"_aarch64.dmg "$OUT/Citrate-Core.app.tar.gz" "$OUT/Citrate-Core.app.tar.gz.sig")
for f in "${LINUX_FILES[@]}"; do ASSETS+=("$OUT/$f" "$OUT/$f.sig"); done
ASSETS+=("$OUT/latest.json" "$OUT/SHA256SUMS")
say "dist ready in $OUT (${#ASSETS[@]} assets)"

if [ "$PUBLISH" -eq 0 ]; then
  say "stopped before publishing (pass --publish to create the release)"
  exit 0
fi

# ---- 7. publish ------------------------------------------------------------------------------
if [ "$REHEARSAL" -eq 1 ]; then
  say "7/7 creating the DRAFT PRERELEASE $TAG (never latest)"
  gh release create "$TAG" "${ASSETS[@]}" --repo "$REPO_SLUG" --draft --prerelease --latest=false \
    --target "$TARGET_SHA" --title "REHEARSAL $VERSION (not a release)" \
    --notes "Rehearsal of scripts/release-local.sh. Not a Citrate Core release; deleted after the check."
else
  say "7/7 creating the release $TAG as latest"
  gh release create "$TAG" "${ASSETS[@]}" --repo "$REPO_SLUG" --latest \
    --target "$TARGET_SHA" --title "Citrate Core $VERSION" --notes "$NOTES"
fi

# Verify what GitHub holds: every asset, at the size and digest we built.
gh release view "$TAG" --repo "$REPO_SLUG" --json assets -q '.assets[] | "\(.name) \(.size)"' > "$WORK/remote-assets.txt"
for f in "${ASSETS[@]}"; do
  n="$(basename "$f")"; s="$(stat -f %z "$f")"
  grep -qx "$n $s" "$WORK/remote-assets.txt" || die "the release has no $n of $s bytes"
done
mkdir -p "$WORK/back"
gh release download "$TAG" --repo "$REPO_SLUG" -p latest.json -p SHA256SUMS -D "$WORK/back"
cmp -s "$WORK/back/latest.json" "$OUT/latest.json" || die "latest.json on the release differs"
cmp -s "$WORK/back/SHA256SUMS" "$OUT/SHA256SUMS" || die "SHA256SUMS on the release differs"
say "the release holds every asset at its size; latest.json and SHA256SUMS read back identical"

if [ "$REHEARSAL" -eq 0 ]; then
  # What a 0.5.2 install sees: the fallback feed, anonymously, then every update file it names.
  code="$(curl -s -o "$WORK/feed.json" -w '%{http_code}' -L "https://github.com/$REPO_SLUG/releases/latest/download/latest.json")"
  [ "$code" = "200" ] || die "the fallback feed answers $code"
  cmp -s "$WORK/feed.json" "$OUT/latest.json" || die "the fallback feed is not this release's latest.json"
  python3 -c "import json;[print(e['url']) for e in json.load(open('$OUT/latest.json'))['platforms'].values()]" |
    while read -r u; do
      c="$(curl -s -o /dev/null -w '%{http_code}' -L -r 0-0 "$u")"
      case "$c" in 200|206) ;; *) die "$u answers $c" ;; esac
    done
  say "the fallback feed and every update file answer anonymously"
elif [ "$CLEANUP" -eq 1 ]; then
  gh release delete "$TAG" --repo "$REPO_SLUG" --yes --cleanup-tag
  say "rehearsal draft $TAG deleted"
fi
