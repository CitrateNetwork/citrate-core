---
created: 2026-07-11
branch: feat/core-s0-scaffold
author: Claude Fable 5 (CORE-S0 scaffold agent), directed by @SaulBuilds
status: active
---

# CLAUDE.md — citrate-core hard rules

Start at `.agentile/AGENT_ENTRY.md`. Canonical truth is the federation planset
(`citrate-federation/.agentile/planset/2026-07-11-citrate-core/`).

## Hard rules (non-negotiable)

1. **No mocks (Rule 1).** No mocked data, fake fixtures presented as live, or
   placeholder features that pretend to work. Every surface states what is
   real. Chain reads hit the live 40204 RPC or show an honest error.
2. **Test count monotone (Rule 2).** `cargo test --workspace --locked` count
   never decreases. Record the count when it changes (sprint file).
3. **All signatures via the SignatureCeremony (Rule 3 — LIFTED by CORE-B1.2).**
   The SignatureCeremony now EXISTS (`src-tauri/src/ceremony.rs`): the single
   human-in-the-loop signing path. EVERY signature — from the user, and later
   from any node-agent / chat-agent / micro-app — routes through it. The gated
   signer (`wallet::sign_message`, `pub(crate)`) is reachable ONLY from
   `SignatureCeremony::approve`; **signing anywhere else is forbidden.** No
   `#[tauri::command]` signs or returns key/seed/entropy (I-2 compile barrier);
   the command surface is `sign_request` / `sign_approve` / `sign_reject`, which
   return a ceremony id / decoded intent / signature-hex / status only. Approval
   is bound to an explicit CeremonyId (no auto-approve, no "approve latest");
   undecodable calldata is blocked until an explicit raw-mode ack; one approval
   yields exactly one signature (single-use); a locked vault fails closed. No
   sidecar, daemon, or remote service ever holds a user key or signs directly.
   (The recoverable EIP-155 tx-signing form is deferred to B1.4; B1.2 signs via
   the message path under the lean `crypto` build.)
4. **Never commit to main.** All work on a feature branch + PR via `gh`.
   Commit with explicit paths only (`git add <paths>`, never `-A`/`.`).
   Never merge anything — the owner merges.
5. **Every doc gets YAML frontmatter** (created, branch, author, status).
6. **Link, don't copy (Rule 9).** The planset lives in citrate-federation;
   this repo points at it.
7. **T1 repo.** Money, keys, identity, staking, distribution. @rule8 items
   (treasury custody, updater keys, gated downloads) need security sign-off
   before deploy. Full audit before public release.

End commits with:
`Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
