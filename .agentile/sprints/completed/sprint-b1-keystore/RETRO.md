---
created: 2026-07-13
branch: docs/skill-artifacts
author: Claude Fable 5, directed by @SaulBuilds
status: archived
sprint: CORE-B1 / Phase B (custody + signing spine)
closing_commit: dabcb3d (B1.5 merged)
purpose: Sprint retrospective for Phase B (agentile:retro skill). Backfilled at skill adoption; the honest accounting the next sprint's plan calibrates against.
---

# Retro — Phase B (custody + signing spine)

## Outcome
| | |
|---|---|
| Goal achieved? | **YES** — the full custody/auth/signing spine, independently signed off (citrate-security #24, SIGNED OFF WITH CONDITIONS, all non-blocking). |
| WPs planned / closed | B1.0, B1.1.0, B1.1-F-1, B1.1, B1.2, B1.3, B1.4.0, B1.4, B1.5 — all closed. Two upstream (B1.1.0, B1.4.0, B1.1-F-1) in citrate-chain. |
| Closing commit | `dabcb3d` (B1.5). Live proof: 40204 tx `0xd40ce789…` @ block 380644 (confirmed). |

## Metrics delta (four ratchet axes)
| Axis | Start (A3) | End (B1.5) |
|---|---|---|
| Rust tests | 73 | **141** (+68) |
| Frontend tests | ~25 | **51** |
| Formal specs | 0 | 0 (opened later, C1.0b) |
| Frontmatter coverage | 100% | 100% |
No axis decreased.

## What worked (concrete + causal)
- **Proving the risky layer cheaply before building on it, then breaking it on purpose.**
  Every WP: build-and-stop → independent reviewer → negative control on each guard. This is
  the only thing that caught the real bugs.
- **Grounding in the real system before scoping** (the A3 lesson carried in): reading the
  actual wallet-core crate surfaced two forks (KeyManager-is-a-2nd-keystore; secp256k1
  mnemonic-recovery was deleted) BEFORE building — not after, the way A3's authority
  mismatch was found after.
- **Upstream fixes stayed upstream** (BIP44 derivation, lean crypto feature, EIP-155 signer)
  — kept the shared crate the single source of crypto truth and the drift map honest.

## What didn't work (named, with cost)
- **Fixes that looked closed but weren't.** B1.0's first anchor fix left F-1b (a legacy
  envelope re-opened the rollback with NO master key) — cost an extra build+review round.
  The lesson: a fix is not the closure; the independent re-attack is.
- **Build agents overstepping into self-review** (recurring early; briefed out with
  build-and-stop). And the CI/infra tax: a private-repo dep needed a token + SSO auth + a
  workflow bug fix (secrets-in-`if`) — three sequential failures, each honest, ~an afternoon.
- **Disk pressure** silently degraded a review agent (forced debug builds) until noticed;
  freed 35G.

## What surprised us (highest-value)
- **A zero-knowledge proving stack walked into the wallet key path as a transitive dep**
  (wallet-core `native` feature → citrate-execution → ark-groth16 → a fresh advisory). Nobody
  *decided* to put a zk prover next to the private key; it arrived invisibly. Only `cargo
  tree` showed it. The dependency graph is part of the trusted surface.
- **The EIP-155 signer carried a latent canonical-RLP bug** (~1/256 sigs geth/ethers reject)
  that no existing test caught — surfaced only by the builder reading the old code.

## Carry-forward (destination + reason)
- **C-1 zeroize physical byte-wipe** (ADV-9 proven to teardown-contract only) → S7 hardening
  (zeroize-audit MIR/LLVM pass); deferred because it needs a compiler-level analysis, not a test.
- **C-2 ADV-matrix row-family enumeration for the Tier-1 audit** → S7.5 audit-prep.
- **C-3 audit-warning baseline (18→19)** → tracked in coverage notes.
- **ChatGPT cross-model quorum leg** (all Rule-8 reviews `Reconciliation: OPEN`) → owner, standing.
- **A3-AUTH-DERIV (LOW), CSP runtime smoke, DUAL-CONTROL 2nd KYC admin** → deploy-time / owner.

## Decisions ratified mid-sprint (should become ADRs)
D-B1-1 (land F-1 hardening before a wallet key), D-B1.1-1 (A2 vault sole custody root;
wallet-core as crypto lib), D-B1.1-2 (re-add BIP44 secp256k1 upstream), D-B1.4 option A
(lean EIP-155 upstream), merge cadence (agent may land independently-reviewed-CLEAR Rule-8
code). **Action: write these as ADRs in `.agentile/decisions/` (document-pass) — not yet done.**

## Action items (owner)
- [ ] Write the D-B1-* ADRs — Claude (next document-pass).
- [ ] ChatGPT cross-model quorum on the Rule-8 reviews — owner.
- [ ] Provision a 2nd KYC admin sub (DUAL-CONTROL) — owner.

## Notes
- Velocity: ~10 sub-WPs across the sprint; the upstream round-trips (B1.1.0, B1.4.0, F-1)
  each added a build+review+merge+re-pin cycle — budget for that on any cross-repo crypto dep.
- Journals/essays produced: field-logs `the-hole-the-fix-left-open`, `the-key-goes-in`,
  `phase-b-signed-off` (ch08-honesty-machine). Essay trigger: **considered** — the
  lean-dependency + honesty-machine arc may warrant an essay at Phase-C or beta close; none
  written yet. Case-study trigger: considered, none warranted (no single incident of that scale).
- Convention note: this repo keeps completed sprints flat under `completed/` (not the skill's
  `completed/<YYYY-MM>/`); followed the existing repo convention (truth hierarchy: repo > skill).
