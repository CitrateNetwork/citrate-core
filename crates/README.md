---
created: 2026-11-27
branch: feat/v60-l0-wp10-dep-intake
author: Larry Klosowski (L0 lane, L0-WP10; Claude Opus 5.5)
status: active
---

# crates/

Lane crates for 0.6.0 (for example `crates/citrate-acp`, `crates/citrate-pqc`). The root
`Cargo.toml` takes every directory here as a workspace member (`members = [..., "crates/*"]`), so a
new crate needs no root edit. Cargo skips files in the glob, and this README keeps the glob matching
something while no crate has landed (a glob that matches nothing is read as a literal path).

Each directory here must hold a `Cargo.toml` (`scripts/dep-intake.test.mjs` checks it). Path crates
here are first party and need no dependency intake row; every third-party package one of them pulls
into `Cargo.lock` does, in `release/dep-intake.json`, reviewed with supply-chain-risk-auditor and
landed by L0 in a dependency PR (at most one a day, so `Cargo.lock` changes stay serialized). A
crate from a federation repo also needs its `[[drift]]` entry in `citrate-federation/manifest.toml`
first (Rule 12) and stays on the federation rev.

Ownership: `crates/**` falls to L0 in `.agentile/v60-ownership.map` until a seam PR assigns a crate
to its lane.
