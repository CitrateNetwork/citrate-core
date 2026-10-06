---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: active
updated: 2026-10-05 (owner decision: D-41 amended again, one v0.5.1 for the SCL v0.5.1 gate; third set: one v0.5.1 and one v0.5.2)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core
companions: 05_SPRINTS_AND_WPS.md, gates.yaml
---

# Release plan: 0.4.0 → 0.5.0

> **Superseded in part (D-41, owner, 2026-10-01): one release, v0.5.0.** There are no point
> releases between 0.4.1 and 0.5.0. Every user story is finished, the integration branch is
> tested and QA'd end to end, and it ships once as **v0.5.0**. The 0.4.2–0.4.7 rows below are
> kept as **internal milestones** (planning and burndown groupings only): no version bump, no
> tag, no signed-release ceremony, no updater manifest, and no mirror run at those points.
> 0.4.1 (already shipped) stays the last public release until 0.5.0. Internal QA builds of the
> integration branch may be made for testing; they are not published.
>
> **Amendment (owner, 2026-10-01): one interim release, 0.4.2.** Cut from
> `release/0.5.0-hermes-upskill` once the downloaded-model fix (#133) and the dark-register text
> fix (#134) landed, with the Hermes sidecar rebuilt from runtime `main` (`3d75efb`). Gate: a
> hands-on QA pass on a signed, notarized Mac build before publishing; then the DGX team builds
> Linux and mirrors. The sidecar-loop preview stays off by default in 0.4.2. After 0.4.2, D-41
> holds again: the next release is 0.5.0.
>
> **Owner decision (2026-10-05): D-41 amended again, one v0.5.1.** Exactly one **v0.5.1** is
> allowed after v0.5.0. It carries the SCL (owned sidecar lifecycle) v0.5.1 release gate: the
> criteria with `release: v0.5.1` in the
> [SCL gates.yaml](../2026-10-05-sidecar-lifecycle/gates.yaml). v0.5.0 needs only the SCL
> criteria with `release: v0.5.0` (`g5-scl`). Same signed-release ceremony as v0.5.0, tagged from `main`.
> Nothing else changes in this plan.
>
> **Owner decision (2026-10-05, third set): D-41 amended again, one v0.5.1 and one v0.5.2.**
> Exactly one **v0.5.1** and exactly one **v0.5.2** are allowed after v0.5.0. v0.5.1 carries the
> SCL criteria with `release: v0.5.1`; v0.5.2 carries those with `release: v0.5.2` (the former
> v0.5.x remainder). v0.5.0 is the minimum viable SCL content (SCL-S0, S1.6a, S8.5a); S7.5a moved
> to v0.5.1. Same signed-release ceremony, tagged from `main`. Nothing else changes in this plan.

The Hermes Upskill program is managed as the full **0.4.x → 0.5.0** release line
(owner, 2026-09-30). `main` is at **0.4.0** (`28a74ba`, the fresh-keys reroll build).
**0.5.0 ships only when every gate in [gates.yaml](gates.yaml) is `met` with evidence**
(D-39: scoped by work, not time).

## Branch model

```
main (0.4.0) ─────────────────────────────────────────────────────► main (0.5.0)
   └── release/0.5.0-hermes-upskill   (integration branch; planset lives here)
          ├── hup/s0-stabilize        ─PR→ release/0.5.0-hermes-upskill  → tag v0.4.1 (shipped)
          ├── hup/s1-one-agent        ─PR→ …                              (milestone only, no tag)
          └── …                                                           → one tag: v0.5.0
```

- **Integration branch:** `release/0.5.0-hermes-upskill`. Sprint branches are
  `hup/s<N>-<slug>`; each WP lands as a PR into the integration branch. The owner merges
  (CLAUDE.md rule 4).
- **One release (D-41):** the integration branch is PR'd to `main` once, when every gate is
  met and QA is done; the owner merges and runs the signed-release ceremony (macOS notarize,
  GitHub release, DO mirror, parity gate) for **v0.5.0** only. Tags are cut from `main` only.
  (Superseded: point releases at each milestone.)
- **Sync:** `main` is merged into the integration branch after every release or hotfix,
  never rebased (the branch is shared).
- **Federation work** (chain, runtime, cluster, memories, pool, settlement) follows
  its own repo branches, tracked in the private federation sprint. Its pins bump through
  `manifest.toml` + `pin-bump.sh`.
- **Version bump points:** `package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json` (+ bundle configs), bumped once, 0.4.1 → 0.5.0, in the release PR.

## Milestones

| Milestone (internal since D-41; only 0.4.1 and 0.5.0 ship) | Sprints | Gate(s) | What it adds |
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

## Release hygiene (milestones: items 1-4; the v0.5.0 release: all, plus a full QA pass)

1. Gate criteria for the milestone are `met` with evidence paths.
2. `cargo test --workspace --locked` and `vitest` counts are recorded in the sprint
   EVIDENCE (Rule 2: monotone).
3. The tripwire, clippy `-D warnings`, the zero-`unwrap` check (Rule 8) and fmt are green.
4. Journal entry + retro for each closed sprint (agentile cadence); an essay attempted
   per milestone.
5. @rule8 items (updater keys, anchor key custody, budgeted-signature ADR) have a
   recorded security sign-off before the release that ships them.
