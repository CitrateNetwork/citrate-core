---
created: 2026-10-01
branch: hup/n4-node-mcp (updated on hup/n5-nodemcp-rest, 2026-10-01)
author: Larry Klosowski + Claude Opus 5.5
status: implemented (HUP-S4.2 + S8.5); off by default; port, personal-memory scope, Hermes write tools and task TTL pending owner sign-off
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

**Read tools** (`readOnlyHint: true`): `node_status`, `chain_head`, `dag_stats`, `get_balance`,
`chain_call` (eth_call), `estimate_gas`, `get_logs` (one contract, at most 5,000 blocks),
`precompile_table`,
`precompile_call` (read-only eth_call to a precompile in the table: 0x0107-0x0109, 0x0110,
0x0111, 0x0120, 0x0130, 0x0200-0x0202), `wallet_info`, `address_book`, `memory_search`,
`groups_list`, `cluster_status`, `cluster_peers`, `invites_list` (ids only, never links or
tokens), `devices_list` (linked devices and revoked device addresses; no signatures), `pins_list`
(the files this node keeps), `request_status`.

`dag_stats` passes on what the node reports exactly: the current tips, height, highest blue score
and GhostDAG parameters. The node's `citrate_getDagStats` also returns blue and red block counts,
but those are a fixed 95% split of the height, not a count, so the tool leaves them out and says so.

Chain reads come from this node when it is running and caught up, otherwise from the public
40204 RPC; every answer says which (`source`). Only read-only JSON-RPC methods are ever called.

**Write tools** (`destructiveHint: true`), each of which only creates a request:

| Tool | On approval |
|---|---|
| `tx_propose` | Opens a SignatureCeremony with the origin `mcp:<token label> via <client name>`. Approving signs and broadcasts through `sign_and_broadcast`. An undecodable transaction needs the raw-data acknowledgement. |
| `deploy_propose` | Refused at once, naming the failing items, unless the deploy gate (HUP-S6.4) is READY for exactly `bytecode ++ constructor_args`. Otherwise it opens the same contract-creation ceremony as the app's own deploy (`contract_deploy::propose_deploy`), tracked by the gate so a later NOT READY result closes it. Approving signs and broadcasts through `sign_and_broadcast`. |
| `pin_add` | Pins the CID on this node's IPFS daemon (a local pin: no storage bond, no transaction). |
| `anchor_propose` | Refused at once while nightly anchoring is off or AnchorRegistry is not in this build's address book. On approval it runs the nightly anchor pass now: each closed day becomes its own anchor approval card, and nothing is signed until the member approves that card. |
| `cluster_join` | Joins this node to the group's cluster mesh. |
| `cluster_share` | Announces a pinned CID to the group's cluster. |
| `invite_create` | Creates a one-time invite; the approved result carries the link. |
| `invite_revoke` | Revokes an outstanding invite (on the relay and locally). |

A write returns `{requestId, state: "pending"}`. The client polls `request_status` and sees
`pending`, `running`, `approved` (with the result: a tx hash, an invite link), `rejected`,
`failed`, or `expired`. A client can see only its own requests.

## MCP Tasks

A client that declares the Tasks extension (`io.modelcontextprotocol/tasks`, SEP-2663) in a
request's `_meta` client capabilities gets a task handle (`resultType: "task"`, a `taskId`,
`status: "working"`, `ttlMs`, `pollIntervalMs: 5000`) from every write tool instead of the plain
pending reply. A client that did not declare it never gets a task. The task is the approval
request, so `tasks/get` follows the member's decision:

| Request state | Task |
|---|---|
| pending, running | `working`, with a status message |
| approved | `completed`, `result` = the tool result (a tx hash, an invite link, the pin) |
| rejected by the member, failed, expired | `completed`, `result` is an `isError` tool result saying why |
| withdrawn with `tasks/cancel` while pending | `cancelled` (its ceremony is closed) |

`tasks/cancel` on a request the member already approved is acknowledged and changes nothing
(cancellation is cooperative). `tasks/update` is acknowledged; this server never asks a client for
input, because the member decides in the app. A client can see and cancel only its own tasks.

## Protocol eras

The server speaks the session era (2025-06-18, 2025-03-26, 2024-11-05: `initialize` opens a
session) and the stateless 2026-07-28 revision side by side. A request whose `params._meta` names
`io.modelcontextprotocol/protocolVersion: 2026-07-28` needs no session. It must carry the
`MCP-Protocol-Version`, `Mcp-Method` and (for `tools/call` and `resources/read`) `Mcp-Name`
headers matching the body (400 `HeaderMismatch` otherwise; Base64 sentinel values are decoded).
`server/discover` lists all four versions, the capabilities (including the Tasks extension) and
the server identity. Stateless results carry `resultType`, the server identity in `_meta`, and
`ttlMs` / `cacheScope` on list and read results. An unknown method is HTTP 404 with `-32601`; an
unsupported version is 400 with `UnsupportedProtocolVersion` (`-32022`) listing the versions. The
stdio shim adds the stateless headers itself and passes the server's JSON-RPC errors through.

Not used: `subscriptions/listen` and `notifications/tasks` (clients poll), multi round-trip
input requests, and `x-mcp-header` parameters.

## Hermes as a client

Hermes can use this server too (Agent, Connected tools (MCP), **Your node**; off by default, and
only available while the Node MCP server is on). Core then issues a connect token labelled
`Hermes (built-in)` (revoking any earlier one), writes a `node` entry into the sidecar's MCP
allowlist that runs `citrate-core --mcp-stdio` with that token and port, and revokes the token
when the switch is off. The token sits in the allowlist file (0600), like other MCP credentials.
Hermes is offered the read tools only (`allow_write_tools = false`, pending owner sign-off).

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

