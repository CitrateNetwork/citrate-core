#!/usr/bin/env bash
# created: 2026-11-27 | branch: feat/v60-l0-wp01b-seam-exemption
# author: Larry Klosowski (L0 lane, L0-WP01b; Claude Opus 5.5)
# status: active
#
# End-to-end tests for lane-guard's decision (`cx-ownership-check.sh --guard`), run by the
# `lane-guard selftest` job. Each case builds a commit in a scratch git repo and runs the guard the
# way lane-guard.yml does: the checker and map from a separate policy dir (the real map of this
# checkout), the judged commit by sha, labels and the characterization result through env.
#
# Two groups:
#   characterization: what the guard did before the seam exemption, for every branch shape; these
#     must never change.
#   seam: the L0 seam exemption (swarm lead decision 2026-11-27, 09_SWARM_EXECUTION 1-2). A
#     feat/v60-l0-wp(01..10)[a-z]-<slug> branch labelled `seam`, with the characterization job
#     green, may change another lane's file only when .agentile/v60-seams.allow lists that exact
#     path for its WP.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

POLICY="$WORK/policy"
mkdir -p "$POLICY/scripts" "$POLICY/.agentile"
cp "$HERE/scripts/cx-ownership-check.sh" "$POLICY/scripts/"
cp "$HERE/.agentile/v60-ownership.map" "$POLICY/.agentile/"

REPO="$WORK/pr"
git init -q "$REPO"
g() { git -C "$REPO" -c user.name=lane-guard-test -c user.email=test@invalid -c commit.gpgsign=false "$@"; }

# The base: one file per owner the cases need (owners from the real map).
put() { mkdir -p "$REPO/$(dirname "$1")"; printf '%s\n' "${2:-x}" >"$REPO/$1"; }
put src/shell/store.ts                    # l0
put src/shell/actions/l4/pins.ts          # l4
put src-tauri/src/cluster_tests.rs        # l7
put src-tauri/src/faucet_tests.rs         # l2
put src/fl/datasetBridge.test.ts          # l8
put src-tauri/addresses/40204.json        # frozen
g add -A && g commit -qm base
BASE="$(g rev-parse HEAD)"

fails=0
n=0

# case <want pass|fail> <name> <branch> <labels, comma list> <characterization> [expect-in-output]
# The case's changes are applied by the caller to the work tree first (see `edit` and `allow`) and
# committed here; NO_COMMIT=1 judges the current HEAD and leaves the work tree uncommitted.
case_() {
  local want="$1" name="$2" branch="$3" labels="$4" char="$5" expect="${6:-}" head out rc=0 got
  [[ -n "${NO_COMMIT:-}" ]] || { g add -A && g commit -qm "$name" --allow-empty; }
  head="$(g rev-parse HEAD)"
  out="$(LG_LABELS="$(printf '%s' "$labels" | tr ',' '\n')" LG_CHARACTERIZATION="$char" \
    CX_DIFF_ROOT="$REPO" CX_DIFF_HEAD="$head" GITHUB_ACTIONS='' \
    bash "$POLICY/scripts/cx-ownership-check.sh" --guard "$branch" "$BASE" 2>&1)" || rc=$?
  if [[ $rc -eq 0 ]]; then got=pass; else got=fail; fi
  n=$((n + 1))
  if [[ "$got" != "$want" ]]; then
    echo "  TEST FAIL: $name: want=$want got=$got (branch=$branch labels=$labels char=$char)"
    printf '%s\n' "$out" | sed 's/^/      | /'
    fails=$((fails + 1))
  elif [[ -n "$expect" && "$out" != *"$expect"* ]]; then
    echo "  TEST FAIL: $name: $got as wanted, but the output does not name '$expect'"
    printf '%s\n' "$out" | sed 's/^/      | /'
    fails=$((fails + 1))
  else
    echo "  ok: $name -> $got"
  fi
  # Every case starts again from the base.
  g reset -q --hard "$BASE"
  g clean -qfdx
}
edit() { local f; for f in "$@"; do put "$f" "changed by the case"; done; }
allow() { mkdir -p "$REPO/.agentile"; printf '%s\n' "$@" >>"$REPO/.agentile/v60-seams.allow"; }

echo "cx-ownership-check --guard: characterization (behaviour before the seam exemption)"
edit src/shell/actions/l4/pins.ts
case_ pass char-own-lane-file feat/v60-l4-wp07-pins "" ""
edit src/shell/store.ts
case_ fail char-lane-touches-hotspot feat/v60-l5-wp02-hf-caps "" "" "src/shell/store.ts (owner: l0)"
edit src/shell/store.ts src-tauri/addresses/40204.json
case_ pass char-not-a-v60-branch fix/055-sidecar-flake "" "" "does not apply"
edit src/shell/store.ts
case_ fail char-malformed-v60-branch feat/v60-l5-hf-caps "" "" "branch name must be"
edit src-tauri/addresses/40204.json
case_ fail char-chain-touches-frozen feat/v60-chain-r1008-pin "" "" "40204.json (owner: frozen)"
edit src-tauri/src/cluster_tests.rs
case_ pass char-chain-touches-other feat/v60-chain-r1008-pin "" ""
edit src/shell/store.ts
case_ pass char-l0-own-file feat/v60-l0-wp11-ratchets "" ""
edit src-tauri/src/cluster_tests.rs
case_ fail char-l0-foreign-file feat/v60-l0-wp11-ratchets "" "" "cluster_tests.rs (owner: l7)"
edit src/shell/store.ts src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail char-seam-branch-no-label feat/v60-l0-wp03-command-registry "" success "cluster_tests.rs (owner: l7)"
edit src/shell/store.ts
case_ pass char-seam-branch-own-files feat/v60-l0-wp03-command-registry "" ""

