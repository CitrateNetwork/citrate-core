#!/usr/bin/env bash
# citrate-core — CI stub node-agent (CORE-C1.2 test helper).
#
# A tiny stand-in for the real `node-agent` daemon so the supervisor spawn/kill,
# the @rule8 bearer handoff (OUR minted token via the CITRATE_NODE_AGENT_TOKEN_FILE
# file channel), and the loopback supervision round-trip can be exercised in CI
# WITHOUT building the heavy real node-agent. It faithfully mirrors the grounded
# supervision contract (citrate-node-agent crates/supervision/src/server.rs):
#   * binds CITRATE_NODE_AGENT_ADDR (loopback) and serves a minimal HTTP surface,
#   * reads the bearer from the file at CITRATE_NODE_AGENT_TOKEN_FILE (exactly the
#     way the real node-agent's SupervisionAuth::load_or_create adopts an existing
#     token — the file IS the IPC channel),
#   * `/health` is OPEN; `/status` requires `Authorization: Bearer <token>` and
#     returns "idle"; a missing/wrong bearer → 401,
#   * blocks until signalled (SIGTERM/SIGKILL) so the supervisor sees a long-lived
#     Running child and `stop` proves a clean kill.
#
# Uses python3's http.server (present on CI + macOS) to avoid a netcat-portability
# mess. Deliberately minimal: only the endpoints the C1.2 handshake test drives.
set -euo pipefail

ADDR="${CITRATE_NODE_AGENT_ADDR:-127.0.0.1:19600}"
HOST="${ADDR%%:*}"
PORT="${ADDR##*:}"
TOKEN_FILE="${CITRATE_NODE_AGENT_TOKEN_FILE:-}"

if [ -z "${TOKEN_FILE}" ]; then
  echo "stub_node_agent: CITRATE_NODE_AGENT_TOKEN_FILE required (the bearer IPC channel)" >&2
  exit 3
fi
# Wait briefly for the parent to have written the token file (it writes it before
# spawn, but be robust to scheduling).
for _ in $(seq 1 50); do
  [ -s "${TOKEN_FILE}" ] && break
  sleep 0.05
done
if [ ! -s "${TOKEN_FILE}" ]; then
  echo "stub_node_agent: token file never appeared: ${TOKEN_FILE}" >&2
  exit 4
fi

exec python3 - "$HOST" "$PORT" "$TOKEN_FILE" <<'PY'
import sys, json
from http.server import BaseHTTPRequestHandler, HTTPServer

host, port, token_file = sys.argv[1], int(sys.argv[2]), sys.argv[3]
with open(token_file, "r") as f:
    EXPECTED = f.read().strip()

class H(BaseHTTPRequestHandler):
    def _send(self, code, body):
        payload = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _authed(self):
        h = self.headers.get("Authorization", "")
        return h.startswith("Bearer ") and h[len("Bearer "):].strip() == EXPECTED

    def do_GET(self):
        if self.path == "/health":
            return self._send(200, {"state": "idle", "active_jobs": 0})
        if not self._authed():
            return self._send(401, "missing or invalid supervision token")
        if self.path == "/status":
            return self._send(200, "idle")
        if self.path == "/signature-requests":
            return self._send(200, [])
        return self._send(404, "not found")

    def do_POST(self):
        if not self._authed():
            return self._send(401, "missing or invalid supervision token")
        return self._send(200, "ok")

    def log_message(self, *args):
        pass  # quiet

HTTPServer((host, port), H).serve_forever()
PY
