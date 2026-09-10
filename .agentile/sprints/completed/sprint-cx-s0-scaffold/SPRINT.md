---
created: 2026-08-26T00:00:00Z
branch: cx/s0-scaffold
author: Larry Klosowski (@SaulBuilds) + Claude Opus 4.8
status: archived
sprint: CX-S0
planset: citrate-core-social (citrate-federation/.agentile/planset/2026-08-26-citrate-core-social/)
tier: T1
---

# Sprint CX-S0 — Foundation & Scaffolding

## Goal

Freeze the shared spine ONCE so every later CX feature WP (S1–S6) edits only files it
exclusively owns — the race-free execution model in 01_SCOPE §4 and 02_ARCHITECTURE §3.
This sprint is **reversible groundwork** (no money, no keys, no chain writes); it is the only
sprint permitted to touch the shared-spine files. After it merges, the spine changes only via a
serialized spine-PR + ADR (01 §5.2).

**Definition of done (gate g0-scaffold):** `npm run build` + `cargo build` green; every new
CX domain method returns an honest `Unavailable`/`NotWired`; every new surface renders an honest
empty state; `scripts/cx-ownership-check.sh` self-test passes; contract tests pin every frozen
interface; baseline recorded; `[[drift]]` pins declared.

## Baseline snapshot (Rule 2 — must not decrease)

| Axis | Count @ 2026-08-26 (734d9f1) |
|---|---|
| src-tauri lib tests | 285 |
| kit crate test attrs | 192 |
| frontend test blocks | 312 (30 files) |
| new CX contract tests (this sprint adds) | +N (S0.2) |
| .agentile docs (frontmatter) | 36 |

CI in this repo is light (no `local-ci.sh`); CX-S0.6 adds `cx-ownership-check.sh` +
`cx-copy-lint.sh` as the CX gate scripts.

## Work packages

Status: `[ ]` not started · `[~]` in progress · `[x]` complete · `[!]` blocked.
All WPs on THIS branch (`cx/s0-scaffold`) — CX-S0 is one serialized merge (01 §4.3).

- [x] **S0.6** Gate scripts + drift pins + baseline. `scripts/cx-ownership-check.sh` (asserts a
      branch diff ⊆ its declared owned-file set), `scripts/cx-copy-lint.sh` (forbidden
      reward/reach phrasings), `.agentile/planset` drift pins for comms-*/nat-*/fed-types/
      compute-pool, this baseline block. **Do first — everything references it.** Effort M.
      **DONE 2026-08-26 (892da93):** both scripts + `.agentile/cx-ownership.map`; both
      `--selftest` green (ownership-check correctly fails cross-lane + spine files for feature
      lanes; copy-lint catches forbidden reward/reach/scale phrasings). `s0` ownership-check
      green on the branch. Drift-pin manifest edge deferred to the first real dep (S3.1) —
      S0 scaffolding uses stubs with no cross-repo edges yet.
- [x] **S0.1** Composable bridge + sliced store. **DONE 2026-08-26 (4728369 + ad89c76).**
      Right-sized to establish the two seams WITHOUT touching the existing monolith (lower risk
      than a full split): (a) store seam — `src/shell/slices/createSlice.ts` (external-store
      factory, 5 tests) so each CX feature owns its own slice, class `Store` untouched;
      (b) bridge seam — `bridge/cx.ts` composes CX domains onto `bridge: BridgeContract &
      CxBridge`, so a lane fills its own `bridge/{tauri,sim}/<domain>.ts`. Full suite 333 pass,
      store.test 86 pass (no regression), tsc clean.
- [x] **S0.2** Freeze all domain interfaces + DTOs + contract tests. **DONE (ad89c76 + 8d9d400).**
      All 6 CX domains (modelsCatalog, storage, groups, cluster, training, agentHarness) declared
      in `domains.ts` as `CxBridge`, composed via `bridge/cx.ts` onto `bridge: BridgeContract &
      CxBridge`; 10 lane-owned tauri/sim stub files; 6 contract tests pin the frozen shapes +
      sim honesty. Full suite 344, tsc clean.
- [x] **S0.3** Register all Rust modules + command names. **DONE (b0a6de7).** 6 host modules
      (model_catalog, storage, comms, cluster, training, hermes) with 33 NotWired command stubs;
      all names registered once in `lib.rs` generate_handler!. `cargo check` clean (4s).
- [x] **S0.4** All surface shells + nav + router. **DONE (3fd13ce).** 6 honest-empty surface
      shells + shared `CxScaffold`; registered once in surfaces/index.ts + App.tsx (import/switch/
      REGISTER/hash-whitelist) + Sidebar NAVI. Copy compliance-clean (cx-copy-lint green).
      `npm run build` green.
