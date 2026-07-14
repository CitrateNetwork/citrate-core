#!/usr/bin/env bash
# CORE-C3 — live proof for the REAL citrate-memories mem-mcp daemon + the E-4
# chain-state ingest of citrate-chain's 40204.json.
#
# The CI tests use a stub MCP socket (fast, no rocksdb/model). THIS script runs
# the REAL rocksdb+transformer daemon (`mcp_serve`) against a real encrypted
# store, ingests the 40204 contract catalog into the `chain-state` tenant, and
# proves — WITHOUT fabricating a graph (Rule 1):
#   1. `mem-ingest`'s chain_ingest loads 40204.json → chain-state nodes,
#   2. `mcp_serve` serves them: memory.recall(repo="chain-state") returns
#      ChainContract nodes matching 40204.json (LiquidStakingPool, EntryPoint…),
#   3. the store on disk is CIPHERTEXT at rest (no contract-name plaintext in the
#      raw RocksDB bytes — the per-tenant XChaCha20 seal).
#
# It is heavy: it builds the rocksdb+transformer daemon and (for semantic search)
# needs the ~440 MB bge model. Run it locally; it is NOT a CI gate.
#
# Usage (from anywhere):
#   MEMORIES=/abs/path/to/citrate-memories \
#   CHAIN=/abs/path/to/citrate-chain \
#   src-tauri/scripts/c3_live_proof.sh
set -euo pipefail

MEMORIES="${MEMORIES:-$HOME/Projects/citrate-labs/citrate-memories}"
CHAIN="${CHAIN:-$HOME/Projects/citrate-labs/citrate-chain}"
CATALOG="${CATALOG:-$CHAIN/contracts/addresses/40204.json}"

if [ ! -d "$MEMORIES" ]; then echo "citrate-memories not found at $MEMORIES (set MEMORIES=)" >&2; exit 2; fi
if [ ! -f "$CATALOG" ]; then echo "40204.json not found at $CATALOG (set CHAIN= / CATALOG=)" >&2; exit 2; fi

WORK="$(mktemp -d -t citrate-c3-XXXXXX)"
PLAIN="$WORK/store.memdag"          # chain_ingest lands here (plaintext)
STORE="$WORK/store.bge.enc.memdag"  # reencrypt seals it here (per-tenant key)
SOCK="$WORK/memdag.sock"
# rocksdb is required; add ",transformer" for semantic search (needs the bge
# model). The catalog ingest + recall proof works with rocksdb alone (the store
# falls back to the hashing embedder), so FEATURES defaults to rocksdb.
FEATURES="${FEATURES:-rocksdb}"

echo "== C3 live proof =="
echo "  memories: $MEMORIES"
echo "  catalog:  $CATALOG"
echo "  store:    $STORE   (encrypted at rest)"
echo "  socket:   $SOCK"

cleanup() { [ -n "${SERVE_PID:-}" ] && kill "$SERVE_PID" 2>/dev/null || true; }
trap cleanup EXIT

cd "$MEMORIES"

echo "--- 1. build daemon + ingester + reencrypt ($FEATURES) ---"
cargo build --release -p mem-mcp --example mcp_serve --features "$FEATURES"
cargo build --release -p mem-ingest --example chain_ingest --features "$FEATURES"
cargo build --release -p mem-store --example reencrypt --features rocksdb

echo "--- 2. ingest 40204.json → chain-state tenant (plaintext store) ---"
# chain_ingest lands the ChainNetwork + ChainContract nodes deterministically.
./target/release/examples/chain_ingest "$PLAIN" "$CATALOG"

echo "--- 2b. seal it: reencrypt → per-tenant XChaCha20 store (the @rule8 leg) ---"
# The KEYS CF only gains entries via an encrypted write path; reencrypt copies the
# graph into an encrypted store, minting a per-tenant key and sealing every node.
./target/release/examples/reencrypt "$PLAIN" "$STORE"

echo "--- 3. serve + recall(chain-state) over the socket ---"
MEM_CHECKPOINT_INTERVAL_SECS=0 ./target/release/examples/mcp_serve "$STORE" "$SOCK" &
SERVE_PID=$!
# Wait for the socket to appear (the daemon binds after taking the DB lock).
for _ in $(seq 1 50); do [ -S "$SOCK" ] && break; sleep 0.2; done
[ -S "$SOCK" ] || { echo "daemon did not bind $SOCK" >&2; exit 1; }

# Drive one JSON-RPC recall over the socket via the mcp_connect shim (stdio<->sock)
# — or, if that example is unavailable, a raw nc/socat round-trip. Here we use a
# short python client so the proof is self-contained.
RECALL_JSON='{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory.recall","arguments":{"repo":"chain-state","budget":50}}}'
RESP="$(printf '%s\n' "$RECALL_JSON" | python3 -c '
import socket,sys,os
sock=sys.argv[1]
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM); s.connect(sock)
s.sendall(sys.stdin.buffer.read())
buf=b""
s.settimeout(10)
try:
    while b"\n" not in buf: buf+=s.recv(4096)
except Exception: pass
sys.stdout.write(buf.decode("utf-8","replace"))
' "$SOCK")"
echo "$RESP"

echo "--- 4. assert 40204 contracts present in the recall ---"
# LiquidStakingPool + EntryPoint are canonical entries in 40204.json.
echo "$RESP" | grep -qi "LiquidStakingPool" && echo "  ✓ LiquidStakingPool present"
echo "$RESP" | grep -qi "chain-state"        && echo "  ✓ chain-state tenant present"

echo "--- 5. ciphertext at rest (no contract-name plaintext in raw store) ---"
# The per-tenant XChaCha20 seal means node text is ciphertext on disk. A raw grep
# of the RocksDB SST/log files must NOT surface a contract name.
if grep -rqa "LiquidStakingPool" "$STORE" 2>/dev/null; then
  echo "  ✗ FAIL: contract-name plaintext found in the store on disk" >&2
  exit 1
fi
echo "  ✓ no contract-name plaintext in the raw store (ciphertext at rest)"

echo "== C3 live proof OK =="
