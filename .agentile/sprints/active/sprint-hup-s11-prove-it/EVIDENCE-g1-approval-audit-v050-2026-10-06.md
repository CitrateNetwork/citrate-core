---
created: 2026-10-06T16:00:00Z
branch: release/v0.5.0-gates
author: Larry Klosowski + Claude Opus 5.5
status: active
gate: HUP g1-approval-audit (v0.5.0 re-run)
---

# g1-approval-audit re-run on the v0.5.0 head (2026-10-06)

The v0.5.0 cut needs the approval-audit enumeration re-run on the release head, which since the
last run (#191, 2026-10-04) adds terminal access (#236: the Hermes sidecar's `shell_run`, on by
default) and model delete (#237: a member-only Models screen action that must never be an agent
tool). Base: `release/0.5.0-hermes-upskill` @ 144b4dc (after #256), on branch
`release/v0.5.0-gates`.

## The surfaces an agent can act through, and what proves each one

| Surface | Who decides | Enumeration / proof |
|---|---|---|
| Core-hosted chat tools (`AGENT_TOOLS`: the in-app loop, the sidecar's core-hosted calls, the idle-view dispatcher, daemons) | `store.handleTool`: every non-read tool stops at a member approval (`requestSig` or the wallet review ceremony) | `src/shell/agentToolGates.test.ts` tripwire over every tool not on the reviewed read-only list; `src/agent/toolAnnotations.test.ts` (every tool annotated; `effect: none` exactly for the read-only list; a tool that asks is never `none`) |
| Headless (no view) | only `node_status`, `groups_list`; any other tool answers "needs the member" | `src-tauri/src/hermes_headless.rs` `HEADLESS_TOOLS` and its tests |
| Sidecar `shell_run` (terminal access, #236) | the sidecar holds every command; core shows the exact argv, folder, program and sandbox on a card; only an explicit Approve runs it | core: `src/shell/shellRunApproval.store.test.ts`, `src/agent/sidecarProvider.shell.test.ts`, `src/agent/approvalCards.shell.test.tsx`, `src/shell/terminalAccessDefault.test.ts`, and the new tests below; runtime: `agent-sidecar` `shell_run_tests` |
| Node MCP server (external clients, the Hermes token) | write tools queue for the member (ceremony or approval inbox); Hermes's token is read-only | `src-tauri/src/node_mcp_tests.rs` (writes queue for approval and run nothing) |
| Model delete (#237) | the member, from the Models screen only | `src-tauri/src/model_delete_tests.rs` and the new tests below |

## Added in this re-run (red-green)

`src/shell/agentToolGates.test.ts`, describe "v0.5.0 approval audit: terminal access (#236) and
model delete (#237)" (5 tests):

1. No chat or Hermes tool is named for deleting a model (or anything: no `delete`, `remove`,
   `uninstall` tool names).
2. Every agent tool, invoked with every approval granted (requestSig approved, the wallet review
   approving), never reaches `bridge.modelsCatalog.deleteLocal`; nor do the names `model_delete`
   and `models_delete`.
3. Static: the only sources naming the delete command or its wrappers are the bridge (interface,
   Tauri, preview), the models slice and the Models screen.
4. `shell_run` is not a core-hosted tool (no shell, exec, terminal, command or spawn tool in
   `AGENT_TOOLS`); calling it through `handleTool` asks nothing, resolves nothing and reports no
   command run.
5. A held `shell_run` runs only on an explicit Approve: declined, expired, cancelled, empty and a
   wrong-case `APPROVED` all return false (fail closed), each after exactly one card with the HIC
   reason.

`src-tauri/src/model_delete_tests.rs` `no_agent_tool_table_offers_a_model_delete`: the node MCP
tool table itself (`node_mcp_tools::TOOLS`, by name, which the existing text check did not read),
plus `node_mcp_hermes.rs`, `hermes_mcp.rs` and `hermes.rs`.

**The tests bite** (each mutant applied alone, then reverted):

| Mutant | Killed by |
|---|---|
| a chat tool renamed `model_delete` | tests 1 and 3 (and the existing tripwire) |
| `handleTool` calls `deleteLocal` for an existing read tool | tests 2 and 3 |
| `approveShellRun` returns `r !== "declined"` (fail open) | test 5 only; the existing `shellRunApproval` test (approved / declined) did not catch it |
| a `shell_exec` chat tool added | test 4 (and the existing tripwire) |
| a node MCP tool renamed `pins_remove` | `no_agent_tool_table_offers_a_model_delete` only; the existing text check did not catch it |

## Results on this head

- `npx vitest run`: 2,444 passed, 24 skipped (265 files); the approval-audit files alone: 68 passed.
- `cargo +1.98.1 test --workspace --locked`: 2,343 passed, 0 failed, 19 ignored on the branch at 144b4dc, and 2,355 passed, 0 failed, 19 ignored after merging #259 and #257 (f260f49); both
  model-delete agent-tool tests pass).
- `cargo +1.98.1 fmt --all -- --check`, `cargo +1.98.1 clippy --workspace --all-targets --locked
  -- -D warnings`, `npm run typecheck`: clean.
- Runtime (the `shell_run` hold itself), citrate-agent-runtime `main` @ 397a6b1, the revision the
  0.5.0 Hermes is built from until the pending runtime change lands (checklist A6, A7):
  `cargo test --locked -p agent-sidecar --lib shell_run`: 26 passed (off unless the env flag is
  exactly `1`; a call waits for the member and shows exactly what will run; declined never runs;
  the decision must carry the argv and cwd that were shown; unanswered expires as declined; stop
  declines a waiting command; outside the grants, tainted sessions and no sandbox are refused
  before asking; the approved run is sandboxed; sessions without grants are not offered the tool;
  a session tool cannot claim the name). Whole `agent-sidecar` lib: 462 passed, 3 ignored.
  `citrate-agent-loop` `taint_tests`: 14 passed. Re-run both on the runtime commit the 0.5.0
  Hermes is finally built from (A7).

## Follow-up (not a gate item)

`handleTool` answers a tool name core does not host with a bare "ok". Nothing runs and nothing is
approved, but the model is told the call succeeded. A clear "not a tool here" reply is a small
honesty fix for 0.5.1; test 4 pins only the safety half today.
