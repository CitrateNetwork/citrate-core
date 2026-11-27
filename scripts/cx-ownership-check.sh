#!/usr/bin/env bash
# cx-ownership-check.sh — the mechanical race-guard for the CX planset (01_SCOPE §5.1).
#
# A feature WP must only touch files its lane OWNS (.agentile/cx-ownership.map, from
# 02_ARCHITECTURE §3.3). This asserts that a branch's diff vs a base ref is a subset of its
# lane's owned set. Run in CI / before every merge:
#
#     scripts/cx-ownership-check.sh <lane> [base-ref]      # default base = origin/main
#     scripts/cx-ownership-check.sh --map v60 <lane> [base-ref]
#     scripts/cx-ownership-check.sh --lane-of <branch>     # feat/v60-l<N>-wpNN-<slug> -> l<N>
#     scripts/cx-ownership-check.sh --selftest             # unit-checks the matcher, no git
#
# Maps: cx (default; lanes s0..s7, s0 = scaffold, owns the spine; see the map header) and v60
# (.agentile/v60-ownership.map, lanes l0..l9 and lq; read by lane-guard.yml, written into
# .github/CODEOWNERS by scripts/gen-codeowners.mjs).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The checkout whose diff is judged. lane-guard runs the base's copy of this script and map from a
# temp dir (so a PR cannot change the rules it is judged by) and points this at the PR checkout.
DIFF_ROOT="${CX_DIFF_ROOT:-$REPO_ROOT}"
# The commit judged: lane-guard passes the event sha, so a moved HEAD cannot shrink the diff.
DIFF_HEAD="${CX_DIFF_HEAD:-HEAD}"
MAP_NAME=cx
MAP="$REPO_ROOT/.agentile/cx-ownership.map"
V60_LANES=()
V60_RES=()
V60_CREATE_LANES=()
V60_CREATE_RES=()

use_map() { # cx | v60
  case "$1" in
    cx)  MAP_NAME=cx;  MAP="$REPO_ROOT/.agentile/cx-ownership.map" ;;
    v60) MAP_NAME=v60; MAP="$REPO_ROOT/.agentile/v60-ownership.map"; load_v60 ;;
    *) echo "unknown map '$1' (cx | v60)" >&2; return 2 ;;
  esac
}

# v60 globs follow CODEOWNERS: '**' spans '/', '*' and '?' stay inside one path segment.
glob_to_ere() {
  local g="$1" re="" c i n=${#1}
  for ((i = 0; i < n; i++)); do
    c="${g:i:1}"
    case "$c" in
      '*') if [[ "${g:i+1:1}" == '*' ]]; then re+='.*'; i=$((i + 1)); else re+='[^/]*'; fi ;;
      '?') re+='[^/]' ;;
      '.'|'+'|'('|')'|'{'|'}'|'['|']'|'^'|'$'|'|'|'\') re+="\\$c" ;;
      *) re+="$c" ;;
    esac
  done
  printf '%s' "$re"
}

load_v60() {
  local l pat
  [[ -f "$MAP" ]] || { echo "missing $MAP" >&2; return 2; }
  V60_LANES=(); V60_RES=(); V60_CREATE_LANES=(); V60_CREATE_RES=()
  while read -r l pat || [[ -n "${l:-}" ]]; do # '||': keep a last row with no trailing newline
    [[ -z "${l:-}" || "${l:0:1}" == "#" ]] && continue
    # CODEOWNERS reads 'dir/' as the directory, this matcher as a literal path: refuse the form.
    [[ "$pat" == */ ]] && { echo "v60 map: glob '$pat' ends in '/'; write '${pat}**'" >&2; return 2; }
    if [[ "$l" == create:* ]]; then # seed grant: never an owner, see v60_may_create
      V60_CREATE_LANES+=("${l#create:}"); V60_CREATE_RES+=("^$(glob_to_ere "$pat")\$")
      continue
    fi
    V60_LANES+=("$l"); V60_RES+=("^$(glob_to_ere "$pat")\$")
  done < "$MAP"
}

