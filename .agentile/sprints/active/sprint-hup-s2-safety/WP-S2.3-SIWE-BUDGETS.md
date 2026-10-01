---
created: 2026-10-01T10:05:00Z
branch: hup/n4-siwe-budgets
author: Larry Klosowski + Claude Opus 5.5
status: implemented, auto-sign inactive until HUP-S5.1 (origin attestation)
wp: HUP-S2.3
sprint_issue: CitrateNetwork/citrate-federation#279
---

# WP HUP-S2.3: SIWE web-signing budgets

Design, honest state, owner placeholders and the invariant-to-test map: `docs/WEB_SIGNING_BUDGETS.md`.

## Acceptance criteria (US-2.3)

| AC | State |
|---|---|
| AC1 only EIP-4361 is budgetable; tx, eth_sign, permits always HIC-1 | Met by type (message-only request, `sign_personal` only) and by tests |
| AC2 domain equals page origin; per-origin count + expiry budget | Met in code and tests. Page origin attestation itself waits on the managed browser (S5.1), so live auto-sign is off and every request is a card |
| AC3 every signature logged (origin, statement, nonce, time) and in the nightly anchor | Logged, hash-chained, head hash exposed. Anchor inclusion is S7.3 wiring |
| AC4 one-click revoke in Settings → Budgets; TLA+ green | Revoke and revoke all in Settings → Budgets. TLC re-run on the O3 and Siwe configs recorded in the issue comment |

## Red-green log

- `siwe` tests: 18 written first, 18 failing against an empty skeleton, then green.
- `web_budget` (23), `ceremony_budget` (12), `web_budgets` app (8): written before their modules; red as compile failures, then green.
- vitest `src/budgets` (12): red (missing modules), then green.

## Not done here

- Origin attestation over CDP (needs S5.1 managed browser).
- A browser tool that raises `web_signing_request` (S5.1).
- Folding the record head hash into the nightly anchor (S7.3).
- x402 B-2 enablement (S1.5, after O-1 asset exists).
- Native-window grant card: the grant is a member click in Settings → Budgets (the app's own UI, not web content).
