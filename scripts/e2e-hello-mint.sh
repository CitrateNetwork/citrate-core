#!/usr/bin/env bash
# HUP-S6 g3-gate + g3-e2e (local half), HUP-S11.1 (per-OS run): hello mint end to end on this
# machine against a local anvil fork of chain 40204. Nothing is sent to 40204 itself.
#
#   1. interview   the real agent sidecar's full-project track: GET /tracks, POST /briefs
#   2. render      citrate-templates renders hello-mint from the brief's answers
#   3. verifiers   forge test --json, slither (SARIF, the runtime toolchain's argv), aderyn
#                  (JSON), medusa fuzz at the tier's call budget; raw outputs only
#   4. dry run     the exact init code is deployed on an anvil fork of 40204 (cast send --create)
#   5. gate        the Rust e2e test hands the raw outputs to the production D-4 gate; when READY
#                  it deploys through the real SignatureCeremony signer, verifies the deployed
#                  code against the compiled artifact, and mints one token through a ceremony
#   6. bug         the same steps on a copy with the supply-cap check removed must be NOT READY,
#                  and its deploy must be refused with no ceremony opened
#
# Verifier configuration (--verifiers):
#   default    every tool from PATH; a missing aderyn or medusa makes the run NOT READY
#   test-full  TEST ONLY, never shipped: aderyn and medusa are fetched from the upstream release
#              named in components/toolchain-bundle.json for this platform, sha256-checked against
#              the measured value, and run for real (proves the READY path)
#
# usage: scripts/e2e-hello-mint.sh --work DIR --deps-cache DIR
#          [--verifiers default|test-full] [--tools-dir DIR] [--tier T0|T1|T2]
#          [--sidecar BIN | --no-interview] [--fork-url URL|none] [--evidence DIR]
#          [--allow-unmeasured] [--keep]
#
# --fork-url defaults to https://rpc.citrate.ai (read-only: anvil only reads state from it).
# --fork-url none starts a plain anvil with chain id 40204 instead.
# --allow-unmeasured lets test-full fetch an artifact whose sha256 the bundle has not recorded
#   yet (Linux, Windows); the measured sha256 is printed and written to the evidence so it can
#   be recorded in components/toolchain-bundle.json.
set -euo pipefail

CORE="$(cd "$(dirname "$0")/.." && pwd)"
WORK=""
CACHE=""
VERIFIERS="default"
TOOLS_DIR=""
TIER="T1"
SIDECAR="${CITRATE_E2E_SIDECAR:-}"
INTERVIEW=1
FORK_URL="https://rpc.citrate.ai"
EVIDENCE=""
ALLOW_UNMEASURED=0
KEEP=0
while [ $# -gt 0 ]; do
  case "$1" in
    --work) WORK="$2"; shift 2 ;;
    --deps-cache) CACHE="$2"; shift 2 ;;
    --verifiers) VERIFIERS="$2"; shift 2 ;;
    --tools-dir) TOOLS_DIR="$2"; shift 2 ;;
    --tier) TIER="$2"; shift 2 ;;
    --sidecar) SIDECAR="$2"; shift 2 ;;
    --no-interview) INTERVIEW=0; shift ;;
    --fork-url) FORK_URL="$2"; shift 2 ;;
    --evidence) EVIDENCE="$2"; shift 2 ;;
    --allow-unmeasured) ALLOW_UNMEASURED=1; shift ;;
    --keep) KEEP=1; shift ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "e2e-hello-mint: unknown argument: $1" >&2; exit 2 ;;
  esac
done
die() { echo "e2e-hello-mint: $*" >&2; exit 1; }
say() { echo "e2e-hello-mint | $*"; }
[ -n "$WORK" ] && [ -n "$CACHE" ] || { echo "e2e-hello-mint: --work and --deps-cache are required" >&2; exit 2; }
CONFIG="$CORE/scripts/e2e/verifiers.$VERIFIERS.json"
[ -f "$CONFIG" ] || die "no verifier configuration $CONFIG"
case "$TIER" in T0|T1|T2) ;; *) die "bad tier $TIER" ;; esac
for t in forge anvil cast jq slither git perl curl; do
  command -v "$t" >/dev/null || die "$t is not installed"
