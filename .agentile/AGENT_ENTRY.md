---
created: 2026-07-11
branch: feat/core-s0-scaffold
author: Claude Fable 5 (CORE-S0 scaffold agent), directed by @SaulBuilds
status: active
repo: citrate-core
tier: T1
---

# Agent Entry — citrate-core

> **Lightweight subset.** This file points back to the canonical Agentile
> framework lineage. Start here whenever you (human or AI) are working in
> **citrate-core**.

## What this repo is

The federation's desktop house: a **Tauri 2.x full-node application** that takes
a new user through one auth flow (KYC + yearly membership) to a funded smart
wallet, a staked full node earning SALT, and tier-gated access to every Citrate
application, SDK, doc, and service. Full-node counterpart to citrate-native
(light node, decision D-7).

Repo tier: **T1** — money, keys, identity, staking, binary distribution. Full
audit before public release.

## What to read, in order

1. **This file** (you're here).
2. **README.md** — honest current status.
3. **The federation planset (canonical truth)** —
   `citrate-federation/.agentile/planset/2026-07-11-citrate-core/`:
   - `00_OVERVIEW.md` — vision, locked decisions D-1…D-14, capability index, reuse map
   - `01_PRODUCT_SPEC.md` — personas, onboarding narrative, page-by-page spec
   - `02_ARCHITECTURE.md` — process topology, sidecars, auth/entitlement, data flows
   - `03_FEATURES_BDD.md` — Gherkin features for every v1 capability
   - `04_DESIGNER_BRIEF.md` — designer round-trip brief (D-9)
   - `05_SPRINTS_AND_WPS.md` — sprint plan CORE-S0…S8 with Rule-11 acceptance
   - `06_SECURITY_AND_COMPLIANCE.md` — threat model, Rule-8 gates
4. **Upstream gaps planset** —
   `citrate-federation/.agentile/planset/2026-07-11-core-upstream-gaps/` (E-1…E-9).
5. **Federation control plane** — `citrate-federation/agentile/AGENT_ENTRY.md`
   and `rules/CORE_RULES.md` (non-negotiables).
6. **CLAUDE.md** (repo root) — this repo's hard rules.

## Core invariants (from planset 00)

- **I-1 (entitlement):** no paid-tier access without a live, server-verified entitlement.
- **I-2 (custody):** citrate-core owns the keystore and every signature; no sidecar or remote service ever holds a user key.
- **I-3 (honesty, Rule 1):** every surface states what is real; unbuilt backing services ship named honestly or not at all.

## What lives here, locally

| Path | Purpose |
|---|---|
| `.agentile/AGENT_ENTRY.md` | This file — entry point. |
| `.agentile/sprints/` | Repo-scoped sprints (`active/`, `backlog/`, `completed/YYYY-MM/`). |
| `.agentile/planset/README.md` | Pointer to the federation planset (link, don't copy — Rule 9). |
| `src-tauri/` | Rust workspace member: the Tauri 2.x shell. |
| `src/` | React + TypeScript + Vite frontend (wagmi + viem, D-13). |
| `services/membership/` | Reserved for the core-membership Next.js app (CORE-S5). Not started. |
