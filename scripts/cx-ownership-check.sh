#!/usr/bin/env bash
# cx-ownership-check.sh — the mechanical race-guard for the CX planset (01_SCOPE §5.1).
#
# A feature WP must only touch files its lane OWNS (.agentile/cx-ownership.map, from
# 02_ARCHITECTURE §3.3). This asserts that a branch's diff vs a base ref is a subset of its
# lane's owned set. Run in CI / before every merge:
#
#     scripts/cx-ownership-check.sh <lane> [base-ref]      # default base = origin/main
#     scripts/cx-ownership-check.sh --selftest             # unit-checks the matcher, no git
#
# Lanes: s0 s1 s2 s3 s4 s5 s6 s7  (s0 = scaffold, owns the spine; see the map header).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MAP="$REPO_ROOT/.agentile/cx-ownership.map"

# In [[ str == pat ]], '*' spans '/', so a trailing '**' or '*' matches any subpath.
# Returns 0 if $file is allowed for $lane (its own globs + the always-allowed '*' globs).
file_allowed() {
  local lane="$1" file="$2" l pat
  while read -r l pat; do
    [[ -z "${l:-}" || "${l:0:1}" == "#" ]] && continue
    if [[ "$l" == "$lane" || "$l" == "*" ]]; then
      # shellcheck disable=SC2053
      [[ "$file" == $pat ]] && return 0
    fi
  done < "$MAP"
  return 1
}

selftest() {
  local fails=0
  check() { # <expect pass|fail> <lane> <file>
    if file_allowed "$2" "$3"; then local got=pass; else local got=fail; fi
    if [[ "$got" != "$1" ]]; then echo "  SELFTEST FAIL: lane=$2 file=$3 want=$1 got=$got"; fails=1
    else echo "  ok: lane=$2 file=$3 -> $got"; fi
  }
  echo "cx-ownership-check --selftest"
  check pass s1 src-tauri/src/model.rs           # s1 owns model.rs
  check pass s1 src/surfaces/Models.tsx          # s1 owns its surface
  check pass s1 src/bridge/tauri/models.ts       # s1 owns its bridge impl
  check fail s1 src/surfaces/Groups.tsx          # Groups belongs to s3 -> RACE, must fail
  check fail s1 src/bridge/domains.ts            # the spine belongs to s0 only -> must fail
  check fail s1 src-tauri/src/lib.rs             # the command registry is s0-frozen -> must fail
  check pass s3 src-tauri/src/comms.rs           # s3 owns comms.rs
  check fail s3 src-tauri/src/storage.rs         # storage is s2 -> must fail
  check pass s0 src/bridge/domains.ts            # s0 owns the whole spine
  check pass s0 src-tauri/src/lib.rs
  check pass s2 .agentile/sprints/active/sprint-cx-s2/SPRINT.md   # always-allowed
  check fail s4 src/surfaces/Train.tsx           # Train is s5 -> must fail
  if [[ $fails -eq 0 ]]; then echo "SELFTEST: PASS"; else echo "SELFTEST: FAIL"; return 1; fi
}

main() {
  if [[ "${1:-}" == "--selftest" ]]; then selftest; return; fi
  local lane="${1:?usage: cx-ownership-check.sh <lane> [base-ref] | --selftest}"
  local base="${2:-origin/main}"
  local changed offenders=()
  changed="$(cd "$REPO_ROOT" && git diff --name-only "${base}...HEAD" 2>/dev/null || git diff --name-only "$base" 2>/dev/null)"
  [[ -z "$changed" ]] && { echo "no changes vs $base — nothing to check"; return 0; }
  while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    file_allowed "$lane" "$f" || offenders+=("$f")
  done <<< "$changed"
  if [[ ${#offenders[@]} -gt 0 ]]; then
    echo "OWNERSHIP VIOLATION — lane '$lane' touched files it does not own (race risk):" >&2
    printf '  %s\n' "${offenders[@]}" >&2
    echo "Fix: move the change into the owning lane, or open a serialized spine-PR (01 §5.2)." >&2
    return 1
  fi
  echo "ownership OK — lane '$lane' diff vs $base is within its owned set (${changed//$'\n'/, })"
}
main "$@"
