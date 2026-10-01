#!/usr/bin/env bash
# HUP-S6.2 template gate: render every template with sample parameters into a
# work directory, then run the real toolchain on the result.
#
#   contract templates: forge build + forge test (unit, fuzz and the Medusa
#                       property harness as Foundry invariants); medusa.json is
#                       checked as JSON. `medusa fuzz` runs only when medusa is
#                       installed; otherwise the script says so.
#   hello-mint app:     tsc --noEmit and vite build, when --node-modules is given.
#
# Dependencies are fetched once into --deps-cache at the commits pinned in
# deps.lock.json, and each checkout's commit is verified. Nothing is deployed.
#
# usage: verify-templates.sh --work DIR --deps-cache DIR [--tier T0|T1|T2]
#                            [--node-modules DIR] [--medusa] [template-id...]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORE="$(cd "$ROOT/.." && pwd)"
WORK=""
CACHE=""
TIER="T1"
NODE_MODULES=""
RUN_MEDUSA=0
IDS=()

while [ $# -gt 0 ]; do
  case "$1" in
    --work) WORK="$2"; shift 2 ;;
    --deps-cache) CACHE="$2"; shift 2 ;;
    --tier) TIER="$2"; shift 2 ;;
    --node-modules) NODE_MODULES="$2"; shift 2 ;;
    --medusa) RUN_MEDUSA=1; shift ;;
    -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
    *) IDS+=("$1"); shift ;;
  esac
done
[ -n "$WORK" ] && [ -n "$CACHE" ] || { echo "verify-templates: --work and --deps-cache are required" >&2; exit 2; }
command -v forge >/dev/null || { echo "verify-templates: forge is not installed" >&2; exit 2; }
command -v jq >/dev/null || { echo "verify-templates: jq is not installed" >&2; exit 2; }

TARGET="${CARGO_TARGET_DIR:-$CORE/target}"
(cd "$CORE" && cargo build -q -p citrate-templates)
BIN="$TARGET/debug/citrate-templates"

# ---- dependencies at the pinned commits ----
mkdir -p "$CACHE"
for dep in $(jq -r '.deps | keys[]' "$ROOT/deps.lock.json"); do
  url=$(jq -r ".deps[\"$dep\"].url" "$ROOT/deps.lock.json")
  tag=$(jq -r ".deps[\"$dep\"].tag" "$ROOT/deps.lock.json")
  commit=$(jq -r ".deps[\"$dep\"].commit" "$ROOT/deps.lock.json")
  if [ ! -d "$CACHE/$dep/.git" ]; then
    git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" "$url" "$CACHE/$dep"
  fi
  got=$(git -C "$CACHE/$dep" rev-parse HEAD)
  if [ "$got" != "$commit" ]; then
    echo "verify-templates: $dep is at $got, deps.lock.json pins $commit" >&2
    exit 1
  fi
done

if [ ${#IDS[@]} -eq 0 ]; then
  while IFS= read -r id; do IDS+=("$id"); done < <("$BIN" list --root "$ROOT" | jq -r '.[].id')
fi

OWNER="0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
mkdir -p "$WORK"
FAILED=()

check_forge_project() {
  local dir="$1" id="$2"
  mkdir -p "$dir/lib"
  for dep in $(jq -r '.deps | keys[]' "$ROOT/deps.lock.json"); do
    cp -R "$CACHE/$dep" "$dir/lib/$dep"
  done
  jq -e '.fuzzing.testLimit > 0' "$dir/medusa.json" >/dev/null
  echo "== $id: forge build"
  (cd "$dir" && forge build --offline) || return 1
  echo "== $id: forge test"
  (cd "$dir" && forge test --offline) || return 1
  if [ "$RUN_MEDUSA" -eq 1 ]; then
    if command -v medusa >/dev/null; then
      echo "== $id: medusa fuzz ($TIER budget)"
      (cd "$dir" && medusa fuzz --config medusa.json) || return 1
    else
      echo "== $id: medusa is not installed; the property harness ran under forge invariants only"
    fi
  fi
}

for id in "${IDS[@]}"; do
  out="$WORK/$id"
  rm -rf "$out"
  kind=$("$BIN" list --root "$ROOT" | jq -r --arg id "$id" '.[] | select(.id == $id) | .kind')
  args=(--param "name=Lemon Drops" --param "symbol=LEMON" --param "owner=$OWNER")
  has() { "$BIN" list --root "$ROOT" | jq -e --arg id "$id" --arg k "$1" '.[] | select(.id == $id) | .params | has($k)' >/dev/null; }
  has supply && args+=(--param "supply=500")
  has price && args+=(--param "price=5000000000000000000")
  echo "== $id: render ($TIER)"
  "$BIN" render --root "$ROOT" --template "$id" --tier "$TIER" --out "$out" "${args[@]}" >/dev/null
  if [ "$kind" = "contract" ]; then
    check_forge_project "$out" "$id" || FAILED+=("$id")
  else
    if [ -d "$out/contracts" ]; then
      check_forge_project "$out/contracts" "$id/contracts" || FAILED+=("$id/contracts")
    fi
    if [ -d "$out/app" ]; then
      if [ -n "$NODE_MODULES" ]; then
        ln -s "$NODE_MODULES" "$out/app/node_modules"
        echo "== $id/app: tsc --noEmit"
        (cd "$out/app" && ./node_modules/.bin/tsc --noEmit -p tsconfig.json) || FAILED+=("$id/app tsc")
        echo "== $id/app: vite build"
        (cd "$out/app" && VITE_CONTRACT_ADDRESS="$OWNER" ./node_modules/.bin/vite build --logLevel warn) || FAILED+=("$id/app build")
        rm "$out/app/node_modules"
      else
        echo "== $id/app: skipped (pass --node-modules to type-check and build the app)"
      fi
    fi
  fi
done

if [ ${#FAILED[@]} -gt 0 ]; then
  echo "verify-templates: FAILED: ${FAILED[*]}" >&2
  exit 1
fi
echo "verify-templates: all passed (${IDS[*]})"