echo "cx-ownership-check --guard: L0 seam exemption"
# The four cases the swarm lead named (2026-11-27).
edit src/shell/store.ts src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-pr-without-label feat/v60-l0-wp03-command-registry "lane:l0" success "cluster_tests.rs (owner: l7)"
edit src/shell/store.ts src-tauri/src/cluster_tests.rs src-tauri/src/faucet_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-pr-unlisted-foreign-file feat/v60-l0-wp03-command-registry seam success "faucet_tests.rs (owner: l2)"
edit src/shell/store.ts
case_ fail non-seam-branch-with-label feat/v60-l0-wp11-ratchets seam success "label 'seam'"
edit src/shell/store.ts src-tauri/src/cluster_tests.rs src/fl/datasetBridge.test.ts
allow "# L0-WP03 (lanes/L0_INTEGRATION_SEAMS.md): structural tests that read the handler list" \
  "l0-wp03 src-tauri/src/cluster_tests.rs" "l0-wp03 src/fl/datasetBridge.test.ts"
case_ pass seam-pr-correct feat/v60-l0-wp03-command-registry "lane:l0,seam" success "seam exemption (l0-wp03)"
# The rest of the boundary.
edit src/shell/actions/l4/pins.ts
case_ fail non-seam-lane-branch-with-label feat/v60-l4-wp03-pins seam success "label 'seam'"
edit src/shell/store.ts
case_ fail non-v60-branch-with-label fix/055-sidecar-flake seam success "label 'seam'"
edit src/shell/store.ts
case_ fail seam-range-ends-at-wp10 feat/v60-l0-wp11-ratchets seam success "label 'seam'"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp10 src-tauri/src/cluster_tests.rs"
case_ pass seam-wp10 feat/v60-l0-wp10-deps seam success
edit src-tauri/src/cluster_tests.rs
allow "l0-wp01b src-tauri/src/cluster_tests.rs"
case_ pass seam-follow-up-letter feat/v60-l0-wp01b-seam-exemption seam success
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-follow-up-needs-own-rows feat/v60-l0-wp03a-acl-split seam success "cluster_tests.rs (owner: l7)"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp04 src-tauri/src/cluster_tests.rs"
case_ fail seam-other-wp-rows-do-not-count feat/v60-l0-wp03-command-registry seam success "cluster_tests.rs (owner: l7)"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-characterization-red feat/v60-l0-wp03-command-registry seam failure "characterization"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-characterization-missing feat/v60-l0-wp03-command-registry seam "" "characterization"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-label-must-be-exact feat/v60-l0-wp03-command-registry "seam-ish,Seam" success "cluster_tests.rs (owner: l7)"
edit src-tauri/addresses/40204.json
allow "l0-wp03 src-tauri/addresses/40204.json"
case_ fail seam-never-reaches-frozen feat/v60-l0-wp03-command-registry seam success "40204.json (owner: frozen)"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/*_tests.rs"
case_ fail seam-allowlist-takes-no-globs feat/v60-l0-wp03-command-registry seam success "exact path"
edit src-tauri/src/cluster_tests.rs
allow "l0-wp03 src-tauri/src/../src/cluster_tests.rs"
case_ fail seam-allowlist-takes-no-dot-segments feat/v60-l0-wp03-command-registry seam success "exact path"
edit src-tauri/src/cluster_tests.rs
allow "l3-wp03 src-tauri/src/cluster_tests.rs"
case_ fail seam-allowlist-rows-are-l0-wps feat/v60-l0-wp03-command-registry seam success "allowlist row"
edit src-tauri/src/cluster_tests.rs
case_ fail seam-no-allowlist-file feat/v60-l0-wp03-command-registry seam success "cluster_tests.rs (owner: l7)"
# The allowlist is read from the judged commit, not from the work tree.
edit src-tauri/src/cluster_tests.rs
g add -A && g commit -qm "no allowlist in the commit"
allow "l0-wp03 src-tauri/src/cluster_tests.rs"
NO_COMMIT=1 case_ fail seam-allowlist-from-the-judged-commit feat/v60-l0-wp03-command-registry seam success "cluster_tests.rs (owner: l7)"

if [[ $fails -eq 0 ]]; then echo "GUARD TESTS: PASS ($n cases)"; else echo "GUARD TESTS: FAIL ($fails of $n)"; exit 1; fi
