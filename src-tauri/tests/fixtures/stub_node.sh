#!/usr/bin/env bash
# citrate-core — CI stub node (CORE-C1.1 test helper).
#
# A tiny stand-in for the real `citrate` binary so the NodeDomain↔supervisor
# wiring, the storage-key-in-keyring handoff, and the ciphertext-at-rest grep
# can be exercised in CI WITHOUT building the heavy ark/zk node. It:
#   1. parses `--data-dir <DIR>` (mirrors the real node's flag),
#   2. reads the 32-byte storage key from $CITRATE_STORAGE_KEY (hex), exactly the
#      way the real node's `at_rest_encryption_from_env` does,
#   3. writes an "encrypted-at-rest" data file whose bytes are the key XOR'd over
#      a known plaintext sentinel — so a raw-disk grep for the sentinel finds
#      CIPHERTEXT, never the plaintext (the ENCRYPT tripwire), while a grep with
#      the key present could recover it (proving the key is load-bearing),
#   4. writes an encryption.meta marker (like the real node), and
#   5. blocks until signalled (SIGTERM/SIGKILL from the supervisor), so the
#      supervisor sees a long-lived Running child and `stop` proves a clean kill.
#
# Deliberately NOT a JSON-RPC server: status-parsing is tested against a mock
# RpcTransport in Rust (no port races in CI). The live node provides the real
# RPC; this stub proves spawn/kill/keyring/ciphertext only.
set -euo pipefail

DATA_DIR=""
while [ $# -gt 0 ]; do
  case "$1" in
    --data-dir) DATA_DIR="$2"; shift 2 ;;
    --network)  shift 2 ;;
    *)          shift 1 ;;
  esac
done

if [ -z "${DATA_DIR}" ]; then
  echo "stub_node: --data-dir required" >&2
  exit 2
fi
if [ -z "${CITRATE_STORAGE_KEY:-}" ]; then
  echo "stub_node: CITRATE_STORAGE_KEY env required (fail closed, like the real node)" >&2
  exit 3
fi

mkdir -p "${DATA_DIR}"

# The known plaintext the tripwire test greps for. If this string appears in the
# raw data file, encryption failed.
SENTINEL="CITRATE_PLAINTEXT_SENTINEL_v1"

# XOR the sentinel with the (repeating) key bytes → non-plaintext ciphertext.
# Pure bash/od so there is no dependency on openssl/python being present.
python3 - "$DATA_DIR/data.rocks" "$CITRATE_STORAGE_KEY" "$SENTINEL" <<'PY'
import sys
out_path, key_hex, sentinel = sys.argv[1], sys.argv[2], sys.argv[3]
key = bytes.fromhex(key_hex)
pt = sentinel.encode()
ct = bytes(pt[i] ^ key[i % len(key)] for i in range(len(pt)))
with open(out_path, "wb") as f:
    f.write(ct)
PY

# encryption.meta marker (like core/storage's encryption.meta).
printf '{"cipher":"stub-xor","value_format":1}' > "${DATA_DIR}/encryption.meta"

# Block until killed. `wait` on a background sleep lets SIGTERM interrupt fast.
sleep 100000 &
SLEEP_PID=$!
trap 'kill "${SLEEP_PID}" 2>/dev/null || true; exit 0' TERM INT
wait "${SLEEP_PID}"
