#!/usr/bin/env bash
# cx-copy-lint.sh — compliance guard for shipped copy (planset RT-4 / 06_SECURITY §3).
#
# The federation's public-copy rules bind CX surfaces: storage/training rewards are
# subsidy/grant-framed (D-22), reach is scoped to "your Groups" until the native<->web bridge
# exists (RT-6), and "up to 2000" only after the soak passes (RT-3). This greps shipped UI +
# machine-readable assets for forbidden phrasings and fails the build.
#
#     scripts/cx-copy-lint.sh                 # lints src/**, docs meant to ship, well-knowns
#     scripts/cx-copy-lint.sh --selftest      # verifies the patterns fire, no repo state
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Forbidden case-insensitive regexes (extended). Keep each with a WHY so reviewers can amend.
FORBIDDEN=(
  'earn +salt +by +storing'          # D-22: it is a subsidy pool, not user earnings
  'pay +to +store'                    # D-22: the user posts a refundable bond, not a payment
  'get +paid +to +pin'                # D-22 framing
  'guaranteed +(salt|rewards?|returns?|income)'  # no guaranteed earnings (compliance)
  'day.?one +(cash|earnings?)'        # no day-one cash earnings
  'message +anyone +on +citrate'      # RT-6: reach not delivered until the web<->native bridge
)
# Conditional: "up to 2000" (or 2,000) is allowed ONLY in files that carry the soak-proof tag.
SCALE_RE='up +to +2[,]?000'
SOAK_TAG='cx-scale-soak-proven'

lint_paths() {
  local paths=("$@") hits=0 re f
  for re in "${FORBIDDEN[@]}"; do
    while IFS= read -r f; do
      [[ -z "$f" ]] && continue
      echo "  FORBIDDEN COPY [$re]: $f" ; hits=1
    done < <(grep -rilE "$re" "${paths[@]}" 2>/dev/null || true)
  done
  # scale claim: flag only if the file lacks the soak-proof tag
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    grep -qi "$SOAK_TAG" "$f" 2>/dev/null && continue
    echo "  UNPROVEN SCALE CLAIM [$SCALE_RE, missing $SOAK_TAG]: $f" ; hits=1
  done < <(grep -rilE "$SCALE_RE" "${paths[@]}" 2>/dev/null || true)
  return $hits
}

selftest() {
  local tmp; tmp="$(mktemp -d)"
  printf 'You can earn SALT by storing files!\n' > "$tmp/bad1.tsx"
  printf 'Message anyone on Citrate.\n'          > "$tmp/bad2.tsx"
  printf 'Scale up to 2000 nodes.\n'             > "$tmp/bad3.md"      # no soak tag -> flagged
  printf 'Up to 2000 nodes. cx-scale-soak-proven\n' > "$tmp/ok1.md"   # tagged -> allowed
  printf 'The network rewards pinners for keeping data available.\n' > "$tmp/ok2.tsx"
  echo "cx-copy-lint --selftest"
  local out; out="$(lint_paths "$tmp" || true)"
  echo "$out"
  local ok=1
  grep -q 'bad1.tsx' <<<"$out" || { echo "  MISS: bad1"; ok=0; }
  grep -q 'bad2.tsx' <<<"$out" || { echo "  MISS: bad2"; ok=0; }
  grep -q 'bad3.md'  <<<"$out" || { echo "  MISS: bad3"; ok=0; }
  grep -q 'ok1.md'   <<<"$out" && { echo "  FALSE-POSITIVE: ok1 (tagged)"; ok=0; }
  grep -q 'ok2.tsx'  <<<"$out" && { echo "  FALSE-POSITIVE: ok2"; ok=0; }
  rm -rf "$tmp"
  [[ $ok -eq 1 ]] && { echo "SELFTEST: PASS"; return 0; } || { echo "SELFTEST: FAIL"; return 1; }
}

main() {
  if [[ "${1:-}" == "--selftest" ]]; then selftest; return; fi
  local targets=()
  [[ -d "$REPO_ROOT/src" ]] && targets+=("$REPO_ROOT/src")
  [[ -d "$REPO_ROOT/public" ]] && targets+=("$REPO_ROOT/public")
  [[ ${#targets[@]} -eq 0 ]] && { echo "no shippable copy dirs found"; return 0; }
  if lint_paths "${targets[@]}"; then
    echo "copy-lint OK — no forbidden reward/reach/scale phrasings in shipped copy"
  else
    echo "COPY-LINT FAILED — fix the phrasings above (planset RT-4 / 06 §3)." >&2
    return 1
  fi
}
main "$@"
