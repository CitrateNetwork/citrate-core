---
created: 2026-10-01
branch: hup/n4-node-mcp (updated on hup/n5-nodemcp-rest and hup/n6-mcp-host, 2026-10-01 and 2026-10-04)
author: Larry Klosowski + Claude Opus 5.5
status: implemented (HUP-S4.2 + S8.5; HUP-S4.1 Hermes entry); off by default; port, personal-memory scope, Hermes write tools and task TTL pending owner sign-off
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
`env = { CITRATE_NODE_MCP_TOKEN = "..." }`. Before it sends the token at all, it asks the server to prove it is Citrate Core: it sends a
fresh 32-byte challenge with no token, and the server answers with an HMAC of that challenge keyed
by the SHA-256 of each token it holds (`X-Citrate-Identity`). Only if one of those matches the
shim's own token does the token go out; otherwise each request is answered with an error that says
the token was not sent. The check runs before every request, so another program that takes the
port while Core is off, or after Core quits mid-session, never sees a token.

Check it from Claude Code with `/mcp`, or ask it to "use citrate-node to show the chain head".

## Hermes in this app (HUP-S4.1, S4.2, S8.5)

Hermes reaches this server through a built-in entry, **Your node**, in the Agent surface's
**Connected tools (MCP)** card. It is off by default, and it can be turned on only while the node
MCP server is on. When it is on, at every Hermes start core:

1. keeps Hermes's current connect token if it is still live, or else revokes every earlier one
   (their sessions and pending requests close) and mints a new token labelled
   `Hermes (built-in)`. That token is held only as its SHA-256, **in memory**: it is never
   written to `tokens.json`, and it ends when the app exits;
2. writes the `node` entry into Hermes's MCP allowlist (`hermes/mcp.json`, 0600): this
   executable run as the stdio shim (`--mcp-stdio`), with `CITRATE_NODE_MCP_TOKEN` and
   `CITRATE_NODE_MCP_PORT` in the entry's explicit environment. The plaintext token is in this
   file (owner-only) because the sidecar hands it to the shim it starts; the server itself keeps
   only the hash, and the token stops working when it is replaced, revoked, or the app exits.

Hermes is offered the read tools only (pending owner sign-off, A24), and this is enforced on both
sides from one switch (`HERMES_NODE_WRITE_TOOLS` in `hermes_mcp.rs`): the sidecar entry says
`allow_write_tools = false`, and Hermes's connect token itself is **read-only**. The server does
not list write tools to a read-only token and refuses a call to one before anything is queued, so
a sidecar that ignored its allowlist still could not raise an approval card. The shim checks the
server's identity with the in-memory token exactly as it does with a member's token. The label
`Hermes (built-in)` is reserved: a member cannot issue a token with it, so no client can appear as
Hermes on an approval card, and revoking Hermes's tokens never touches a member's. Settings marks
the token "read tools only".

Were the switch turned on, write tools would still never act on their own: each would become a
request in this server's approval inbox (shown in the app, labelled with the token), and a
transaction is signed only through the SignatureCeremony. Once a Hermes session has read MCP output
(always untrusted), an effectful MCP call also waits for the member on the sidecar's MCP approval
card before it is even sent. The names `node` and `citrate-node` stay reserved, so a member-added
server cannot take them.

## What it offers

**Resources:** `citrate://node/status`, `citrate://chain/head`, `citrate://wallet` (public
address and balance), `citrate://addresses` (the address book this build ships with),
`citrate://precompiles`, and two templates: `citrate://memory/{tenant}/search?q={query}` and
`citrate://contract/{address}/abi` (the ABI registry: a 40204 contract's ABI from CitrateScan's
verified sources, the same source as the Contract reader, with its match status: verified,
partial match, or not verified. The source text is left out; no ABI is ever guessed).

