---
created: 2026-11-08
branch: rel/053-network
author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
status: active
---

# 0.5.2 on r1006: the node dir a 0.5.3 build opens (CORE-N3)

`src-tauri/src/upgrade_r1007_tests.rs` builds a 0.5.2 install on r1006 from the 0.5.1 fixtures
in `../upgrade-v0.5.1/` (app data, keychain entries, the RocksDB file set and node keys of
`node-r1005/`) with these two files laid over its node dir, and starts it with this build on a
filled r1007 profile.

| File | What it is |
|---|---|
| `node/node.toml` | `src-tauri/config/member-node.toml` at tag `v0.5.2`, rendered the way `network_profile::render_node_network` renders it for the `r1006` profile at `v0.5.2` (chain id 40204, `genesis_profile = "r1006"`, boot1-3 + rpc.citrate.ai), data dir `/Users/member/Library/Application Support/ai.citrate.core/node`, with one member edit (`max_peers = 64`) that the switch must keep byte for byte |
| `node/chain-genesis` | the marker 0.5.2 commits after its own r1005 to r1006 switch: the r1006 block 0 and a `migration <nonce>` line (the nonce is fixture data; any 32 hex digits commit the same way) |

The app data layout did not change between 0.5.1 and 0.5.2 for the files the switch must keep
(wallet vault, keychain slots, IPFS pins, Store models, daemons, Hermes settings, invites), so
those come from the 0.5.1 fixtures unchanged. The RocksDB files are placeholder bytes, as there.
