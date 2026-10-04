---
created: 2026-10-01
branch: hup/n4-node-mcp
author: Larry Klosowski + Claude Opus 5.5
status: implemented (HUP-S4.2 + S8.5); off by default; port and personal-memory scope pending owner sign-off
---

# The citrate-node MCP server

Citrate Core can act as an [MCP](https://modelcontextprotocol.io) server, so Claude Code,
Cursor, another agent, or Hermes itself can use this member's node: read its status and the
chain, search the shared knowledge graphs, look at clusters and invites, and *ask* for
changes. Every change waits for the member's approval in the app. Transactions are signed only
through the SignatureCeremony, the app's one signing path. No MCP client ever holds a key or
gets a signature without the member pressing Approve.

Planset: `.agentile/planset/2026-09-30-hermes-upskill/` (US-4.2, US-8.3; WPs HUP-S4.2, HUP-S8.5).
Code: `src-tauri/src/node_mcp*.rs`, `src/nodeMcp/`.

## Turn it on

Settings, **API endpoints & keys**, **Node MCP server**:

1. **Turn on.** The server listens on `http://127.0.0.1:47204/mcp`, on this computer only. The
   switch is remembered across launches. If the port is taken, the panel shows the error.
2. **Create token.** Give it a label (the client it is for). The token (`cnmcp_` + 64 hex) is
   shown once, with ready-made commands. Only its SHA-256 is stored. Revoke it from the same list
   at any time: the next request with it is refused, its sessions end, and its pending requests
   are closed.

## Connect Claude Code

Over HTTP (Claude Code sends the header on every request):

```sh
claude mcp add --transport http citrate-node http://127.0.0.1:47204/mcp \
  --header "Authorization: Bearer cnmcp_..."
```

Or through the stdio shim, which is the app binary run with `--mcp-stdio` (it starts no window
and no sidecars; it forwards stdin to the running app's loopback endpoint):

```sh
claude mcp add citrate-node -e CITRATE_NODE_MCP_TOKEN=cnmcp_... \
  -- "/Applications/Citrate Core.app/Contents/MacOS/citrate-core" --mcp-stdio
```

The shim only ever sends the token to a loopback address. `CITRATE_NODE_MCP_PORT` (or
`CITRATE_NODE_MCP_URL`, loopback only) points it at a non-default port. Hermes's MCP host
(`citrate-agent-runtime/agent-mcp-host`) can use the shim the same way, as a `stdio` server with
`env = { CITRATE_NODE_MCP_TOKEN = "..." }`.

Check it from Claude Code with `/mcp`, or ask it to "use citrate-node to show the chain head".

## What it offers

**Resources:** `citrate://node/status`, `citrate://chain/head`, `citrate://wallet` (public
address and balance), `citrate://addresses` (the address book this build ships with),
`citrate://precompiles`, and the template `citrate://memory/{tenant}/search?q={query}`.

**Read tools** (`readOnlyHint: true`): `node_status`, `chain_head`, `get_balance`, `chain_call`
(eth_call), `estimate_gas`, `get_logs` (one contract, at most 5,000 blocks), `precompile_table`,
`precompile_call` (read-only eth_call to a precompile in the table: 0x0107-0x0109, 0x0110,
0x0111, 0x0120, 0x0130, 0x0200-0x0202), `wallet_info`, `address_book`, `memory_search`,
`groups_list`, `cluster_status`, `cluster_peers`, `cluster_devices` (each member's linked
machines and whether they are connected; added by HUP-S8.3 on `hup/n5-fleet-rest`),
`invites_list` (ids only, never links or tokens), `request_status`.

Chain reads come from this node when it is running and caught up, otherwise from the public
40204 RPC; every answer says which (`source`). Only read-only JSON-RPC methods are ever called.

**Write tools** (`destructiveHint: true`), each of which only creates a request:

| Tool | On approval |
|---|---|
| `tx_propose` | Opens a SignatureCeremony with the origin `mcp:<token label> via <client name>`. Approving signs and broadcasts through `sign_and_broadcast`. An undecodable transaction needs the raw-data acknowledgement. |
| `cluster_join` | Joins this node to the group's cluster mesh. |
| `cluster_share` | Announces a pinned CID to the group's cluster. |
| `invite_create` | Creates a one-time invite; the approved result carries the link. |
| `invite_revoke` | Revokes an outstanding invite (on the relay and locally). |

A write returns `{requestId, state: "pending"}`. The client polls `request_status` and sees
`pending`, `running`, `approved` (with the result: a tx hash, an invite link), `rejected`,
`failed`, or `expired`. A client can see only its own requests.

## Rules the server enforces

- Loopback bind only; the `Host` header must name the loopback endpoint (DNS-rebinding guard);
  a browser `Origin` from anywhere else is refused; no CORS headers are sent.
- `Authorization: Bearer <connect token>` on every request; `initialize` opens a session bound to
  that token; a session cannot be used with another token.
- JSON-RPC batches are refused (MCP 2025-06-18); protocol versions 2025-06-18, 2025-03-26 and
  2024-11-05 are accepted.
- Bounds: 16 KiB headers, 1 MiB body, a 15 s deadline for the whole request, 32 connections,
  64 sessions, 16 pending requests in total, 15 minutes before a pending request expires (its
  ceremony is closed).
- Text shown on an approval card (token labels, the client name, an invite's `for_handle`) may
  not contain control characters or Unicode text-direction characters.
- Arguments are validated against each tool's schema before anything touches the node; unknown
  arguments are refused.
- The member's `personal` memory tenant is not searchable over MCP.

## Not in this build

- `deploy_propose`, `pin_add`, `faucet_request`, `anchor_propose` from the planset's tool list.
  Deploys go through the in-app deploy gate (HUP-S6.4); the faucet is an ADR in review; the
  anchor registry is not deployed on 40204 yet.
- MCP Tasks for long operations, `dag_stats`, and a device list for cluster tools (no device
  registry exists in core yet).
- Server-initiated messages (SSE streams, elicitation). Approvals happen in the app, and the
  client polls.

## Pending owner sign-off

- The default port `47204`.
- Whether a token may be scoped to the member's personal memory (today: never).
- The bounds above (16 pending requests, 15-minute expiry, 16 live tokens, 5,000-block
  `get_logs` range, 32 connections, 64 sessions) are conservative defaults.

## Demo transcript

Recorded 2026-10-01 with the real server in a test harness whose chain reads go to the live
public 40204 RPC (`cargo test --lib node_mcp_demo -- --ignored --nocapture`, served on port
47299). Tokens are elided. In the app, a public-RPC answer's `source` reads
`public-rpc (this node is not running or still syncing)`; the test harness reports the shorter
`public-rpc`.

Claude Code as the client (HTTP transport, bearer header):

```text
$ claude -p --strict-mcp-config --mcp-config citrate-node.json \
    --allowedTools mcp__citrate-node__chain_head \
    "Call the citrate-node chain_head tool once and reply with only the chainId, height and source it returned."
chainId: 40204, height: 78024, source: public-rpc
```

curl, `tools/list` (names) and one read tool:

```text
POST /mcp (no token)                       -> 401
POST /mcp initialize                        -> 200, Mcp-Session-Id: ad8b...
POST /mcp tools/list                        -> node_status, chain_head, get_balance, chain_call,
   estimate_gas, get_logs, precompile_table, precompile_call, wallet_info, address_book,
   memory_search, groups_list, cluster_status, cluster_peers, invites_list, request_status,
   tx_propose, cluster_join, cluster_share, invite_create, invite_revoke
POST /mcp tools/call get_balance {"address":"0xBa4a...3ad5"}
   -> {"address":"0xba4a...3ad5","balanceWei":"888130174596060937125253","source":"public-rpc"}
```

The stdio shim (`citrate-core --mcp-stdio`, the debug app binary):

```text
{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"citrate://chain/head"}}
-> {"id":3,"jsonrpc":"2.0","result":{"contents":[{"mimeType":"application/json",
    "text":"{\"chainId\": 40204, \"height\": 78052, \"source\": \"public-rpc\"}","uri":"citrate://chain/head"}]}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"precompile_call",
  "arguments":{"address":"0x0000000000000000000000000000000000000120","data":"0x"}}}
-> {"precompile":"ED25519_VERIFY","result":"0x","source":"public-rpc"}
```

A shim pointed at a non-loopback URL refuses to start (exit 2) without sending the token.