done

now_ms() { perl -MTime::HiRes=time -e 'printf("%d\n", time()*1000)'; }
sha256_of() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
platform() {
  local os arch
  os="$(uname -s)"; arch="$(uname -m)"
  case "$os/$arch" in
    Darwin/arm64) echo macos-arm64 ;;
    Darwin/x86_64) echo macos-x64 ;;
    Linux/x86_64) echo linux-x64 ;;
    Linux/aarch64|Linux/arm64) echo linux-arm64 ;;
    MINGW*/*|MSYS*/*|CYGWIN*/*) echo windows-x64 ;;
    *) echo unknown ;;
  esac
}
PLATFORM="$(platform)"

PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done
  if [ "$KEEP" = 0 ]; then rm -rf "$WORK"; fi
}
trap cleanup EXIT
rm -rf "$WORK"; mkdir -p "$WORK" "$CACHE"
WORK="$(cd "$WORK" && pwd)"
mkdir -p "$WORK/good" "$WORK/bug" "$WORK/interview" "$WORK/tools"
[ -n "$TOOLS_DIR" ] || TOOLS_DIR="$WORK/tools"
mkdir -p "$TOOLS_DIR"; TOOLS_DIR="$(cd "$TOOLS_DIR" && pwd)"

# ------------------------------------------------------------------------ tool resolution
# Prints the absolute path of the tool to run, or nothing when it is not available.
fetch_bundle_tool() {
  local name="$1" bundle="$CORE/components/toolchain-bundle.json" art url fmt status want entry file got dir
  art=$(jq -c --arg n "$name" --arg p "$PLATFORM" '.tools[] | select(.name==$n) | .artifacts[$p] // empty' "$bundle")
  [ -n "$art" ] || { echo "e2e-hello-mint: the bundle has no $name artifact for $PLATFORM" >&2; return 0; }
  url=$(echo "$art" | jq -r '.url // empty'); fmt=$(echo "$art" | jq -r '.format')
  status=$(echo "$art" | jq -r '.status'); want=$(echo "$art" | jq -r '.sha256 // empty')
  entry=$(echo "$art" | jq -r '(.entrypoints // [])[0] // empty')
  [ -n "$entry" ] || entry=$(jq -r --arg n "$name" '.tools[] | select(.name==$n) | .entrypoints[0] // empty' "$bundle")
  [ -n "$entry" ] || entry="$name"
  [ -n "$url" ] || { echo "e2e-hello-mint: $name has no upstream artifact for $PLATFORM ($status)" >&2; return 0; }
  if [ "$status" != "measured" ] && [ "$ALLOW_UNMEASURED" = 0 ]; then
    echo "e2e-hello-mint: $name for $PLATFORM is $status (no recorded sha256); pass --allow-unmeasured to measure it" >&2
    return 0
  fi
  dir="$TOOLS_DIR/$name-$PLATFORM"; file="$TOOLS_DIR/$(basename "$url")"
  [ -f "$file" ] || curl -fsSL -o "$file" "$url"
  got=$(sha256_of "$file")
  if [ -n "$want" ] && [ "$got" != "$want" ]; then
    echo "e2e-hello-mint: $name sha256 $got does not match the bundle's $want" >&2
    return 1
  fi
  echo "{\"tool\":\"$name\",\"platform\":\"$PLATFORM\",\"url\":\"$url\",\"sha256\":\"$got\",\"bundleStatus\":\"$status\"}" >> "$WORK/tools-measured.jsonl"
  rm -rf "$dir"; mkdir -p "$dir"
  case "$fmt" in
    tar.gz) tar -xzf "$file" -C "$dir" ;;
    tar.xz) tar -xJf "$file" -C "$dir" ;;
    zip) unzip -q "$file" -d "$dir" ;;
    raw) cp "$file" "$dir/$entry" ;;
    *) echo "e2e-hello-mint: unknown archive format $fmt" >&2; return 1 ;;
  esac
  chmod +x "$dir/$entry" 2>/dev/null || true
  [ -x "$dir/$entry" ] || [ -x "$dir/$entry.exe" ] || { echo "e2e-hello-mint: $entry not found in the $name archive" >&2; return 1; }
  if [ -x "$dir/$entry" ]; then echo "$dir/$entry"; else echo "$dir/$entry.exe"; fi
}
resolve_tool() {
  local name="$1" source
  source=$(jq -r --arg n "$name" '.tools[$n].source // "path"' "$CONFIG")
  case "$source" in
    path) command -v "$name" || true ;;
    bundle) fetch_bundle_tool "$name" ;;
    *) die "verifier configuration: unknown source $source for $name" ;;
  esac
}
ADERYN="$(resolve_tool aderyn)"
MEDUSA="$(resolve_tool medusa)"
# medusa compiles through crytic-compile, which ships with slither; put it on PATH.
SLITHER_BIN="$(command -v slither)"
SLITHER_REAL="$(perl -MCwd=realpath -e 'print realpath($ARGV[0])' "$SLITHER_BIN")"
export PATH="$(dirname "$SLITHER_REAL"):$PATH"
EXPECT="not-ready"; [ -n "$ADERYN" ] && [ -n "$MEDUSA" ] && EXPECT="ready"
say "platform $PLATFORM, verifiers $VERIFIERS, aderyn ${ADERYN:-not installed}, medusa ${MEDUSA:-not installed}, expect $EXPECT"

# anvil's development account 0: the member's wallet in this run (the Rust test opens the same
# public development mnemonic in a vault). No key is stored anywhere by this script.
OWNER=0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
GOAL="help me make an NFT project called Lemon Drops, 500 supply, 5 SALT each"
ANSWERS='{"name":"Lemon Drops","supply":"500","price":"5"}'

# ------------------------------------------------------------------------ 1. interview
if [ "$INTERVIEW" = 1 ]; then
  if [ -z "$SIDECAR" ]; then
    for c in "$CORE/../citrate-agent-runtime/target/debug/citrate-agent-sidecar" "$CORE/../citrate-agent-runtime/target/release/citrate-agent-sidecar"; do
      [ -x "$c" ] && SIDECAR="$c" && break
    done
  fi
  [ -n "$SIDECAR" ] && [ -x "$SIDECAR" ] || die "no agent sidecar binary (pass --sidecar BIN, or --no-interview)"
  SC_PORT="${E2E_SIDECAR_PORT:-19781}"
  TOKEN_FILE="$WORK/interview/token"
  ( umask 077; perl -e 'open(my $f,"<","/dev/urandom") or die; read($f,my $b,24); print unpack("H*",$b)' > "$TOKEN_FILE" )
  mkdir -p "$WORK/interview/capsules"
  CITRATE_HERMES_ADDR="127.0.0.1:$SC_PORT" CITRATE_HERMES_TOKEN_FILE="$TOKEN_FILE" \
    CITRATE_HERMES_CAPSULES="$WORK/interview/capsules" "$SIDECAR" > "$WORK/interview/sidecar.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 1 80); do curl -sf "http://127.0.0.1:$SC_PORT/health" >/dev/null 2>&1 && break; sleep 0.25; done
  AUTH="Authorization: Bearer $(cat "$TOKEN_FILE")"
  curl -sf -H "$AUTH" "http://127.0.0.1:$SC_PORT/tracks" > "$WORK/interview/tracks.json" || die "GET /tracks failed"
  QN=$(jq '[.[] | select(.id=="full-project")][0].questions | length' "$WORK/interview/tracks.json")
  [ "$QN" -ge 1 ] && [ "$QN" -le 5 ] || die "the full-project track asks $QN questions (US-6.1: at most 5)"
  jq -e '[.[] | select(.id=="full-project")][0].questions | all(.default != "")' "$WORK/interview/tracks.json" >/dev/null \
    || die "a full-project question has no default"
  jq -n --arg g "$GOAL" --argjson a "$ANSWERS" '{goal:$g, answers:$a}' \
    | curl -sf -H "$AUTH" -H 'content-type: application/json' -d @- "http://127.0.0.1:$SC_PORT/briefs" \
    > "$WORK/interview/brief.json" || die "POST /briefs failed"
  jq -e '.brief.track=="full-project" and .brief.workflow=="hello-mint"' "$WORK/interview/brief.json" >/dev/null \
    || die "the brief does not name the hello-mint workflow"
  for g in "forge test" Slither Aderyn Medusa "anvil fork" SignatureCeremony; do
    jq -e --arg g "$g" '[.brief.gates[] | contains($g)] | any' "$WORK/interview/brief.json" >/dev/null \
      || die "the brief does not name the gate \"$g\""
  done
  say "interview: $QN questions with defaults; brief names hello-mint and the D-4 gates"
  answer() { jq -r --arg k "$1" '.brief.constraints[] | select(.id==$k) | .answer' "$WORK/interview/brief.json"; }
else
  say "interview: not exercised (--no-interview); using the answers directly"
  answer() { echo "$ANSWERS" | jq -r --arg k "$1" '.[$k]'; }
fi

# ------------------------------------------------------------------------ 2. render
NAME="$(answer name)"; SUPPLY="$(answer supply)"; PRICE_SALT="$(answer price)"
# SALT (up to 18 decimals) to wei, as a decimal string (no floating point).
salt_to_wei() {
  local v="$1" int frac
  [[ "$v" =~ ^[0-9]+(\.[0-9]{1,18})?$ ]] || die "price $v is not a SALT amount"
  int="${v%%.*}"; frac=""; [[ "$v" == *.* ]] && frac="${v#*.}"
  while [ ${#frac} -lt 18 ]; do frac="${frac}0"; done
  echo "$int$frac" | sed 's/^0*//; s/^$/0/'
}
PRICE_WEI="$(salt_to_wei "$PRICE_SALT")"
# Symbol: the name's letters and digits, upper-cased, at most 11, starting with a letter.
SYMBOL="$(echo "$NAME" | tr -cd 'A-Za-z0-9' | tr 'a-z' 'A-Z' | cut -c1-11)"
TARGET="${CARGO_TARGET_DIR:-$CORE/target}"
(cd "$CORE" && cargo build -q -p citrate-templates)
"$TARGET/debug/citrate-templates" render --root "$CORE/templates" --template hello-mint --tier "$TIER" \
  --out "$WORK/good/project" --param "name=$NAME" --param "symbol=$SYMBOL" --param "supply=$SUPPLY" \
  --param "price=$PRICE_WEI" --param "owner=$OWNER" > "$WORK/good/render.json"
