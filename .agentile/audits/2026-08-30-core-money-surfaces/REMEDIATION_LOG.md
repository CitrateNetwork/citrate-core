---
created: 2026-08-30T00:00:00Z
branch: audit/2026-08-30-core-money-surfaces
author: Claude Fable 5
status: active
audit_id: 2026-08-30-core-money-surfaces
---

# Remediation log — citrate-core @rule8 / gateSec (Milestone B)

Each finding → disposition. No CRITICAL/HIGH; the register is pre-ship gates + INFO/LOW. Nothing here
blocks current development — each gate blocks the specific feature it names from shipping.

| ID | Sev | Status | Action / owner |
|---|---|---|---|
| CORE-G1 | MEDIUM (gate) | **Open — pre-WO-2** | Document the updater **minisign private-key custody** (offline/HSM, NOT on CI) + notarization identity; rotate `TREASURY_SIGNER_TOKEN`. Owner, before the signed build ships. Verification itself is already enforced in code. |
| CORE-G2 | MEDIUM (gate) | **Enforced by CI (tripwire added)** | `SessionBudget` must stay unwired until ADR-2026-08-29 + a dedicated @rule8 review. **Tripwire landed:** `kit/src/ceremony_tests.rs::core_g2_session_budget_is_not_wired_into_the_production_signer` fails if a `.covers(`/`.consume(` call appears in the non-test source of `ceremony.rs` (the signer path — the only place wiring takes effect). Runs under `cargo test --workspace --locked` (the `ci.yml` gate). Verified non-vacuous (a temporary production `.covers(` injection fails it). To lift the gate, wire the budget **and** update this test in the same reviewed change. |
| CORE-L1 | LOW | **Open — pre-confidential-provider** | Land + review the release **keyring-intake WP** for confidential MCP `client_secret`s before any confidential provider is enabled in a shipped build. Confirmed unbundled today (no `bundle.resources`, gitignored). |
| CORE-I1 | INFO | **Open — WO-7 not built** | When the Commissary in-app download leg is built, reuse the `model.rs` sha256 pin + quarantine pattern; the entitlement/signed-URL gate is server-side (core-membership, own audit). |
| CORE-I2 | INFO | **Doc fix** | Scope the sign-off invariant to *wallet/EVM + fund-moving* (the ed25519 proposer PoP + comms/cluster MLS/libp2p identities sign outside the ceremony by design). Documented in REPORT §3; no code change. |
| CORE-I3 | INFO | **Accepted** | Invite tokens are device-local plaintext consent tokens (not keys/credentials) — low sensitivity, accepted. |

## Path to discharging the citrate-core @rule8 leg
The in-repo code is CLEAN (no CRITICAL/HIGH; ceremony invariant verified + physically enforced). The
leg's opinion is PROVISIONAL-PENDING-GATES. To flip it: discharge CORE-G1/G2/L1 as their features
approach shipping, keep training write-paths code-gated (already enforced), then owner + security lead
sign `12_OPINION.md` alongside the settlement leg, and add the federation manifest pin. This leg +
`citrate-settlement/.agentile/audits/2026-08-30-setl-money-surface/` = the combined @rule8 / gateSec
sign-off that unblocks real member SALT.
