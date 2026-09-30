---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core + federation
companions: 00_OVERVIEW.md
---

# Baseline: what exists on 2026-09-30

A read-only survey across the federation (7 parallel lanes). "Implemented" ≠ "wired" ≠
"runtime-proven". Each row says which.

## citrate-core Hermes

| Area | State |
|---|---|
| Chat loop | `src/agent/harness.ts`: 19 tools, ≤6 turns, non-streaming; prompts hardcoded in `src-tauri/src/ai.rs`; no personas |
| Local model | Bundled llama-server, `--ctx-size 8192`, default Gemma 4 E4B Q4_0 (sha-pinned, resumable). Catalog: HF + GitHub search |
| Router | `modelRouter.ts` (local / registry / gateway), TLA+ `ModelRouter` |
| Sidecar | `hermes.rs` supervises `agent-sidecar` on loopback w/ bearer; commands `hermes_*`. **The chat loop never calls capsules** |
| Skills | 4 mechanisms: WASM capsules (2 bundled), SkillRegistry read-only, local markdown skills, localStorage prompt-skills |
| Memory | `mem-mcp` sidecar, BGE bundle, 3 tenants; 14 bundled docs ingested |
| Voice | STT via Web Speech API; no TTS |
| Approvals | Ceremony bridge (`bridge_pending`), chain/code/shell kinds; approval coverage of every write tool to be re-audited (S0.9) |
| Filesystem | No fs plugin; the chat agent has no file tools |
| MCP | No general client/server; memory socket only; Connections = OAuth tokens (no MCP calls) |
| IPFS | Bundled kubo; `storage_*` commands; no CLI; not agent tools |
| Onboarding | s1–s6 + model step; membership via external core-membership checkout |
| Formal | `ModelRouter`, `MemoryPack`, `ConsentGate`, `SidecarSupervisor` |
| Tests | ≈180 Rust + ≈75 TS Hermes-adjacent |

## Federation surfaces

| Repo | Relevant state |
|---|---|
| citrate-agent-runtime | agent-sidecar, capsules ×10, signing tiers, ApprovalQueue (TLA+), `agent-legacy` CapabilityGrant w/ `allowed_paths` + sandbox denylist, `agent-code` tools. Capsule fs/net currently rejected at load (no WASI preopens) |
| citrate-memories | 10 crates; mem-mcp 11 tools; Belnap confidence + CRDT merge; ingest is deterministic; SyncBundle export is the natural preload channel |
| citrate-explorer | MCP (22 read tools, resources, prompts); verify API; verified-source read exists over HTTP but not as an agent tool |
| citrate-chain | GhostDAG + REVM; precompiles 0x0107–0x0111, 0x0120, 0x0130, 0x0200–0x0202; `citrate_*` RPC incl. verify + pinArtifact; Foundry contracts w/ OpenZeppelin (no Solady vendored); CLI `wizard contract` (ERC-721 template); faucet service; 4337 stack |
| Paraconsensus | Gradient Papers No. II; `core/learning/belnap.rs`; precompile 0x0110; learning daemon; ContradictionLedger; memory CRDT. Checkpoint learning-root producer path present but not enabled |
| citrate-cluster | Admission-gated libp2p mesh; manual bootstrap; off by default in core; transport sign-off (CL-S4) open |
| citrate-comms | Self-admit invites (BLAKE3 token hash, 14-day TTL) + claim-back |
| citrate-sizeup | Probe + GGUF-header model fit implemented; `fleet.toml` renderer not wired; macOS verified only |
| compute-pool / settlement | Coordinator + workers; settlement Round state machine (TLA+); a live federated round has not run yet |
| Skills corpora | agentile (13), Trail of Bits (74), frontend-skills (36), hermes-agent fork (174), all agentskills.io SKILL.md |
| Faucet | `POST /faucet`, rate-limited drip; public status to be confirmed; core ADR 2026-07-27 said "no faucet" for the bond, to be revisited for deploy gas (D-3) |

## Known gaps this program closes

The chat loop and sidecar are separated; there are no fs/shell/browser/search/toolchain
tools; the knowledge bundle is tiny; no MCP host; no audit gate before deploy; no site
hosting path; no verified-source tool for the agent; no per-device identity for fleets;
no on-chain agent identity/benchmarks deployed; a live FL round has never run.
Chain-contract items are tracked in the federation sprint.