- [ ] **S0.3** Register all Rust modules + command names. Create `src-tauri/src/<feature>.rs`
      (models catalog additions, storage, comms, cluster, training, hermes, agent_tools) with
      `#[tauri::command]` stubs returning `NotWired`; list ALL command names in `lib.rs`
      `generate_handler!` ONCE. Effort M.
- [ ] **S0.4** All surface shells + nav + router. Create `src/surfaces/<Feature>.tsx` honest
      empty shells (Models, StorageFiles, Groups, Cluster, Train, Agent); register nav
      (`Sidebar.tsx`), router (`App.tsx`), exports (`surfaces/index.ts`). Effort M.
- [x] **S0.5** Bundle `mem-mcp`; declare `comms-relay`/`hermes` sidecars. **DONE 2026-08-26
      (9d2f6c1):** `mem-mcp` was ALREADY bundled in both overlays (FEATURE_MAP note stale —
      the memory-graph gap is the ingest path + auto-start, not bundling). Added
      `binaries/comms-relay` + `binaries/hermes` to both overlays; README documents them +
      the packaging-gated-on-binaries caveat. JSON valid; s0 ownership-check green.

## Method notes

- No state machines in CX-S0 → no TLA+. BDD: the "scaffold renders honest empty states"
  scenarios (04 C-18/C-23 honesty lines). Failing-first: contract tests (S0.2) + the
  ownership-check self-test (S0.6) are written red, then made green by the scaffold.
- Rule 4: commit with explicit paths only; owner merges the PR; never commit to main.
- Rule 8: no `.unwrap()` in new host/UI paths (stubs use `?`/honest errors).
- Rule 1: every stub states it is a stub (`Unavailable`/`NotWired`), never fakes success.

## Daily updates
- 2026-08-26 — kickoff; branch `cx/s0-scaffold` off `origin/main@734d9f1`; baseline snapped
  (285 / 192 / 312); starting S0.6.
- 2026-08-26 — S0.6 done (892da93): gate scripts + ownership map, both --selftest green.
  S0.5 done (9d2f6c1): sidecar spine declared; mem-mcp already bundled (plan corrected).
- 2026-08-26 — S0.1 done (4728369): createSlice store seam (5 tests). S0.2 done (ad89c76 +
  8d9d400): bridge composition seam + all 6 CX domains + 14 contract tests. S0.3 done (b0a6de7):
  6 Rust modules + 33 command stubs, cargo-check clean. S0.4 done (3fd13ce): 6 surface shells +
  nav/router, npm run build green. **CX-S0 COMPLETE — gate g0-scaffold met (pending owner PR merge).**
  Frontend 312→344; ownership s0 green on all 8 commits. Ready to open the S0 PR; then Lanes A/B/C/D.

## Exit criteria (gate g0-scaffold)
1. [x] `cargo check` + `npm run build` green with all CX stubs honest (Rule 1).
2. [x] Contract tests green (14 across 6 domains); interfaces frozen.
3. [x] `cx-ownership-check.sh` + `cx-copy-lint.sh` self-tests green; ownership s0 green on every commit.
4. [x] mem-mcp bundled (already was); comms-relay/hermes declared in overlays.
5. [x] Test counts ≥ baseline: frontend 344 (was 312, +32), lib 285 unchanged. Rule 2 held.
6. [~] `[[drift]]` pins — deferred to the first REAL cross-repo dep (CX-S3.1); S0 uses stubs
       with no cross-repo edges, so no pin is due yet.
7. [ ] PR opened; close-with-proof written; **owner merges** (Rule 4/10).

## Close-with-proof

CX-S0 delivered the parallel-safe scaffold (planset 01 §4-§5, 02 §3) as ONE serialized branch
`cx/s0-scaffold` (8 commits). **Real:** the ownership guard + copy-lint (self-tested), the store
slice seam (`createSlice`, 5 tests), the bridge composition seam (`BridgeContract & CxBridge`, 6
domains, 14 contract tests), the Rust command registry (33 stubs, cargo-check clean), 6 surface
shells (build green). **Scaffold (honest, by design):** every CX domain/command/surface returns
`Unavailable`/`NotWired`/empty — nothing is wired to real behavior; that is each feature lane's
job. **Unproven:** nothing claimed beyond "the seams compile, are frozen, and are guarded."
After merge, lanes A/B/C/D open in parallel; each edits only its owned files (guard-enforced).
