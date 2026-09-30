---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: active
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core
companions: 05_SPRINTS_AND_WPS.md, gates.yaml
---

# Release plan: 0.4.0 → 0.5.0

The Hermes Upskill program is managed as the full **0.4.x → 0.5.0** release line
(owner, 2026-09-30). `main` is at **0.4.0** (`28a74ba`, the fresh-keys reroll build).
**0.5.0 ships only when every gate in [gates.yaml](gates.yaml) is `met` with evidence**
(D-39: scoped by work, not time).

## Branch model

```
main (0.4.0) ─────────────────────────────────────────────────────► main (0.5.0)
   └── release/0.5.0-hermes-upskill   (integration branch; planset lives here)
          ├── hup/s0-stabilize        ─PR→ release/0.5.0-hermes-upskill  → tag v0.4.1
          ├── hup/s1-one-agent        ─PR→ …                              → tag v0.4.2
          └── …
```

- **Integration branch:** `release/0.5.0-hermes-upskill`. Sprint branches are
  `hup/s<N>-<slug>`; each WP lands as a PR into the integration branch. The owner merges
  (CLAUDE.md rule 4).
- **Point releases:** at each milestone, the integration branch is PR'd to `main`, and
  the owner merges and runs the signed-release ceremony (macOS notarize, GitHub release,
  DO mirror, parity gate). Tags are cut from `main` only.
- **Sync:** `main` is merged into the integration branch after every release or hotfix,
  never rebased (the branch is shared).
- **Federation work** (chain, runtime, cluster, memories, pool, settlement) follows
  its own repo branches, tracked in the private federation sprint. Its pins bump through
  `manifest.toml` + `pin-bump.sh`.
- **Version bump points:** `package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json` (+ bundle configs), bumped in the milestone PR.

## Milestones

| Version | Sprints | Gate(s) | What a user gets |
|---|---|---|---|
| **0.4.1** | HUP-S0 Stabilize | gate1: g1-approval-audit, g1-no-block, g1-downloads, g1-render, g1-sidebar | No pinwheel; downloads finish + resume; clean markdown + real streaming; ~10-item sidebar |
| **0.4.2** | HUP-S1 One agent | gate1 rest; gate0 formal (small bounds) | One Hermes across app/CLI/MCP; interview → brief; verifier-decided outcomes; tiered model; per-tier scorecard |
| **0.4.3** | HUP-S2 HIC + HUP-S3 Knowledge | gate0 rule3-adr; gate2 hic/knowledge/personas | Folder grants, sandboxed shell, SIWE budgets (after the ADR), undo; bundled knowledge graph; personas + tracks; verified self-learning |
| **0.4.4** | HUP-S4 MCP + HUP-S5 Web | gate2 mcp; gate3 updater/licence/browser | MCP host + node MCP server; managed browser + pop-outs; private search; decide() slot; signed component updater |
| **0.4.5** | HUP-S6 dApp forge | gate3 gate/e2e | **hello mint** on a T1 machine: interview → build → audit gate → faucet → ceremony deploy → verify → IPFS. *Devrel demo release.* |
| **0.4.6** | HUP-S7 Chain-native + HUP-S8 Fleet | gate4 identity/anchor/precompiles/fleet | AgentSBT, nightly anchor, metering + activity monitor; fleet wizard, per-device keys, mesh on by default |
| **0.4.7** | HUP-S9 Learn together + HUP-S10 Everyday | gate4 fl; gate5 everyday | Live paraconsensus FL round incl. the hard fork; Hermes LoRA; media, office, widgets, daemons, journal export |
| **0.5.0** | HUP-S11 Prove it | gate5 size/os/redteam/docs, **all gates met** | hello mint e2e on macOS/Linux/Windows clean installs; eval CI; red-team clean; docs |

Milestones can merge (e.g. 0.4.3 split into two point releases) without renumbering
0.5.0. A milestone never ships with an open High from the red-team register.

## Release hygiene per milestone

1. Gate criteria for the milestone are `met` with evidence paths.
2. `cargo test --workspace --locked` and `vitest` counts are recorded in the sprint
   EVIDENCE (Rule 2: monotone).
3. The tripwire, clippy `-D warnings`, the zero-`unwrap` check (Rule 8) and fmt are green.
4. Journal entry + retro for each closed sprint (agentile cadence); an essay attempted
   per milestone.
5. @rule8 items (updater keys, anchor key custody, budgeted-signature ADR) have a
   recorded security sign-off before the release that ships them.
