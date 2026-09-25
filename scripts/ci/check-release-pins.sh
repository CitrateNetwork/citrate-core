#!/usr/bin/env bash
# PBA-L7b-005 tripwire — the release workflow may only stage `runtime-deps` assets that pass the
# committed sha256 manifest check, and the check must run before anything is staged or signed.
# Fails if:
#   * any `gh release download runtime-deps` in release.yml names an asset not in the verified list,
#   * a download targets the bundle tree (src-tauri/...) directly instead of the scratch dir,
#   * the verify step is missing, or runs after staging / after the tauri-action build step,
#   * the verify script itself does not fail closed (self-test below).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WF="${1:-$ROOT/.github/workflows/release.yml}"
fail(){ echo "::error::release pin tripwire: $*"; exit 1; }
grep -q 'gh release download runtime-deps' "$WF" || fail "no runtime-deps download found (update the tripwire)"
if grep -E 'gh release download runtime-deps' "$WF" | grep -q -- '-D src-tauri'; then
  fail "an asset is downloaded straight into the bundle tree (must go to the verified scratch dir)"
fi
verify_line="$(grep -n 'verify-runtime-deps.sh' "$WF" | head -1 | cut -d: -f1)"
[ -n "$verify_line" ] || fail "release.yml never runs scripts/ci/verify-runtime-deps.sh"
for marker in 'chmod +x src-tauri/binaries' 'tar -xzf' 'tauri-apps/tauri-action'; do
  l="$(grep -nF "$marker" "$WF" | head -1 | cut -d: -f1)"
  [ -z "$l" ] || [ "$l" -gt "$verify_line" ] || fail "'$marker' (line $l) runs before the digest check (line $verify_line)"
done
# Every downloaded asset must be one the verify step checks: downloads must go through "$a" from
# the ASSETS array, which is exactly the list handed to the verifier.
if grep -E 'gh release download runtime-deps' "$WF" | grep -vq -- '-p "\$a"'; then
  fail "a runtime-deps download bypasses the verified ASSETS list"
fi
grep -q 'verify-runtime-deps.sh src-tauri/runtime-deps.sha256 "$DL" "${ASSETS\[@\]}"' "$WF" \
  || fail "the verifier is not handed the full ASSETS list"
# Self-test: the verifier fails closed on unpinned / tampered assets and passes a good pin.
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
printf 'good bytes' > "$T/a.bin"
printf '# comment\n' > "$T/m"
if "$ROOT/scripts/ci/verify-runtime-deps.sh" "$T/m" "$T" a.bin >/dev/null 2>&1; then fail "verifier accepted an UNPINNED asset"; fi
( cd "$T" && shasum -a 256 a.bin ) >> "$T/m"
"$ROOT/scripts/ci/verify-runtime-deps.sh" "$T/m" "$T" a.bin >/dev/null 2>&1 || fail "verifier rejected a correctly pinned asset"
# An ambiguous manifest (the same asset pinned twice) is refused, not "any line matches".
cat "$T/m" "$T/m" > "$T/m2"
if "$ROOT/scripts/ci/verify-runtime-deps.sh" "$T/m2" "$T" a.bin >/dev/null 2>&1; then fail "verifier accepted a DUPLICATE pin"; fi
printf 'tampered' > "$T/a.bin"
if "$ROOT/scripts/ci/verify-runtime-deps.sh" "$T/m" "$T" a.bin >/dev/null 2>&1; then fail "verifier accepted a TAMPERED asset"; fi
echo "release pin tripwire: OK"