jq '.params' "$WORK/good/render.json" > "$WORK/good/params.json"
echo "$OWNER" > "$WORK/good/owner.txt"
CONTRACT=$(jq -r '.params.contract' "$WORK/good/render.json")
say "render: hello-mint $TIER as $CONTRACT ($NAME / $SYMBOL, supply $SUPPLY, price $PRICE_WEI wei)"

for dep in $(jq -r '.deps | keys[]' "$CORE/templates/deps.lock.json"); do
  url=$(jq -r ".deps[\"$dep\"].url" "$CORE/templates/deps.lock.json")
  tag=$(jq -r ".deps[\"$dep\"].tag" "$CORE/templates/deps.lock.json")
  commit=$(jq -r ".deps[\"$dep\"].commit" "$CORE/templates/deps.lock.json")
  [ -d "$CACHE/$dep/.git" ] || git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" "$url" "$CACHE/$dep"
  [ "$(git -C "$CACHE/$dep" rev-parse HEAD)" = "$commit" ] || die "$dep is not at $commit"
  mkdir -p "$WORK/good/project/contracts/lib"
  cp -R "$CACHE/$dep" "$WORK/good/project/contracts/lib/$dep"
done

# The injected bug (US-6.1 AC2): the same project with the supply-cap check removed.
cp -R "$WORK/good/project" "$WORK/bug/project"
BUG_SRC="$WORK/bug/project/contracts/src/Token.sol"
perl -0pi -e 's/\n[ \t]*uint256 remaining = MAX_SUPPLY - minted;//; s/\n[ \t]*if \(quantity > remaining\) revert SoldOut\(quantity, remaining\);//' "$BUG_SRC"
! grep -q 'revert SoldOut' "$BUG_SRC" || die "the injected bug did not apply"
cp "$WORK/good/params.json" "$WORK/bug/params.json"

