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
  use_map cx
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

  # v60 map (.agentile/v60-ownership.map): the LAST matching row owns a file, '*' stays inside one
  # path segment and '**' spans segments (the CODEOWNERS rules, so both files agree).
  use_map v60
  check fail l5 src/shell/store.ts               # hotspot: owner l0 -> a lane PR must fail
  check pass l4 src/shell/actions/l4/pins.ts     # a lane's own action module
  check fail l4 src/shell/actions/l1/turn.ts     # another lane's action module
  check pass l0 src-tauri/src/lib.rs
  check fail l1 src-tauri/permissions/l2-commands.toml
  check pass l1 src-tauri/src/hermes_mcp.rs
  check pass l1 src-tauri/src/model_proxy_http.rs
  check fail l5 src-tauri/src/model_proxy_http.rs # L5's model rows must not reach L1's proxy
  check pass l2 src-tauri/src/hic/engine.rs
  check pass l2 kit/src/ceremony.rs              # D-12: ceremony code is L2
  check pass l3 src/onboarding/Onboarding.tsx
  check fail l3 src/onboarding/LocalModelControls.tsx # the later, narrower L5 row wins
  check pass l5 src/onboarding/LocalModelControls.tsx
  check fail l5 src-tauri/src/media_intake.rs    # L4's file, not under L5's media rows
  check pass l4 src-tauri/src/social.rs          # shared row: L4 and L6
  check pass l6 src-tauri/src/social.rs
  check pass l6 components/src/mcp_subregistry.rs
  check fail l2 components/src/mcp_subregistry.rs
  check pass l7 src-tauri/src/cluster_mesh.rs
  check pass l7 release/signoffs/CL-S4_PRIVATE_LINKS_ADDENDUM.md
  check fail l7 release/signoffs/CL-S4.json      # signed records stay L0 (owner-pinned review)
  check pass l8 src/fl/RoundsRoute.tsx
  check fail l8 src-tauri/src/fl_rounds/inner.rs # '*' in fl_*.rs must not cross '/'
  check pass l9 src-tauri/src/pqc_seal.rs
  check pass lq scripts/ci/lq/chain-touch.mjs
  check fail l0 .agentile/coverage/TRIPWIRES.md  # LQ's, inside L0's coverage dir
  check fail l3 src/shell/places.ts              # L0 until WP08 lands the route registry
  check fail l0 src-tauri/addresses/40204.json   # frozen: no lane branch may touch it
  check fail l1 .agentile/v60-ownership.map      # only L0 edits the map
  check pass l4 .agentile/sprints/active/v60/SPRINT.md # always allowed
  check pass l0 src/surfaces/Unclaimed.tsx       # unclaimed paths fall back to L0
  check fail l9 src/surfaces/Unclaimed.tsx

  # Branch name -> lane (lane-guard.yml). 1 = not a lane branch (skip), 2 = malformed v60 branch.
  lane_case() { # <want lane|none|bad> <branch>
    local got rc=0
    got="$(lane_of "$2")" || rc=$?
    case "$rc" in 0) ;; 1) got=none ;; *) got=bad ;; esac
    if [[ "$got" != "$1" ]]; then echo "  SELFTEST FAIL: branch=$2 want=$1 got=$got"; fails=1
    else echo "  ok: branch=$2 -> $got"; fi
  }
  lane_case l5 feat/v60-l5-wp02-hf-caps
  lane_case lq feat/v60-lq-wp04-chain-touch
  lane_case l0 feat/v60-l0-wp24-release-0.6.0
  lane_case l2 feat/v60-l2-wp06b-ceremony-budget
  lane_case none fix/055-sidecar-flake
  lane_case none feat/v60-chain-r1008-pin        # chain lane: LQ's chain-touch guard
  lane_case bad feat/v60-l5-hf-caps              # no wp number
  lane_case bad feat/v60-L5-wp02-hf-caps
  lane_case bad feat/v60-l10-wp01-x
  lane_case bad 'feat/v60-l5-wp02-$(id)'
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
