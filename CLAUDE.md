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
3. **All signatures via SignatureCeremony** once it exists (CORE-S2, I-2).
   Until then: no signing code paths at all. No sidecar, daemon, or remote
   service ever holds a user key.
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