# The lanes (comma list) of the LAST matching non-'*' row; empty when no row claims the file.
v60_owner() {
  local file="$1" i owner=""
  for ((i = 0; i < ${#V60_LANES[@]}; i++)); do
    [[ "${V60_LANES[i]}" == "*" ]] && continue
    [[ "$file" =~ ${V60_RES[i]} ]] && owner="${V60_LANES[i]}"
  done
  printf '%s' "$owner"
}

# 'create:<lanes> <glob>' rows: those lanes may ADD a matching file absent at the base (L0 seeding
# another lane's registration point, section 2.2). Editing or deleting it stays with its owner.
v60_may_create() {
  local lane="$1" file="$2" i
  for ((i = 0; i < ${#V60_CREATE_LANES[@]}; i++)); do
    [[ ",${V60_CREATE_LANES[i]}," == *",$lane,"* && "$file" =~ ${V60_CREATE_RES[i]} ]] && return 0
  done
  return 1
}

v60_always_allowed() {
  local file="$1" i
  for ((i = 0; i < ${#V60_LANES[@]}; i++)); do
    [[ "${V60_LANES[i]}" == "*" && "$file" =~ ${V60_RES[i]} ]] && return 0
  done
  return 1
}

# Branch -> lane for lane-guard. Prints the lane and returns 0; returns 1 for a branch the guard
# does not govern (not feat/v60-*); returns 2 for a feat/v60-* branch that is neither
# feat/v60-l<N>-wpNN[a-z]-<slug> nor feat/v60-chain-<slug>. The chain lane gets 'chain': here it is
# held to the frozen paths only, and LQ's chain-touch guard judges the rest.
lane_of() {
  local b="$1"
  [[ "$b" == feat/v60-* ]] || return 1
  if [[ "$b" =~ ^feat/v60-chain-[a-z0-9][a-z0-9.-]*$ ]]; then printf chain; return 0; fi
  if [[ "$b" =~ ^feat/v60-(l[0-9]|lq)-wp[0-9]{2}[a-z]?-[a-z0-9][a-z0-9.-]*$ ]]; then
    printf '%s' "${BASH_REMATCH[1]}"; return 0
  fi
  return 2
}

# Returns 0 if $file is allowed for $lane. $3 is the file's git status in the diff (A added,
# D deleted, M or anything else modified; default M).
# cx: in [[ str == pat ]], '*' spans '/', so a trailing '**' or '*' matches any subpath; a file is
# allowed by any of the lane's globs plus the always-allowed '*' globs. The status is not used.
# v60: the owner is the last matching row (see v60_owner); '*' rows are always allowed; a 'create:'
# row also allows an added file; frozen files are allowed to no lane; 'chain' may touch any file
# that is not frozen.
file_allowed() {
  local lane="$1" file="$2" status="${3:-M}" l pat owner
  if [[ "$MAP_NAME" == v60 ]]; then
    v60_always_allowed "$file" && return 0
    owner="$(v60_owner "$file")"
    [[ "$owner" == frozen ]] && return 1
    [[ "$lane" == chain ]] && return 0
    [[ ",$owner," == *",$lane,"* ]] && return 0
    [[ "$status" == A ]] && v60_may_create "$lane" "$file" && return 0
    return 1
  fi
  while read -r l pat || [[ -n "${l:-}" ]]; do
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
  check() { # <expect pass|fail> <lane> <file> [git status: A added, M modified (default), D deleted]
    local st="${4:-M}"
    if file_allowed "$2" "$3" "$st"; then local got=pass; else local got=fail; fi
    if [[ "$got" != "$1" ]]; then echo "  SELFTEST FAIL: lane=$2 file=$3 status=$st want=$1 got=$got"; fails=1
    else echo "  ok: lane=$2 file=$3 status=$st -> $got"; fi
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
  # L0 seeds the per-lane registration points (section 2.2, WP03 to WP08): it may ADD one that does
  # not exist at the base; editing or deleting it afterwards is the owning lane's alone.
  check pass l0 src-tauri/src/commands/l1.rs A
  check pass l0 src-tauri/permissions/l3-commands.toml A
  check pass l0 src/bridge/lanes/l5.ts A
  check pass l0 src/shell/actions/l4/x.ts A
  check pass l0 src-tauri/src/lanes/l2/hooks.rs A
  check pass l0 src/shell/routes/l6.tsx A
  check fail l0 src-tauri/src/commands/l1.rs M   # exists at the base: L1's file
  check fail l0 src/bridge/lanes/l5.ts D         # deleting is not creating
  check fail l2 src-tauri/src/commands/l1.rs A   # only L0 seeds another lane's point
  check fail l0 src-tauri/src/provider_new.rs A  # L1 code, not a registration point
  check fail l0 src-tauri/addresses/40204.json A # frozen beats the seed grant
  check pass l1 src-tauri/src/commands/l1.rs A   # the lane may add its own point too
  # Chain lane branches in this repo: frozen paths only (LQ's chain-touch guard covers the rest).
  check fail chain src-tauri/addresses/40204.json
  check fail chain src-tauri/network/profiles.json
  check pass chain src-tauri/src/node_pin.rs

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
  lane_case l2 feat/v60-l2-wp99b-split-example
  lane_case none fix/055-sidecar-flake
  lane_case chain feat/v60-chain-r1008-pin       # chain lane: frozen paths here, the rest in LQ's guard
  lane_case bad 'feat/v60-chain-$(id)'
  lane_case bad feat/v60-l5-hf-caps              # no wp number
  lane_case bad feat/v60-L5-wp02-hf-caps
  lane_case bad feat/v60-l10-wp01-x
  lane_case bad 'feat/v60-l5-wp02-$(id)'
  if [[ $fails -eq 0 ]]; then echo "SELFTEST: PASS"; else echo "SELFTEST: FAIL"; return 1; fi
}

main() {
  if [[ "${1:-}" == "--selftest" ]]; then selftest; return; fi
  if [[ "${1:-}" == "--lane-of" ]]; then lane_of "${2:?--lane-of needs a branch name}"; return; fi
  if [[ "${1:-}" == "--map" ]]; then use_map "${2:?--map needs cx or v60}"; shift 2; fi
  local lane="${1:?usage: cx-ownership-check.sh [--map cx|v60] <lane> [base-ref] | --lane-of <branch> | --selftest}"
  local base="${2:-origin/main}"
  local f st i list offenders=() changed=() statuses=()
  # -z: paths arrive raw and NUL-delimited (no C-quoting of non-ASCII names, no newline splitting).
  # --no-renames: a rename lists both paths (D old, A new), so moving another lane's file out still
  # names it. --name-status: an added file (A) is absent at the base, which a 'create:' row needs.
  list="$(mktemp)"
  if ! (cd "$DIFF_ROOT" && { git diff -z --no-renames --name-status "${base}...${DIFF_HEAD}" >"$list" 2>/dev/null ||
    git diff -z --no-renames --name-status "$base" "$DIFF_HEAD" >"$list" 2>/dev/null; }); then
    rm -f "$list"; echo "cannot diff against '$base' in $DIFF_ROOT" >&2; return 2
  fi
  while IFS= read -r -d '' st && IFS= read -r -d '' f; do
    if [[ -n "$f" ]]; then statuses+=("${st:0:1}"); changed+=("$f"); fi
  done <"$list"
  rm -f "$list"
  [[ ${#changed[@]} -eq 0 ]] && { echo "no changes vs $base — nothing to check"; return 0; }
  for ((i = 0; i < ${#changed[@]}; i++)); do
    f="${changed[i]}"
    if ! file_allowed "$lane" "$f" "${statuses[i]}"; then
      if [[ "$MAP_NAME" == v60 ]]; then offenders+=("$f (owner: $(v60_owner "$f"))"); else offenders+=("$f"); fi
    fi
  done
  if [[ ${#offenders[@]} -gt 0 ]]; then
    echo "OWNERSHIP VIOLATION — lane '$lane' touched files it does not own (race risk):" >&2
    printf '  %s\n' "${offenders[@]}" >&2
    if [[ "$MAP_NAME" == v60 ]]; then
      echo "Fix: move the change into the owning lane, or ask L0 for a seam PR (L0_INTEGRATION_SEAMS §2)." >&2
    else
      echo "Fix: move the change into the owning lane, or open a serialized spine-PR (01 §5.2)." >&2
    fi
    return 1
  fi
  local IFS=,
  echo "ownership OK — lane '$lane' diff vs $base is within its owned set (${changed[*]})"
}
main "$@"
