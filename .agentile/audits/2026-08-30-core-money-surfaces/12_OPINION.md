---
created: 2026-08-30T00:00:00Z
branch: audit/2026-08-30-core-money-surfaces
author: Claude Fable 5 (drafting) — signers below
status: active
audit_id: 2026-08-30-core-money-surfaces
---

# citrate-core @rule8 / gateSec (Milestone B) — Opinion

Companion to `REPORT.md`. The citrate-core leg of the @rule8 money-surface sign-off, reconciling into
the central gateSec / SETL-S4 trail in citrate-security.

## 1. Independence grade

**`SELF`** — single model (Claude, in-org), with an internal two-pass split: a primary surface map and
an independent adversarial pass instructed to *refute* the ceremony no-bypass invariant. Every
load-bearing claim (ceremony reachability, `SessionBudget` dead-code, updater verification, oauth
bundling, model quarantine, staking drift tests) was re-verified against source. Code under audit was
written in-org; no cross-model-blind or external-party leg was run (recorded pending in §6).

## 2. Graded conclusion

**`CLEAN` for the in-repo code; `PROVISIONAL-PENDING-GATES` for the @rule8 leg.**

All five surfaces reviewed are clean: no CRITICAL/HIGH, the Rule-3 ceremony invariant holds and is
*physically* enforced by the `kit`/`src-tauri` crate boundary, no ungated money write, no key over the
invoke boundary. The reason the leg is not simply CLEAN is that the @rule8 items are **pre-ship process
gates** (updater key custody, `SessionBudget` staying unwired, the confidential-OAuth release path,
Commissary discipline-when-built) that are not yet discharged, and some gated features are unshipped.
That is the honest default until the gates run — the code is sound; the ship conditions are open.

## 3. Authorization-style recommendation

**Authorize continued development** of citrate-core on all five surfaces — the architecture is correct
and safe as-is. **Do NOT ship** the signed/notarized build + updater (WO-2), the in-app Commissary
downloads (WO-7), or any confidential MCP provider until their specific gate conditions (§6 of the
report / §6 below) are discharged and re-reviewed. Real member SALT stays blocked until the combined
gateSec (this leg + the settlement leg) flips — which is already enforced in code by the training
write-path refusals.

## 4. Findings summary

| Severity | Count | IDs |
|---|---|---|
| CRITICAL | 0 | — |
| HIGH | 0 | — |
| MEDIUM (pre-ship gate) | 2 | CORE-G1 (updater key custody), CORE-G2 (SessionBudget unwired) |
| LOW | 1 | CORE-L1 (confidential OAuth release path) |
| INFO | 3 | CORE-I1 (Commissary unbuilt), CORE-I2 (signing-scope wording), CORE-I3 (invite tokens) |

Top by CIT-SEV: CORE-G1 `sev=M; diff=H; asset=infra; impact=rce; reach=privileged; conf=firm`
(CRITICAL stakes if the updater private key leaks — hence a hard pre-ship gate).

## 5. Coverage statement

Covered: the five @rule8 surfaces (treasury custody, updater, gated downloads, stake-lock, credentials)
+ the SignatureCeremony core, to source-read depth, with an adversarial refutation pass on the ceremony
invariant. **Not covered:** the out-of-repo server-side pieces (the droplet treasury signer, the
core-membership signed-URL/entitlement gate) — audited in their own repos; the settlement signer
custody (settlement leg); a running dynamic/build-time test of the WO-2 updater against a live
malicious feed (infra not built — waived at BC-8); the blind cross-model and external-party legs.
Residual risk is bounded by keeping the gated features unshipped until their conditions are met.

## 6. Close-gate ledger

| Gate | State |
|---|---|
| No CRITICAL/HIGH in reviewed code | **discharged** (0/0) |
| Ceremony no-bypass invariant (adversarial) | **discharged** (refutation failed; physically enforced) |
| CORE-G1 updater private-key custody documented (off-CI) | pending (pre-WO-2 ship) |
| CORE-G2 SessionBudget unwired + CI tripwire | pending (standing condition) |
| CORE-L1 confidential-OAuth release keyring intake reviewed | pending (pre-confidential-provider ship) |
| Blind cross-model quorum | pending |
| External-party leg | n-a (in-org self-assessment) |
| Findings register machine-readable + counts reconcile | yes (§4 ↔ REPORT §3) |

## 7. Signature

Drafted by: Claude Fable 5 (citrate-core), 2026-08-30 — the analysis and grade above.

This leg is signed off by the two gateSec reviewers together with the settlement leg:

- `________________________`  **Larry Klosowski (owner)** — date: __________
- `________________________`  **Security lead** — date: __________

Immutable once signed (Rule 6); revisions are dual-dated reissuances. As drafted, the conclusion is
**CLEAN code / PROVISIONAL-PENDING-GATES @rule8 leg** — signing endorses continued development and the
pre-ship gate conditions in §6.