# ------------------------------------------------------------------------ 3. verifiers
# toolrun NAME OUTFILE BIN ARGS... : run BIN in the contracts dir, write a GateInputs ToolRun.
CONTRACTS=""
toolrun() {
  local name="$1" out="$2" bin="$3"; shift 3
  if [ -z "$bin" ]; then jq -n '{state:"notInstalled"}' > "$out"; return 0; fi
  local raw="$out.raw" t0 t1 ver rc
  ver=$("$bin" --version 2>/dev/null | head -1 || true)
  t0=$(now_ms)
  set +e
  (cd "$CONTRACTS" && FOUNDRY_OFFLINE=true "$bin" "$@") > "$raw" 2> "$out.stderr"
  rc=$?
  set -e
  t1=$(now_ms)
  # A tool that writes its report to a file (aderyn --output) names it in REPORT_FILE.
  if [ -n "${REPORT_FILE:-}" ]; then
    if [ -s "$REPORT_FILE" ]; then cp "$REPORT_FILE" "$raw"; else : > "$raw"; fi
  fi
  if [ ! -s "$raw" ]; then
    jq -n --arg m "$name exited $rc without a report: $(tail -c 240 "$out.stderr" | tr '\n' ' ')" '{state:"error", message:$m}' > "$out"
  else
    jq -n --rawfile o "$raw" --argjson d "$((t1 - t0))" --arg v "$ver" '{state:"ran", output:$o, durationMs:$d, toolVersion:$v}' > "$out"
  fi
}