**Read tools** (`readOnlyHint: true`): `node_status`, `chain_head`, `dag_stats`, `get_balance`,
`chain_call` (eth_call), `estimate_gas`, `get_logs` (one contract, at most 5,000 blocks),
`precompile_table`,
`precompile_call` (read-only eth_call to a precompile in the table: 0x0107-0x0109, 0x0110,
0x0111, 0x0120, 0x0130, 0x0200-0x0202), `ed25519_verify` (a typed helper for ED25519_VERIFY
at 0x0120: it encodes `public key (32 bytes) || signature (64 bytes) || message (at most 8 KiB)`,
calls the precompile read-only and returns `valid`; an answer that is not a 32-byte word is an
error, never "valid"), `wallet_info`, `address_book`, `memory_search`,
`groups_list`, `cluster_status`, `cluster_peers`, `cluster_devices` (each member's linked
machines and whether they are connected; HUP-S8.3), `invites_list` (ids only, never links or
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

**Budgeted tool** (HUP-S6.5; `readOnlyHint: false`, `destructiveHint: false`): `faucet_request`
`{initcode_hash}` asks the Citrate faucet for deploy gas for a deploy the deploy gate marked
READY. It runs at once, without an approval card, because it acts only inside the faucet budget
the member granted in Settings, Budgets (HIC-2), which is off by default. Core picks the
recipient (the member's own wallet), checks that the balance is short of the deploy's gas, and
allows one request per 24 hours. The tool cannot name an address, an amount or a time. See
[FAUCET_IN_APP.md](FAUCET_IN_APP.md).

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

**Deploy-and-confirm.** For an approved `tx_propose` or `deploy_propose`, the task does not stop
at the transaction hash. `tasks/get` reads the receipt (`eth_getTransactionReceipt`, a read) and
the task stays `working` ("sent; waiting for the chain to include it") until the transaction is in
a block. It then completes with the tool result plus a `receipt` (status, block number, gas used,
and for a deploy the new contract's address), or with an `isError` result if it reverted.
`request_status` adds the same information as `confirmation` (`waiting`, `included`, `reverted`,
or `unknown` when the receipt cannot be read). A malformed receipt is never reported as included.

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

- The planset's other long-running tasks: "sync" and "FL round" as MCP tasks. Neither has a
  write behind it on this server today (node sync runs by itself; FL rounds are not exposed over
  MCP), so there is nothing for a task to track yet.
- Typed helpers for the other precompiles (tensor commit, Merkle, Belnap Q16, routing, CommD
  fold, x402): their input formats need the chain-side precompile work (HUP-S7.2) to settle.
  `precompile_call` reaches them with raw calldata.
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

## Demo transcript, part 3 (fan-out 6 review, 2026-10-04)

Recorded 2026-10-04 on branch `hup/n5-nodemcp-rest` after the merge of `hup/m2-core`, macOS
arm64. Same setup as part 2: the real server code (`node_mcp_demo`, from a snapshot of the built
test binary) on port 47299, chain reads from the live public 40204 RPC (height 132335 at the
time), and no app behind it (no wallet, ceremony, daemons or deploy gate). The harness also issued
the read-only token core gives Hermes (`NODE_MCP_DEMO_RO_TOKEN_FILE`). Tokens are elided.

**Claude Code 2.1.281 as the client** (HTTP transport, bearer header):

```text
$ claude -p "Use only the citrate-node MCP server. 1) Read the MCP resource
    citrate://contract/0xBa4aBd4f3fcA5365b2451b4E9662e4Cfd22b3ad5/abi ... 2) Call ed25519_verify
    once ... 3) Call tx_propose once ..." --strict-mcp-config --mcp-config citrate-node.json \
    --allowedTools "ListMcpResourcesTool,ReadMcpResourceTool,mcp__citrate-node__ed25519_verify,mcp__citrate-node__tx_propose"
1. The ABI resource came back with status `unverified`, contractName `null` and 0 ABI entries
   (`abi` is `null`). It does report a contract with 13,026 bytes of code, and says to submit the
   source at `POST /api/verify` to get it verified.
2. `ed25519_verify` didn't return a valid/invalid answer. It returned this error: `the precompile
   returned "0x", not a 32-byte word (is ED25519_VERIFY active on this chain?)`
3. `tx_propose` also failed, with the error `demo harness: no ceremony`. No request id came back,
   so no signature request was opened and nothing was proposed or sent.
```

Both "failures" are the honest answers: CitrateScan has no verified source for the rerolled 40204
contracts yet (ModelRegistry, SkillRegistry, AgentSBT, CitrateMemberSBT and CitratePaymaster read
the same), live 40204 answers `0x` at 0x0120 (ED25519_VERIFY is not active there), and the harness
has no ceremony.

**The read-only token core gives Hermes** (curl, stateless era):

```text
read-only token, tools/list       -> 200, 20 tools, write tools listed: []
read-only token, tools/call pin_add
                                  -> 200 {"isError":true,"content":[{"text":"pin_add is a write tool,
                                     and this connect token is limited to the read tools."}], ...}
member token, tools/list          -> 28 tools
member token, resources/templates/list
                                  -> [citrate://memory/{tenant}/search?q={query},
                                      citrate://contract/{address}/abi]
```

Deploy-and-confirm (a task that stays `working` until the receipt is in a block) is covered by
the unit tests against fixture receipts; it needs a member to approve a real transaction in the
app before it can be recorded live.

Still to record: the same flows against the packaged app with a member approving a request in
Settings (the approval UI, a signed `tx_propose`, and a `deploy_propose` behind a READY gate).

## Hermes demo transcript (HUP-S4.1)

Recorded on `hup/n6-mcp-host` before that branch was stacked on the node MCP lane. At the time the
`node` entry offered Hermes the write tools and its token was labelled "Hermes in this app"; on the
stacked branch the entry and the token are read only and the label is `Hermes (built-in)` (see
"Hermes in this app" above), so the `tx_propose` step below is no longer offered to Hermes.

Recorded 2026-10-04. citrate-core's `hermes_node_entry_demo` (ignored test) ran the real node
MCP server on port 47298 with live 40204 chain reads, minted Hermes's in-memory token and wrote
the allowlist core gives Hermes; citrate-agent-runtime's `node_demo` (ignored test) then loaded
that allowlist into Hermes's MCP host, which started the release `citrate-core --mcp-stdio` shim
as the `node` stdio server. The harness backend records a proposed transaction instead of opening
the SignatureCeremony (nothing is signed; in the app the ceremony opens and the member decides).

Core side:

```text
allowlist core wrote for Hermes: {"servers":[{"allow_write_tools":true,"args":["--mcp-stdio"],"command":".../citrate-core/target/release/citrate-core","env":{"CITRATE_NODE_MCP_PORT":"47298","CITRATE_NODE_MCP_TOKEN":"cnmcp_<minted for Hermes, elided>"},"name":"node","timeout_ms":30000,"transport":"stdio"}]}
serving http://127.0.0.1:47298/mcp ; tokens: ["Hermes in this app"]
approval inbox: {"id":"mcpr-1","tokenId":"4836092d","origin":"mcp:Hermes in this app via citrate-hermes","summary":"Sign and send a transaction: Transfer","kind":"signature","ceremony_id":"harness-ceremony-1",...,"state":"pending","decidedMs":null}
recent calls: initialize, tools/list, tools/call chain_head, tools/call tx_propose (all by token 4836092d)
```

Hermes's MCP host side:

```text
server node (stdio): Ready, protocol Some("2025-06-18"), era Some(Legacy), 21 tools offered, skipped []
tools Hermes is offered: [mcp__node__node_status, mcp__node__chain_head, ... mcp__node__tx_propose, mcp__node__cluster_join, mcp__node__cluster_share, mcp__node__invite_create, mcp__node__invite_revoke]
>>> mcp__node__chain_head {}
<<< untrusted: {"chainId": 40204, "height": 132335, "source": "public-rpc"}
>>> mcp__node__tx_propose {"to":"0x52908400098527886E0F7030069857D2E4169EE7","value_wei":"1"}
<<< untrusted: {"requestId": "mcpr-1", "state": "pending", "summary": "Sign and send a transaction: Transfer",
    "next": "The member must approve this in Citrate Core (Settings, API endpoints & keys, Node MCP server). Nothing happens until they do. ..."}
```

The node server speaks the handshake protocol, so the host's `server/discover` probe was refused
and it fell back to `initialize` (era legacy). In a real session the chain_head result taints
the session, so the tx_propose call would first wait on the sidecar's MCP approval card, and
then on the member's approval of request `mcpr-1` in the app.