- `faucet_request` from the planset's tool list: the faucet ADR is proposed and waits on owner
  decisions O-1 to O-4.
- The planset's other long-running tasks: "sync" and "FL round" as MCP tasks, and the
  "confirm" half of deploy-and-confirm (a deploy task completes with the transaction hash; the
  receipt is not awaited).
- Server-initiated messages (SSE streams, `subscriptions/listen`, elicitation). Approvals happen in
  the app, and the client polls.
- An end-to-end approval of a real `tx_propose` or `deploy_propose` in the packaged app (needs a
  member at the app; agents never sign).

## Pending owner sign-off

- The default port `47204`.
- Whether a token may be scoped to the member's personal memory (today: never).
- Whether Hermes is offered the node server's write tools (today: read tools only), and whether
  its switch should default on (today: off).
- The task TTL (30 minutes from creation) and polling interval (5 seconds).
- The `deploy_propose` gas cap (30,000,000; default 2,000,000).
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

## Demo transcript, part 2 (gate g2-mcp)

Recorded 2026-10-01 on branch `hup/n5-nodemcp-rest`, macOS arm64. The server is the real server
code (`node_mcp_demo`, run from the built test binary) on port 47299, with chain reads going to the
live public 40204 RPC; the app's node, wallet, daemons and deploy gate are not behind it, so those
tools answer "not available" honestly. The stdio shim is the debug `citrate-core` binary built from
this branch. Tokens are elided.

**Hermes's own MCP host as the client.** `citrate-agent-mcp-host` from runtime `main` (01a32ed),
driven by a scratch program (not shipped) with exactly the `node` allowlist entry core writes for
Hermes: `stdio`, `citrate-core --mcp-stdio`, `CITRATE_NODE_MCP_TOKEN` and `CITRATE_NODE_MCP_PORT`
in `env`, `allow_write_tools = false`.

```text
probe: ok=true protocolVersion=2025-06-18 serverName=citrate-node serverVersion=0.4.2
       capabilities.extensions = {"io.modelcontextprotocol/tasks": {}}
       27 tools listed: 19 offered (read-only), 8 not offered (the write tools), all trust=untrusted
connect: server=node state=Ready tools_offered=19 skipped=8
  skipped: tx_propose / deploy_propose / pin_add / anchor_propose / cluster_join / cluster_share /
           invite_create / invite_revoke: not annotated read-only, and this server does not allow
           write tools
offered to the model: mcp__node__node_status, mcp__node__chain_head, mcp__node__get_balance,
  mcp__node__chain_call, mcp__node__estimate_gas, mcp__node__get_logs, mcp__node__precompile_table,
  mcp__node__precompile_call, mcp__node__wallet_info, mcp__node__address_book,
  mcp__node__memory_search, mcp__node__groups_list, mcp__node__cluster_status,
  mcp__node__cluster_peers, mcp__node__invites_list, mcp__node__dag_stats,
  mcp__node__devices_list, mcp__node__pins_list, mcp__node__request_status
call mcp__node__chain_head {}      -> Untrusted({"chainId": 40204, "height": 93858, "source": "public-rpc"})
call mcp__node__dag_stats {}       -> Untrusted({"height": 93859, "maxBlueScore": 93859, "tipsCount": 1,
                                       "tips": ["0xde77...7a8f"], "ghostdagParams": {"k": 18, ...},
                                       "note": "Blue and red block counts are not reported: ...",
                                       "source": "public-rpc"})
call mcp__node__precompile_call {"address":"0x...0120","data":"0x"}
                                   -> Untrusted({"precompile": "ED25519_VERIFY", "result": "0x", "source": "public-rpc"})
call mcp__node__pin_add {...}      -> Error("'mcp__node__pin_add' is not an MCP tool offered in this session")
```

**Claude Code 2.1.281 as the client** (HTTP transport, bearer header):

```text
$ claude -p "...1) Call dag_stats... 2) Call pin_add once with cid bafybeigdyrzt... 3) Call request_status..." \
    --strict-mcp-config --mcp-config citrate-node.json \
    --allowedTools "mcp__citrate-node__dag_stats,mcp__citrate-node__pin_add,mcp__citrate-node__request_status"
1. dag_stats: height 93871, tipsCount 1, source public-rpc
2. pin_add: requestId mcpr-1, state pending (needs your approval in Citrate Core)
3. request_status mcpr-1: pending
```

**The 2026-07-28 stateless era and MCP Tasks** (curl, no session; the client declares the Tasks
extension in `_meta`):

```text
server/discover                -> 200 supportedVersions [2026-07-28, 2025-06-18, 2025-03-26, 2024-11-05],
                                  capabilities.extensions.io.modelcontextprotocol/tasks, cacheScope public
tools/call pin_add             -> 200 {"resultType":"task","taskId":"mcpr-2","status":"working",
                                  "statusMessage":"Waiting for the member to approve or reject this in Citrate Core.",
                                  "ttlMs":1800000,"pollIntervalMs":5000, ...}
tasks/get mcpr-2               -> 200 {"resultType":"complete","status":"working", ...}
tasks/cancel mcpr-2            -> 200 {"resultType":"complete"}
tasks/get mcpr-2               -> 200 {"resultType":"complete","status":"cancelled","statusMessage":"Withdrawn by the client."}
tools/call chain_head with Mcp-Name: node_status
                               -> 400 {"error":{"code":-32020,"message":"Header mismatch: Mcp-Name header value does not match the body"}}
stdio shim, one stateless line -> {"result":{"resultType":"complete","structuredContent":{"chainId":40204,"height":93879,"source":"public-rpc"}, ...}}
```

Still to record: the same flows against the packaged app with a member approving a request in
Settings (the approval UI, a signed `tx_propose`, and a `deploy_propose` behind a READY gate).