ANVIL_PORT="${E2E_ANVIL_PORT:-18547}"
RPC="http://127.0.0.1:$ANVIL_PORT"
if [ "$FORK_URL" = "none" ]; then
  anvil --chain-id 40204 --port "$ANVIL_PORT" --silent &
else
  anvil --fork-url "$FORK_URL" --port "$ANVIL_PORT" --silent &
fi
PIDS+=($!)
for _ in $(seq 1 120); do cast chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.25; done
[ "$(cast chain-id --rpc-url "$RPC")" = "40204" ] || die "the anvil fork does not report chain id 40204"
FORK_BLOCK=$(cast block-number --rpc-url "$RPC")
say "fork: anvil on $RPC, chain id 40204, ${FORK_URL/none/no upstream} at block $FORK_BLOCK"

gate_inputs() {
  local which="$1"
  CONTRACTS="$WORK/$which/project/contracts"
  local d="$WORK/$which" art budget
  (cd "$CONTRACTS" && FOUNDRY_OFFLINE=true forge build --offline >/dev/null)
  art="$CONTRACTS/out/Token.sol/$CONTRACT.json"
  jq -r '.bytecode.object' "$art" > "$d/bytecode.hex"
  jq -r '.deployedBytecode.object' "$art" > "$d/deployed-bytecode.hex"
  budget=$(jq -r '.fuzzing.testLimit' "$CONTRACTS/medusa.json")
  toolrun forge "$d/forge.json" "$(command -v forge)" test --json --offline
  toolrun slither "$d/slither.json" "$(command -v slither)" . --sarif - --exclude-dependencies --disable-color --compile-force-framework foundry
  # The gate parses Aderyn's JSON report (the runtime toolchain asks for SARIF, which the gate
  # does not read yet; see the WP notes).
  rm -f "$d/aderyn-report.json"
  REPORT_FILE="$d/aderyn-report.json" toolrun aderyn "$d/aderyn.json" "$ADERYN" . --output "$d/aderyn-report.json" --skip-update-check
  toolrun medusa "$d/medusa.json" "$MEDUSA" fuzz --no-color --test-limit "$budget" --timeout "$(jq -r '.fuzzing.timeout' "$CONTRACTS/medusa.json")"
  rm -rf "$CONTRACTS/medusa-corpus"

  # 4. the fork dry run of exactly this init code (the template constructor takes no arguments).
  local t0 t1 receipt hash precompiles
  t0=$(now_ms)
  set +e
  receipt=$(cast send --rpc-url "$RPC" --unlocked --from "$OWNER" --json --create "$(cat "$d/bytecode.hex")" 2> "$d/dryrun.stderr")
  set -e
  t1=$(now_ms)
  if [ -n "$receipt" ] && echo "$receipt" | jq -e . >/dev/null 2>&1; then
    echo "$receipt" > "$d/dryrun.receipt.json"
    hash=$(echo "$receipt" | jq -r '.transactionHash')
    cast tx "$hash" --rpc-url "$RPC" --json | jq -r '.input' > "$d/dryrun.input.hex"
    jq -n --rawfile o "$d/dryrun.receipt.json" --argjson dm "$((t1 - t0))" --arg v "$(anvil --version | head -1)" \
      '{state:"ran", output:$o, durationMs:$dm, toolVersion:$v}' > "$d/dryrun.json"
  else
    jq -n --arg m "dry run failed: $(tail -c 240 "$d/dryrun.stderr" | tr '\n' ' ')" '{state:"error", message:$m}' > "$d/dryrun.json"
    echo "0x" > "$d/dryrun.input.hex"
  fi
  # Declared precompile use: a literal Citrate precompile address in the sources means "unknown"
  # (the gate also scans the bytecode for call sites).
  if grep -Eiq 'address\(0x0*(1[0-3][0-9a-f]|20[0-9]|100[0-3])\)' "$CONTRACTS"/src/*.sol; then precompiles=unknown; else precompiles=none; fi

  jq -n --arg bc "$(cat "$d/bytecode.hex")" --arg ti "$(cat "$d/dryrun.input.hex")" --arg pc "$precompiles" \
    --argjson budget "$budget" \
    --slurpfile f "$d/forge.json" --slurpfile s "$d/slither.json" --slurpfile a "$d/aderyn.json" \
    --slurpfile m "$d/medusa.json" --slurpfile r "$d/dryrun.json" \
    '{bytecodeHex:$bc, constructorArgsHex:null,
      compiler:{solcVersion:"0.8.36", optimizer:true, optimizerRuns:200, evmVersion:"cancun", viaIr:false},
      forgeTests:$f[0], slither:$s[0], aderyn:$a[0], medusa:{run:$m[0], callBudget:$budget},
      forkDryRun:{run:$r[0], txInputHex:$ti, citratePrecompiles:$pc}}' > "$d/gate-inputs.json"
  say "verifiers($which): forge $(jq -r .state "$d/forge.json"), slither $(jq -r .state "$d/slither.json"), aderyn $(jq -r .state "$d/aderyn.json"), medusa $(jq -r .state "$d/medusa.json"), dry run $(jq -r .state "$d/dryrun.json")"
}
gate_inputs good
gate_inputs bug

# Compiler settings come from the rendered foundry.toml; refuse a drift instead of guessing.
grep -q '^solc = "0.8.36"' "$WORK/good/project/contracts/foundry.toml" && grep -q '^evm_version = "cancun"' "$WORK/good/project/contracts/foundry.toml" \
  && grep -q '^optimizer_runs = 200' "$WORK/good/project/contracts/foundry.toml" || die "foundry.toml compiler settings changed; update the gate inputs"

# ------------------------------------------------------------------------ 5 + 6. gate, ceremony, verify, mint, bug
(cd "$CORE/src-tauri" && CITRATE_E2E_HM_DIR="$WORK" CITRATE_E2E_HM_EXPECT="$EXPECT" CITRATE_E2E_RPC="$RPC" \
  CITRATE_E2E_HM_PROJECT="$WORK/good/project" \
  cargo test --lib e2e_hello_mint_on_an_anvil_fork -- --nocapture --test-threads=1)

EV="$WORK/evidence-$EXPECT.json"
[ -f "$EV" ] || die "the Rust e2e step wrote no evidence"
jq -n --slurpfile e "$EV" --arg p "$PLATFORM" --arg v "$VERIFIERS" --arg x "$EXPECT" --arg f "$FORK_URL" \
  --arg b "$FORK_BLOCK" --arg t "$TIER" --arg core "$(git -C "$CORE" rev-parse HEAD)" \
  --arg forge "$(forge --version | head -1)" --arg slither "$(slither --version 2>/dev/null | head -1)" \
  --arg aderyn "${ADERYN:+$("$ADERYN" --version 2>/dev/null | head -1)}" \
  --arg medusa "${MEDUSA:+$("$MEDUSA" --version 2>/dev/null | head -1)}" \
  --arg when "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  '{when:$when, coreCommit:$core, platform:$p, verifiers:$v, expect:$x, tier:$t, forkUrl:$f, forkBlock:$b,
    tools:{forge:$forge, slither:$slither, aderyn:(if $aderyn=="" then "not installed" else $aderyn end),
           medusa:(if $medusa=="" then "not installed" else $medusa end)}, result:$e[0]}' > "$WORK/summary.json"
if [ -f "$WORK/tools-measured.jsonl" ]; then
  jq -s '.' "$WORK/tools-measured.jsonl" > "$WORK/tools-measured.json"
  jq --slurpfile m "$WORK/tools-measured.json" '.toolsFetched=$m[0]' "$WORK/summary.json" > "$WORK/summary.tmp" && mv "$WORK/summary.tmp" "$WORK/summary.json"
fi
if [ -n "$EVIDENCE" ]; then
  mkdir -p "$EVIDENCE"
  cp "$WORK/summary.json" "$EVIDENCE/summary-$PLATFORM-$VERIFIERS.json"
  [ -f "$WORK/interview/brief.json" ] && cp "$WORK/interview/brief.json" "$EVIDENCE/brief.json"
  for w in good bug; do
    for f in forge slither aderyn medusa dryrun; do
      [ -f "$WORK/$w/$f.json.raw" ] && cp "$WORK/$w/$f.json.raw" "$EVIDENCE/$PLATFORM-$VERIFIERS-$w-$f.out"
    done
    [ -f "$WORK/$w/dryrun.receipt.json" ] && cp "$WORK/$w/dryrun.receipt.json" "$EVIDENCE/$PLATFORM-$VERIFIERS-$w-dryrun.receipt.json"
  done
fi
say "OK: $VERIFIERS verifiers on $PLATFORM, verdict $(jq -r '.result.gateGood.verdict' "$WORK/summary.json"), bug build $(jq -r '.result.gateBug.verdict' "$WORK/summary.json")"
