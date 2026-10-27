---
created: 2026-10-18
branch: beta/k1a-gate0-reground
author: "Larry Klosowski + Claude Opus 5.5 (MAC lane)"
status: active (SCL-S1.4 evidence for g0-harness)
sprint: sprint-scl-s1-theory
wp: SCL-S1.4
commit_pin: b521d98
planset: .agentile/planset/2026-10-05-sidecar-lifecycle/
---

# SCL-S1.4: SPEC-001 re-grounded on `main`

**Commit pin:** citrate-core `main` at `b521d98` ("release: v0.5.1 third-party notices", #340).
The planset (05, S1.4) names `release/0.5.0-hermes-upskill` as the target. v0.5.0 and v0.5.1
have since been released from `main` (#320 and #340), so this record pins `main`, which is where
SCL work now lands.

This file re-grounds three things against the pin and adds one:

1. the supervisor line references used by SPEC-001 and 06_BUG_TRIAGE (section A);
2. SPEC-001's "61 manager commands" inventory (sections 0 to 3, 5);
3. the SC5 to SC7 touch lists and the profile anchors they depend on (section 6);
4. the command-to-consumer map SCL-S2's compatibility layer (O-2) is written against (section 4).

SPEC-001 (`docs/specs/001-owned-sidecar-lifecycle.md`) and ADR-0001 are content-locked and are
not edited; this file records where their references now point. Method for sections 0 to 6: every
`#[tauri::command]` in the scoped files was listed at both `30c789e` (SPEC-001's pin) and
`b521d98`; frontend consumers were found by grepping `src/` for the invoke names and the bridge
wrappers (test files excluded). Line numbers are on `b521d98`.

## A. Supervisor source references

| File | SHA-256 at `b521d98` | At `d16f194` (06_BUG_TRIAGE) |
|---|---|---|
| `kit/src/supervisor.rs` | `b865b76d374237c46ca1ca5d2bd833efc1125f79294f8632b0d5be874659aa00` | `14d72aec…2d1c`; changed since |
| `kit/src/supervisor_tests.rs` | `fffdaf0e921adeb0b04bb2aeec5addfd676cecec7d43f651d4d5ef57a0e1ef8e` | changed since |
| `kit/src/supervisor_windows_tests.rs` | `3bae40105926ba14d45db0970f8ee1a286aa5d607288115fe98fa6a63efcfac9` | unchanged |

Commits touching the supervisor since `d16f194`: `aef935a` (SCL-S0.3 `ChildWatch` hooks), `0925f97`
(`terminate_unsupervised` for the knowledge importer), `cac0334` and `9f862ba` (test changes).
None of them changes the five mechanisms below; only the line numbers moved.

| Defect (06) | Reference at `d16f194` | Same code at `b521d98` |
|---|---|---|
| (a) Stop in the Off wait `continue`s the spawn loop | `supervisor.rs:1000-1003` | `supervisor.rs:1063-1069` (`RunOutcome::Stopped` arm, `match control.recv()`, `Ok(ControlMsg::Stop) => continue`) |
| (a) `stop()` returns at once when already Off; doc says "Idempotent" | `:635-646`, doc `:634` | `:668-679`, doc `:667` |
| (a) Backoff path terminates instead | `:1335-1339` | `:1414-1419` (`backoff_or_control`, `Ok(ControlMsg::Stop)` arm) |
| (b) `refresh_health!` sets sustained health from elapsed time only | `:1182-1190` | `:1252-1261` |
| (b) the reset of `consecutive_failures` | `:985-986` | `:1049-1051` |
| (c) `ProbeRunner` dropped on timeout inside grace / outside grace | `:1266-1270` / `:1270-1274` | `:1345-1348` / `:1349-1354` |
| (c) a new probe thread spawned | `:1284` | `:1363` (`ProbeRunner::spawn`, defined `:1180-1187`) |
| (d) `BufRead::lines()` with no per-line byte bound | `:733`, `:748` | `:766`, `:781` (`spawn_log_readers`, `:758-790`) |
| (d) ring evicts by count only | `:428` | `:457-463` (`LogRing::push`; `LOG_RING_CAPACITY` `:438`) |
| (e) `terminate_child` signals only the child pid | `:877`, `:892` | `:915`, `:934` (`terminate_child`, `:907-947`) |
| (e) `to_command` sets no process group, session or Job | `:320` | `:351-374` |

Kit supervisor test counts at the pin: 24 Unix tests (`supervisor_tests.rs`) and 3 Windows tests
(`supervisor_windows_tests.rs`). The planset's "22 + 3" predates S0.3, which added 2 Unix tests.
`.github/workflows/lifecycle-platforms.yml` asserts 24 on macOS and 3 on Windows.

## 0. Headline

| Scope | Total | async / sync | offloaded / inline |
|---|---|---|---|
| SPEC-001 scope, the nine files (node, agent, memory, serve, embed_serve, ipfs, hermes, comms, cluster), @30c789e | **61** | 61 / 0 | **40 / 21** (SPEC figure reproduces exactly) |
| Same nine files, @b521d98 | **66** | 66 / 0 | **42 / 24** |
| Nine files + Hermes `#[path]` submodules (hermes_undo, hermes_workers, hermes_decide, hermes_personas, hermes_shell, hermes_mcp_cards), @b521d98 | **80** | 80 / 0 | **56 / 24** |
| Invite commands (`invites.rs`), @30c789e and @b521d98 | 9 | 9 / 0 | 0 / 9 (all inline; same 9 names at both revisions) |

Delta from 30c789e to b521d98 in the nine files: **+5, -0** (no command was removed or reclassified).

| New command | Owner | Class | Introduced by |
|---|---|---|---|
| hermes_session_open_unattended | Hermes | offloaded | 093a502 (HUP-S10.3) |
| hermes_session_close | Hermes | offloaded | 093a502 (HUP-S10.3) |
| cluster_devices | Cluster | inline | 7716db2 (HUP-S8.1) |
| cluster_group_seed | Cluster | inline | 88585ec (HUP-S8.4) |
| cluster_add_seed | Cluster | inline | 88585ec (HUP-S8.4) |

There are also 14 commands in Hermes `#[path]` submodules. None of those files existed at 30c789e. All 14 are offloaded: hermes_checkpoints/undo_step/checkpoint_diff/undo_session (a2b7f7a, HUP-S2.9), hermes_workers (2f16d17, S1.9), hermes_decide_stats (8a09e39, S5.3), hermes_personas/workflows/persona_check/track_workflow_run (512a45d, S3.3/S3.7), hermes_shell_pending/decide (f20bef9, S2.2), hermes_mcp_pending/decide (c4b2aba, S4.1). SPEC-001's 61 did not count them, so whether they belong in scope is a SPEC decision.

`embed_serve.rs` was added after 30c789e and defines **0** commands. EmbedServer is driven only from the Hermes manager (see §1).

For scale, the whole app had 163 `#[tauri::command]` in `src-tauri/src` at 30c789e and 370 at b521d98.

## 1. Command table (b521d98)

Handler list: `src-tauri/src/lib.rs:430` (`.invoke_handler(tauri::generate_handler![`) through `lib.rs:997` (`])`). The "reg" column is the line of the command's entry in that list.

### Node: `src-tauri/src/node.rs`

| Command | file:line | async | class | Manager/supervisor methods touched | reg |
|---|---|---|---|---|---|
| node_status | node.rs:1332 | async | offloaded | NodeManager::status (node.rs:897) | lib.rs:737 |
| node_start | node.rs:1348 | async | **inline** | wallet::address_auto_unlocked (keyring); grant_status::read_grant_status (RPC); set_coinbase (489); start (809, spawn) | lib.rs:738 |
| node_proposer_identity | node.rs:1397 | async | offloaded | wallet::address_auto_unlocked; proposer_pubkey (635, file read) | lib.rs:742 |
| node_validator_earnings | node.rs:1439 | async | **inline** | proposer_pubkey (file); validator::read_validator_rewards (RPC) | lib.rs:753 |
| node_register_validator | node.rs:1501 | async | **inline** | wallet::address_auto_unlocked; read_grant_status (RPC); validator::read_registration_nonce (RPC); sign_registration (643); ceremony.request | lib.rs:746 |
| node_arm_mining | node.rs:1579 | async | **inline** | arm_mining_if_synced (549): mining_gate RPC read, respawn on arm | lib.rs:749 |
| node_stop | node.rs:1586 | async | offloaded | stop (880) | lib.rs:739 |
| node_logs | node.rs:1607 | async | offloaded | logs (967) | lib.rs:763 |
| node_last_crash | node.rs:1648 | async | offloaded | no supervisor call; reads `<app_data>/node/crash-records.jsonl` | lib.rs:755 |

### NodeAgent: `src-tauri/src/agent.rs`

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| agent_status | agent.rs:902 | async | offloaded | AgentManager::status (578) | lib.rs:770 |
| agent_start | agent.rs:918 | async | offloaded | start (531) | lib.rs:771 |
| agent_stop | agent.rs:934 | async | offloaded | stop (556) | lib.rs:772 |
| user_claim | agent.rs:968 | async | **inline** | wallet::address_auto_unlocked; read_grant_status (RPC); NodeManager::proposer_pubkey; read_validator_rewards or earnings::read_claimable (RPC); bridge_user_claim (715, RPC) | lib.rs:778 |

### Memory: `src-tauri/src/memory.rs` (all offloaded)

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| memory_status | memory.rs:1169 | async | offloaded | MemoryManager::status (732) | lib.rs:787 |
| memory_ingest_docs | memory.rs:1191 | async | offloaded | docs_ingest::read_corpus_dir; ingest_docs_corpus (792) | lib.rs:795 |
| memory_seed_context | memory.rs:1243 | async | offloaded | seed_context (894) | lib.rs:798 |
| memory_start | memory.rs:1265 | async | offloaded | start (685) | lib.rs:788 |
| memory_stop | memory.rs:1281 | async | offloaded | stop (719) | lib.rs:789 |
| memory_recall | memory.rs:1299 | async | offloaded | recall (763) | lib.rs:790 |
| memory_search | memory.rs:1327 | async | offloaded | search_with (1014) | lib.rs:791 |
| memory_neighbors | memory.rs:1364 | async | offloaded | neighbors (1030) | lib.rs:792 |
| memory_constellation | memory.rs:1395 | async | offloaded | constellation (1047) | lib.rs:802 |

### Llama: `src-tauri/src/serve.rs` (all offloaded)

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| model_serve_start | serve.rs:661 | async | offloaded | fl_rounds::reapply_before_start; serve_plan::plan_for_model; ServeManager::current_model_path (510); ModelManager::is_ready; start_with_plan (394) | lib.rs:945 |
| model_serve_stop | serve.rs:697 | async | offloaded | stop (436) | lib.rs:946 |
| model_serve_status | serve.rs:716 | async | offloaded | status (528) | lib.rs:947 |
| model_inference_state | serve.rs:743 | async | offloaded | ModelManager::status; is_running (550); ai::select_inference_state | lib.rs:948 |

### Embed server: `src-tauri/src/embed_serve.rs`

No commands. `EmbedServer::ensure_started` (embed_serve.rs:218) and `stop` (252) are reached only through HermesManager: start → hermes.rs:723-724; failure and stop paths → hermes.rs:741-742 and 762-763. The manager is built at hermes.rs:1661-1668 (`with_embed`, 525).

### IPFS: `src-tauri/src/ipfs.rs` (all offloaded)

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| ipfs_start | ipfs.rs:189 | async | offloaded | IpfsManager::start (111) | lib.rs:950 |
| ipfs_stop | ipfs.rs:209 | async | offloaded | stop (128) | lib.rs:951 |
| ipfs_status | ipfs.rs:227 | async | offloaded | running (140) | lib.rs:952 |

### Hermes: `src-tauri/src/hermes.rs` (all offloaded)

Every command resolves `manager()` (hermes.rs:1586). That call lazily builds the global and never starts the sidecar; only hermes_start starts it.

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| hermes_start | hermes.rs:1697 | async | offloaded | hermes_mcp::sync_for_app; HermesManager::start (700, which also runs EmbedServer::ensure_started); status (782) | lib.rs:674 |
| hermes_status | hermes.rs:1717 | async | offloaded | is_running (819); remote_status (867) | lib.rs:675 |
| hermes_skills | hermes.rs:1737 | async | offloaded | list_skills (876) | lib.rs:676 |
| hermes_run_skill | hermes.rs:1750 | async | offloaded | run_skill (886) | lib.rs:677 |
| hermes_pending_approvals | hermes.rs:1773 | async | offloaded | pending_approvals (902) | lib.rs:678 |
| hermes_stop | hermes.rs:1795 | async | offloaded | stop (753) | lib.rs:679 |
| hermes_bridge_pending | hermes.rs:1811 | async | offloaded | bridge_pending (1242, RPC) | lib.rs:705 |
| hermes_resolve | hermes.rs:1843 | async | offloaded | resolve_head (1316) | lib.rs:706 |
| hermes_tracks | hermes.rs:2004 | async | offloaded | tracks (1171) | lib.rs:693 |
| hermes_brief_create | hermes.rs:2011 | async | offloaded | brief_create (1181) | lib.rs:694 |
| hermes_brief_check | hermes.rs:2027 | async | offloaded | brief_check (1214) | lib.rs:695 |
| hermes_session_open_unattended **(new)** | hermes.rs:2221 | async | offloaded | open_local_session: ServeManager::is_running/api_key/base_url; session_open (959) | lib.rs:686 |
| hermes_session_close **(new)** | hermes.rs:2234 | async | offloaded | session_close (1139) | lib.rs:687 |
| hermes_session_open | hermes.rs:2245 | async | offloaded | ServeManager::is_running/api_key/base_url/current_model_file/ctx_size; agent_grants::GrantStore::for_app (fs); session_open (959) | lib.rs:680 |
| hermes_session_send | hermes.rs:2287 | async | offloaded | session_send (1059) | lib.rs:681 |
| hermes_session_events | hermes.rs:2303 | async | offloaded | session_events (1076); hermes_headless::LEASES.view_page | lib.rs:682 |
| hermes_session_tool_result | hermes.rs:2322 | async | offloaded | LEASES.view_answered; session_tool_result (1095) | lib.rs:683 |
| hermes_session_stop | hermes.rs:2342 | async | offloaded | session_stop (1124) | lib.rs:684 |

### Hermes `#[path]` submodules (declared at hermes.rs:42-54, 2351-2362; all new since 30c789e, all offloaded)

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| hermes_checkpoints | hermes_undo.rs:319 | async | offloaded | checkpoints_list (229) | lib.rs:688 |
| hermes_undo_step | hermes_undo.rs:333 | async | offloaded | undo_step (257) | lib.rs:689 |
| hermes_checkpoint_diff | hermes_undo.rs:349 | async | offloaded | checkpoint_diff (267) | lib.rs:692 |
| hermes_undo_session | hermes_undo.rs:364 | async | offloaded | undo_session (310) | lib.rs:690 |
| hermes_workers | hermes_workers.rs:79 | async | offloaded | is_running; workers (54) | lib.rs:710 |
| hermes_decide_stats | hermes_decide.rs:146 | async | offloaded | is_running; decide_stats (133) | lib.rs:711 |
| hermes_track_workflow_run | hermes_personas.rs:297 | async | offloaded | track_workflow_run (256) | lib.rs:699 |
| hermes_personas | hermes_personas.rs:308 | async | offloaded | personas (168) | lib.rs:696 |
| hermes_workflows | hermes_personas.rs:316 | async | offloaded | workflows (173) | lib.rs:697 |
| hermes_persona_check | hermes_personas.rs:324 | async | offloaded | persona_check (179) | lib.rs:698 |
| hermes_shell_pending | hermes_shell.rs:147 | async | offloaded | shell_pending (88) | lib.rs:701 |
| hermes_shell_decide | hermes_shell.rs:157 | async | offloaded | shell_decide (113) | lib.rs:704 |
| hermes_mcp_pending | hermes_mcp_cards.rs:151 | async | offloaded | mcp_pending (66) | lib.rs:702 |
| hermes_mcp_decide | hermes_mcp_cards.rs:161 | async | offloaded* | mcp_decide (92) | lib.rs:703 |

\* After off_main returns, hermes_mcp_decide calls `app.opener().open_url(...)` in the async body (hermes_mcp_cards.rs:175-177) for an allowed `open_url` card.

Out of scope: `agent_tools.rs` (0 commands) and `hermes_chain.rs` (0) are also `#[path]` submodules. Other top-level modules reach the Hermes manager through `manager_for` (hermes.rs:1577) but were not classified here. They are browser.rs (8 commands), mcp_servers.rs (7), web_budgets.rs (8), skills_local.rs (5), chain_agent.rs (5), node_mcp.rs (6), anchor_proof.rs (2), benchmark_share.rs (1), deploy_gate_toolchain.rs (1) and hic_records.rs (1).

### Comms: `src-tauri/src/comms.rs` (all inline)

All the `groups_*` commands go through `route` (comms.rs:1118) → `ensure_started` (1066) → `ipc` (474). `ensure_started` holds the init mutex (`MANAGER_INIT`, 890), reads the keyring (`provision_comms_seed`, 963), and on first use or when not running calls `CommsMemberManager::start` (365), which spawns the daemon. `ipc` does the blocking UDS or named-pipe round trip with a 5 s deadline (`COMMS_IPC_TIMEOUT`, 680). On Windows that round trip runs on a worker thread via `run_with_deadline` (818), but the async command still blocks its runtime worker until the deadline.

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| groups_create | comms.rs:1139 | async | inline | route → ensure_started/start/ipc | lib.rs:637 |
| groups_list | comms.rs:1152 | async | inline | route | lib.rs:639 |
| groups_self_address | comms.rs:1167 | async | inline | device_identity (1006) → provision_comms_seed (keyring); never starts the daemon | lib.rs:640 |
| comms_relay_status | comms.rs:1181 | async | inline | MANAGER.get() only; relay_status (456) → member_ipc_quick (≤750 ms); never starts the daemon | lib.rs:638 |
| groups_join | comms.rs:1195 | async | inline | route | lib.rs:641 |
| groups_add_member | comms.rs:1202 | async | inline | route | lib.rs:642 |
| groups_roster | comms.rs:1287 | async | inline | route | lib.rs:643 |
| groups_assign_role | comms.rs:1302 | async | inline | route | lib.rs:644 |
| groups_offboard | comms.rs:1320 | async | inline | route | lib.rs:645 |
| groups_send | comms.rs:1330 | async | inline | route | lib.rs:646 |
| groups_messages | comms.rs:1340 | async | inline | route | lib.rs:647 |

### Cluster: `src-tauri/src/cluster.rs` (all inline)

`route` (cluster.rs:765) → `ensure_started` (715): `MANAGER.get_or_try_insert_ensured`, then `build_and_start` (733: cluster_mesh::current, device_link::mesh_identity (keyring), `start` 265) → `ipc` (323, 5 s deadline `CLUSTER_IPC_TIMEOUT` 492). `feed_roster` (769) awaits `comms::groups_roster` (the full Comms inline path above), loads DeviceLinkStore and PeerLinkStore from fs, then routes `SetRoster`.

| Command | file:line | async | class | Methods | reg |
|---|---|---|---|---|---|
| cluster_status | cluster.rs:822 | async | inline | feed_roster; route | lib.rs:648 |
| cluster_peers | cluster.rs:850 | async | inline | feed_roster; route | lib.rs:650 |
| cluster_devices **(new)** | cluster.rs:871 | async | inline | feed_roster; route | lib.rs:653 |
| cluster_join | cluster.rs:885 | async | inline | feed_roster; route | lib.rs:649 |
| cluster_share_file | cluster.rs:892 | async | inline | route (no feed_roster) | lib.rs:651 |
| cluster_group_seed **(new)** | cluster.rs:915 | async | inline | feed_roster; route | lib.rs:654 |
| cluster_add_seed **(new)** | cluster.rs:940 | async | inline | seed_text; feed_roster; route | lib.rs:655 |
| cluster_leave | cluster.rs:956 | async | inline | route (no feed_roster) | lib.rs:652 |

## 2. Totals per owner

| Owner | @30c789e total (off/inl) | @b521d98 total (off/inl) | Change |
|---|---|---|---|
| Node | 9 (5/4) | 9 (5/4) | 0 |
| NodeAgent | 4 (3/1) | 4 (3/1) | 0 |
| Memory | 9 (9/0) | 9 (9/0) | 0 |
| Llama (serve.rs) | 4 (4/0) | 4 (4/0) | 0 |
| Embed (embed_serve.rs) | file absent | 0 | file added, no commands |
| IPFS | 3 (3/0) | 3 (3/0) | 0 |
| Hermes (hermes.rs) | 16 (16/0) | 18 (18/0) | +2 offloaded |
| Comms | 11 (0/11) | 11 (0/11) | 0 |
| Cluster | 5 (0/5) | 8 (0/8) | +3 inline |
| **Nine-file total** | **61 (40/21)** | **66 (42/24)** | **+5 (+2/+3)** |
| Hermes submodules | files absent | 14 (14/0) | +14 offloaded |
| **With submodules** | 61 (40/21) | **80 (56/24)** | +19 |

Every command in scope is `async`; there are 0 sync commands. The 24 inline commands at HEAD are Node ×4, NodeAgent ×1, Comms ×11 and Cluster ×8.

## 3. Invite commands: `src-tauri/src/invites.rs` (9, all async, all inline, 0 off_main)

Shared helpers: `store_path` (121) runs `create_dir_all`, `load` (131) does `fs::read` of `invites/pending.json`, and `save` (138) writes through `fsutil::write_secret_file`. `referral_log_path` (284), `load_referrals` (294) and `append_referral` (306) do the same for `invites/referral-log.json`. The Comms helpers below all go through comms `route`, which can start the daemon: `publish_invite` (comms.rs:1250), `revoke_invite` (1267), `redeem_invite` (1276), `submit_claim` (1216) and `poll_claims` (1236).

| Command | file:line | Inline Comms | Inline fs | Inline keyring | reg |
|---|---|---|---|---|---|
| group_invite_create | invites.rs:164 | publish_invite; group_name_of (113) awaits comms::groups_list | load, save, append_referral | via ensure_started on a cold start | lib.rs:495 |
| group_invite_redeem | invites.rs:218 | redeem_invite | append_referral | via ensure_started on a cold start | lib.rs:496 |
| group_referral_log | invites.rs:319 | none | load_referrals | none | lib.rs:502 |
| group_referral_export | invites.rs:326 | none | load_referrals | none | lib.rs:503 |
| group_invite_submit_claim | invites.rs:345 | submit_claim | none | comms::device_identity (keyring) | lib.rs:497 |
| group_invite_poll_claims | invites.rs:363 | poll_claims once per outstanding invite (sequential) | load | via ensure_started on a cold start | lib.rs:498 |
| group_invites | invites.rs:427 | none | load (create_dir_all + read) | none | lib.rs:499 |
| group_invite_verify_consume | invites.rs:438 | none | load, save | none | lib.rs:500 |
| group_invite_revoke | invites.rs:458 | revoke_invite | load, save | via ensure_started on a cold start | lib.rs:501 |

Since 30c789e, invites.rs changed by 2 lines (2 insertions, 2 deletions). The command set is the same 9 names.

## 4. Command → frontend consumer map

Every invoke goes through the timeout-wrapped `invoke` in `src/bridge/tauri/invoke.ts` (12 s default; overrides in the UNBOUNDED set and the LONG map). Wrapper objects:

- `bridge.node`, `bridge.agent`, `bridge.memory`, `bridge.model` and `bridge.chat` are defined inline in `src/bridge/tauri/index.ts`.
- `bridge.agentHarness` is `tauriAgentHarness` in `src/bridge/tauri/agent.ts:29`.
- `bridge.groups` is `tauriGroups` in `src/bridge/tauri/comms.ts:22`.
- `bridge.cluster` is `tauriCluster` in `src/bridge/tauri/cluster.ts`.
- `bridge.invites` is `tauriInvites` in `src/bridge/tauri/invites.ts:5`.
- The last four are wired in `src/bridge/cx.ts:51-59`.

Test files were excluded. "NONE" means no non-test caller outside `src/bridge` was found.

| Command | Invoke site | Wrapper | Main callers |
|---|---|---|---|
| node_status | bridge/tauri/index.ts:230 | node.status | shell/store.ts:1332 (node poll), store.ts:3312 (agent tool `node_status`) |
| node_start | index.ts:233 | node.start | store.ts:3937-3938 (startNode, def 3909; autostart 728; stall restart 3881; 3906) |
| node_proposer_identity | NONE | none | **no frontend consumer** |
| node_validator_earnings | index.ts:247-248 | node.validatorEarnings | store.ts:4188 |
| node_register_validator | index.ts:241 | node.registerValidator | store.ts:4450 |
| node_arm_mining | index.ts:255 | node.armMining | store.ts:1388-1389, 1511, 4438 |
| node_stop | index.ts:236 | node.stop | store.ts:3874 (stall watchdog), 3976-3977 (stopNode) |
| node_logs | index.ts:266 | node.logs | store.ts:1454 |
| node_last_crash | index.ts:244 | node.lastCrash | store.ts:3900 |
| agent_status | index.ts:300 | agent.status | **NONE** (wrapper unused) |
| agent_start | index.ts:303 | agent.start | **NONE** |
| agent_stop | index.ts:306 | agent.stop | **NONE** |
| user_claim | index.ts:327 (after `agent_earnings` at 321-323) | agent.claim | store.ts:4267 |
| memory_status | index.ts:334 | memory.status | store.ts:834 |
| memory_ingest_docs | index.ts:364 | memory.ingestDocs | store.ts:949 |
| memory_seed_context | index.ts:368 | memory.seedContext | store.ts:943 |
| memory_start | index.ts:337 | memory.start | store.ts:864 |
| memory_stop | index.ts:340 | memory.stop | **NONE** |
| memory_recall | index.ts:348 | memory.recall | store.ts:2416, 3298 |
| memory_search | index.ts:351-352 | memory.search | store.ts:970, 3290; agent/eval.ts:174 |
| memory_neighbors | index.ts:357 | memory.neighbors | **NONE** |
| memory_constellation | index.ts:360 | memory.constellation | store.ts:972 |
| model_serve_start | index.ts:285 | model.serveStart | store.ts:2865, 4131 |
| model_serve_stop | index.ts:289 | model.serveStop | store.ts:2860 |
| model_serve_status | NONE | none | **no frontend consumer** |
| model_inference_state | index.ts:420 | chat.inferenceState | store.ts:1013, 4147 |
| ipfs_start | index.ts:259 | node.startIpfs | store.ts:3934 |
| ipfs_stop | NONE (only a timeout entry at invoke.ts:110) | none | **no frontend consumer** |
| ipfs_status | NONE | none | **no frontend consumer** |
| hermes_start | bridge/tauri/agent.ts:33 | agentHarness.start | shell/slices/agent.ts:155 (startRuntime) |
| hermes_status | agent.ts:36 | agentHarness.status | slices/agent.ts:105, 167, 223; store.ts:1025-1026 (sidecarUp probe) |
| hermes_skills | agent.ts:39 | agentHarness.skills | slices/agent.ts:107 |
| hermes_run_skill | agent.ts:45 | agentHarness.runSkill | slices/agent.ts:222 |
| hermes_pending_approvals | agent.ts:48 | agentHarness.pendingApprovals | slices/agent.ts:107, 224 |
| hermes_stop | agent.ts:51 | agentHarness.stop | slices/agent.ts:192 (stopRuntime) |
| hermes_bridge_pending | agent.ts:56 | agentHarness.bridgePending | store.ts:4760 |
| hermes_resolve | agent.ts:60 | agentHarness.resolve | store.ts:4741; slices/agent.ts:249 |
| hermes_tracks | agent.ts:86 | agentHarness.tracks | components/InterviewCard.tsx:103 (api = bridge.agentHarness, AgentChat.tsx:283-286) |
| hermes_brief_create | agent.ts:89 | agentHarness.briefCreate | InterviewCard.tsx:116, 144 |
| hermes_brief_check | agent.ts:92 | agentHarness.briefCheck | InterviewCard.tsx:168 |
| hermes_session_open_unattended | agent.ts:80 | agentHarness.sessionOpenUnattended | store.ts:2895 (runDaemonTurn, 2888) → daemons/turn.ts:107 |
| hermes_session_close | agent.ts:83 | agentHarness.sessionClose | store.ts:1056 (sidecar provider), 2900 (daemon); agent/sidecarProvider.ts:613; agent/learnLauncher.ts:195 (via shell/TeachHermesCard.tsx:103) |
| hermes_session_open | agent.ts:65 | agentHarness.sessionOpen | store.ts:1048 → sidecarProvider.ts:271; learnLauncher.ts:145 |
| hermes_session_send | agent.ts:68 | agentHarness.sessionSend | store.ts:1049, 2896; sidecarProvider.ts:639, 646; learnLauncher.ts:169 |
| hermes_session_events | agent.ts:71 | agentHarness.sessionEvents | store.ts:1050, 1052, 2897; sidecarProvider.ts:337, 420, 689; learnLauncher.ts:174 |
| hermes_session_tool_result | agent.ts:74 | agentHarness.sessionToolResult | store.ts:1053, 2898; sidecarProvider.ts:562 |
| hermes_session_stop | agent.ts:77 | agentHarness.sessionStop | store.ts:1054, 2899; sidecarProvider.ts:316 |
| hermes_checkpoints | agent.ts:103 | agentHarness.checkpoints | slices/agentUndo.ts:81 (refreshUndoPanel ← store.ts:2528, 2713, 3025; popout/appHost.ts:94); popout/diffHost.ts:35 |
| hermes_undo_step | agent.ts:106 | agentHarness.undoStep | agentUndo.ts:98 (undoChange ← components/AgentChat.tsx:221, surfaces/Journal.tsx:469, appHost.ts:96) |
| hermes_checkpoint_diff | agent.ts:113 | agentHarness.checkpointDiff | popout/diffHost.ts:36 (diffOps, used at diffHost.ts:42) |
| hermes_undo_session | agent.ts:109 | agentHarness.undoSession | agentUndo.ts:115 (undoSession ← appHost.ts:96) |
| hermes_workers | popout/appHost.ts:86 (direct invoke) | none (appHost `workers`) | Activity-monitor popout host |
| hermes_decide_stats | popout/appHost.ts:88 (direct invoke) | none (appHost `decide`) | Activity-monitor popout host |
| hermes_track_workflow_run | agent.ts:164 | agentHarness.trackWorkflowRun | store.ts:1058 → sidecarProvider.ts:760 |
| hermes_personas | agent.ts:155 | agentHarness.personas | components/PersonaPicker.tsx:157 (api = bridge.agentHarness at surfaces/Settings.tsx:1178, onboarding/Onboarding.tsx:1005) |
| hermes_workflows | agent.ts:161 | agentHarness.workflows | PersonaPicker.tsx:157 |
| hermes_persona_check | agent.ts:158 | agentHarness.personaCheck | PersonaPicker.tsx:197 |
| hermes_shell_pending | agent.ts:167 | agentHarness.shellPending | store.ts:1061 → sidecarProvider.ts:333 |
| hermes_shell_decide | agent.ts:170 | agentHarness.shellDecide | store.ts:1062 → sidecarProvider.ts:355 |
| hermes_mcp_pending | agent.ts:173 | agentHarness.mcpPending | store.ts:1064 → sidecarProvider.ts:379 |
| hermes_mcp_decide | agent.ts:176 | agentHarness.mcpDecide | store.ts:1065 → sidecarProvider.ts:393 |
| groups_create | bridge/tauri/comms.ts:24 (+ groups_roster at 26) | groups.create | store.ts:3357; slices/groups.ts:137 |
| groups_list | comms.ts:31 | groups.list | store.ts:2073, 3324, 4647; slices/cluster.ts:85, 207; slices/groups.ts:126; surfaces/Train.tsx:41 |
| groups_self_address | comms.ts:35 | groups.selfAddress | store.ts:2093; slices/groups.ts:98 |
| comms_relay_status | comms.ts:63 | groups.relayStatus | surfaces/RelayStatusChip.tsx:29 |
| groups_join | comms.ts:38 | groups.join | slices/groups.ts:251 |
| groups_add_member | comms.ts:41 | groups.addMember | slices/groups.ts:239 |
| groups_roster | comms.ts:26 (inside create), 44 | groups.roster | store.ts:3330; slices/groups.ts:153, 212, 226, 241 |
| groups_assign_role | comms.ts:47 | groups.assignRole | slices/groups.ts:210 |
| groups_offboard | comms.ts:50 | groups.offboard | slices/groups.ts:224 |
| groups_send | comms.ts:53 | groups.send | store.ts:4658; slices/cluster.ts:213; slices/groups.ts:186 |
| groups_messages | comms.ts:56 | groups.messages | slices/groups.ts:154, 197 |
| cluster_status | bridge/tauri/cluster.ts:25 | cluster.status | slices/cluster.ts:97 |
| cluster_peers | cluster.ts:28 | cluster.peers | slices/cluster.ts:98 |
| cluster_devices | cluster.ts:42 | cluster.devices | slices/cluster.ts:130 |
| cluster_join | cluster.ts:31 | cluster.join | slices/cluster.ts:266 |
| cluster_share_file | cluster.ts:34 | cluster.shareFile | slices/cluster.ts:294, 311 |
| cluster_group_seed | cluster.ts:83 | cluster.groupSeed | **NONE** (wrapper unused) |
| cluster_add_seed | cluster.ts:86 | cluster.addGroupSeed | **NONE** (wrapper unused) |
| cluster_leave | cluster.ts:37 | cluster.leave | slices/cluster.ts:278 |
| group_invite_create | bridge/tauri/invites.ts:7 | invites.create | store.ts:3388; surfaces/Groups.tsx:342, 365 |
| group_invite_redeem | invites.ts:10 | invites.redeem | Groups.tsx:422, 568 |
| group_invites | invites.ts:13 | invites.list | Groups.tsx:297 |
| group_invite_verify_consume | invites.ts:16 | invites.verifyConsume | Groups.tsx:313, 393 |
| group_invite_revoke | invites.ts:19 | invites.revoke | Groups.tsx:935 |
| group_invite_submit_claim | invites.ts:22 | invites.submitClaim | Groups.tsx:438, 581 |
| group_invite_poll_claims | invites.ts:25 | invites.pollClaims | Groups.tsx:304 |
| group_referral_log | invites.ts:28 | invites.referralLog | **NONE** (wrapper unused) |
| group_referral_export | invites.ts:31 | invites.exportReferralLog | Groups.tsx:980 |

No frontend consumer (11): node_proposer_identity, agent_status, agent_start, agent_stop, memory_stop, memory_neighbors, model_serve_status, ipfs_stop, ipfs_status, cluster_group_seed, cluster_add_seed (and group_referral_log among the invites). Four of them (node_proposer_identity, model_serve_status, ipfs_stop, ipfs_status) have no invoke site at all. The rest have a bridge wrapper with no caller. The supervisor-backed ones are still reached Rust-side; for example, `shutdown_all_sidecars` stops the agent, memory, serve and ipfs managers.

## 5. generate_handler! registration

All commands above are registered in the single list at `src-tauri/src/lib.rs:430-997`. Grouped line ranges:

- invites 495-503
- comms 637-647
- cluster 648-655
- hermes 674-711 (submodule paths `hermes::undo::` 688-692, `hermes::personas::` 696-699, `hermes::shell::`/`hermes::mcp_cards::` 701-704, `hermes::workers::` 710, `hermes::decide::` 711)
- node 737-763
- agent 770-778
- memory 787-802
- serve 945-948
- ipfs 950-952

Per-command lines are in the "reg" columns of §1 and §3. Every command found is registered.

## 6. SC5-SC7 touch-list path check (b521d98)

| Path | Exists | Change since 30c789e |
|---|---|---|
| src-tauri/src/node.rs | yes (1693 lines) | M |
| src-tauri/src/agent.rs | yes (1069) | M |
| src-tauri/src/memory.rs | yes (1422) | M |
| src-tauri/src/ipfs.rs | yes (240) | M |
| src-tauri/src/serve.rs | yes (781) | M |
| src-tauri/src/embed_serve.rs | yes (290) | **A** (added after 30c789e) |
| src-tauri/src/comms.rs | yes (1356) | M |
| src-tauri/src/cluster.rs | yes (966) | M |
| src-tauri/src/hermes.rs | yes (2387) | M |
| src-tauri/src/lib.rs | yes (1299) | M |
| src-tauri/capabilities/default.json | yes (15) | M; `"updater:default"` at **line 10**, `"process:default"` at **line 11** (same lines at 30c789e) |
| src/shell/updater.ts | yes (196) | unchanged; `relaunch` import from `@tauri-apps/plugin-process` at 176-177 |
| src/shell/store.ts | yes (5507) | M |
| src/App.tsx | yes (307) | M |

No renames among these paths (`git diff -M --name-status 30c789e HEAD` shows only M or A).

| Anchor | Current location |
|---|---|
| `server_healthy: serve.0.is_running()` | serve.rs:769 (in model_inference_state_sync, 759). The field is declared at ai.rs:218 (`ProviderInputs`, ai.rs:214); `ServeManager::is_running` is at serve.rs:550 (Supervisor state == Running) |
| `select_inference_state` | defined ai.rs:266 (uses `server_healthy` at 268); called serve.rs:773 |
| `shutdown_all_sidecars` | defined lib.rs:1019; called lib.rs:1010 (RunEvent handler) and local_data.rs:495 (inside off_main in local_data_delete). It stops knowledge_import, then node/agent/memory/serve/ipfs `.0.stop()`, then comms::shutdown (comms.rs:895), cluster::shutdown (cluster.rs:636) and hermes::shutdown (hermes.rs:1511) |
| `sweep_orphan_sidecars` | defined lib.rs:218; called lib.rs:286 via `startup_barrier::StartupBarrier::global().run_cleanup(...)` in setup |
| RunEvent Exit/ExitRequested | lib.rs:1000 `.run(\|app_handle, event\|` with `matches!(event, tauri::RunEvent::ExitRequested { .. } \| tauri::RunEvent::Exit)` at lib.rs:1006-1009, then `shutdown_all_sidecars` at 1010 |
| `local_data_delete` | local_data.rs:487 (async command; doc at 482; off_main at 494); registered lib.rs:925 |
| `mapNodeState` | src/shell/store.ts:396 (def); used store.ts:1424 |
| Stall watchdog `setState({ node: "off" })` | store.ts:3878, inside checkNodeLiveness (3860; stall branch 3868-3882; interval set at 747, every 30 s). Not the watchdog: store.ts:3972 is the same setState in stopNode |
| Llama grace / healthy_after | serve.rs:75 `LLAMA_START_GRACE = 180 s` (used 378); serve.rs:64 `LLAMA_HEALTHY_AFTER = 60 s` (used 428). Health interval 5 s at serve.rs:59 |
| Embed grace / healthy_after | embed_serve.rs:50 `EMBED_START_GRACE = 60 s` (used 240); embed_serve.rs:51 `EMBED_HEALTHY_AFTER = 30 s` (used 245) |
| Comms HealthCheck | comms.rs:103 `COMMS_START_GRACE = 20 s` (HealthCheck at 348, grace 350; socket-connect probe); comms.rs:102 `COMMS_HEALTHY_AFTER = 30 s` (used 385) |
| Cluster HealthCheck | cluster.rs:74 `CLUSTER_START_GRACE = 20 s` (HealthCheck at 248, grace 250); cluster.rs:73 `CLUSTER_HEALTHY_AFTER = 30 s` (used 287) |
| Hermes HealthCheck | hermes.rs:77 `HERMES_START_GRACE = 20 s` (HealthCheck at 664, grace 666; HTTP /health probe); hermes.rs:75 `HERMES_HEALTHY_AFTER = 30 s` (used 733) |
| Node healthy_after | node.rs:85 `NODE_HEALTHY_AFTER = 90 s` (used 855). The node spec sets no HealthCheck (no `health_check` in node.rs) |

The agent, memory and ipfs managers set neither a HealthCheck nor a custom healthy_after, so they use the supervisor defaults.
