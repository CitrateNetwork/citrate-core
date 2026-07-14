#!/usr/bin/env bash
# citrate-core — CI stub mem-mcp daemon (CORE-C3 test helper).
#
# A tiny stand-in for the real `mcp_serve` daemon so the MemoryDomain↔supervisor
# wiring, the store-key-in-keyring handoff, the ciphertext-at-rest grep, and the
# stale-socket-safe restart can be exercised in CI WITHOUT building the heavy
# rocksdb+transformer daemon or downloading a 440 MB bge model.
#
# Grounded CLI (mem-mcp examples/mcp_serve.rs): positional `<store-path>
# <sock-path>`. This stub:
#   1. takes arg1 = store dir, arg2 = socket path (matching the real daemon),
#   2. reads the 32-byte store key from $CITRATE_MEM_STORE_KEY (hex) — the
#      forward-compat env seam citrate-core passes; fail closed if absent,
#   3. writes an "encrypted-at-rest" store file (store-dir/data.enc) whose bytes
#      are a known plaintext sentinel XOR'd by the key — so a raw-disk grep for
#      the sentinel finds CIPHERTEXT, never plaintext (the ENCRYPT tripwire),
#   4. STALE-SOCKET-SAFE like the real daemon: if the socket path already exists,
#      remove it before binding (the real daemon does this because it holds the
#      RocksDB lock, which proves the socket stale),
#   5. binds the Unix socket and answers newline-delimited JSON-RPC tools/call
#      with a fixture recall payload, so a socket round-trip works, and
#   6. blocks until signalled (SIGTERM/SIGKILL from the supervisor).
set -euo pipefail

STORE_DIR="${1:-}"
SOCK_PATH="${2:-}"

if [ -z "${STORE_DIR}" ] || [ -z "${SOCK_PATH}" ]; then
  echo "stub_mem_mcp: usage: stub_mem_mcp <store-path> <sock-path>" >&2
  exit 2
fi
if [ -z "${CITRATE_MEM_STORE_KEY:-}" ]; then
  echo "stub_mem_mcp: CITRATE_MEM_STORE_KEY env required (fail closed)" >&2
  exit 3
fi

mkdir -p "${STORE_DIR}"

python3 - "$STORE_DIR" "$SOCK_PATH" "$CITRATE_MEM_STORE_KEY" <<'PY'
import os, sys, socket, json, threading

store_dir, sock_path, key_hex = sys.argv[1], sys.argv[2], sys.argv[3]
key = bytes.fromhex(key_hex)

# 3. Write the "encrypted" store file: sentinel XOR key → ciphertext on disk.
SENTINEL = b"CITRATE_MEM_PLAINTEXT_SENTINEL_v1"
ct = bytes(SENTINEL[i] ^ key[i % len(key)] for i in range(len(SENTINEL)))
with open(os.path.join(store_dir, "data.enc"), "wb") as f:
    f.write(ct)
# An encryption marker like the real store's keyring CF presence.
with open(os.path.join(store_dir, "encryption.meta"), "w") as f:
    f.write('{"cipher":"stub-xor","value_format":1}')

# 4. Stale-socket-safe: remove a leftover socket file before binding.
try:
    if os.path.exists(sock_path):
        os.unlink(sock_path)
except OSError:
    pass

# A grounded-shape render_result payload (mem-mcp render_result).
RECALL_TEXT = (
    "freshness: HEAD abc123def456 (7 commits) ingested @ 1720000000000ms\n"
    "tenant 'personal' — 6 nodes, showing 2:\n"
    "  0a1b2c3d4e [Rationale] prefers reduced telemetry\n"
    "  9988776655 [Doc] node data dir fact\n"
)

def serve(conn):
    buf = b""
    f = conn.makefile("rwb")
    for raw in f:
        line = raw.decode("utf-8", "replace").strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except Exception:
            continue
        rid = req.get("id")
        method = req.get("method", "")
        if method == "initialize":
            resp = {"jsonrpc": "2.0", "id": rid, "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {"name": "citrate-memories-stub", "version": "0"}}}
        elif method == "tools/call":
            resp = {"jsonrpc": "2.0", "id": rid, "result": {
                "content": [{"type": "text", "text": RECALL_TEXT}], "isError": False}}
        elif method == "tools/list":
            resp = {"jsonrpc": "2.0", "id": rid, "result": {"tools": []}}
        else:
            resp = {"jsonrpc": "2.0", "id": rid, "result": {}}
        if rid is not None:
            f.write((json.dumps(resp) + "\n").encode())
            f.flush()

srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(sock_path)
srv.listen(16)

# 6. Accept loop until killed. Each connection gets its own thread.
while True:
    conn, _ = srv.accept()
    threading.Thread(target=serve, args=(conn,), daemon=True).start()
PY
