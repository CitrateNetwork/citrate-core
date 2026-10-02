---
created: 2026-10-01T10:05:00Z
branch: hup/n4-siwe-budgets, hup/n5-siwe-rest
author: Larry Klosowski + Claude Opus 5.5
status: implemented and wired in the managed browser (off by default with the browser); packaged-app run outstanding
updated: 2026-10-01T20:00:00Z
wp: HUP-S2.3
sprint_issue: CitrateNetwork/citrate-federation#279
---

# WP HUP-S2.3: SIWE web-signing budgets

Design, honest state, owner placeholders and the invariant-to-test map: `docs/WEB_SIGNING_BUDGETS.md`.

## Acceptance criteria (US-2.3)

| AC | State |
|---|---|
| AC1 only EIP-4361 is budgetable; tx, eth_sign, permits always HIC-1 | Met by type (message-only request, `sign_personal` only) and by tests |
| AC2 domain equals page origin; per-origin count + expiry budget | Met. Core attests the origin with its own read of the managed browser's DevTools list and requires it to equal the asking context's origin (n5). Frame provenance comes from Chrome's context events via the sidecar (residual, documented) |
| AC3 every signature logged (origin, statement, nonce, time) and in the nightly anchor | Met up to the anchor transaction: every closed record is copied into the decision log the nightly anchor batches (n5). The anchor transaction itself is S7.3 / O-5 |
| AC4 one-click revoke in Settings → Budgets; TLA+ green | Met: Settings → Budgets and the "Signed for you" notice (n5). TLC re-run on all five configs (n5) |

## Red-green log

- `siwe` tests: 18 written first, 18 failing against an empty skeleton, then green.
- `web_budget` (23), `ceremony_budget` (12), `web_budgets` app (8): written before their modules; red as compile failures, then green.
- vitest `src/budgets` (12): red (missing modules), then green.

## n5 (hup/n5-siwe-rest, core + runtime)

- Live path in the managed browser: page provider and bridge (runtime), request read from the
  sidecar by id, origin attested by core, decision through the ceremony, delivery to the page.
- The webview supplies only a request id; taint is computed by the sidecar from every live session
  (every source kept, not only the first). Closes the caller-supplied taint gap from the review.
- "Signed for you" notice with "Revoke this budget"; sign-in cards at the Signature Ceremony.
- Records copied into the anchored decision log, with an export cursor in the MACed file.
- Rollback protection for the budget file (generation sealed in the keychain).
- Ceremony tripwires now scan the whole production module.

## Not done

- Packaged-app run with a real https dApp (an automatic sign-in end to end); Linux and Windows.
- Core-held DevTools session on the frame tree (frame provenance stays sidecar-reported).
- x402 B-2 enablement (S1.5, after the O-1 asset exists).
- Native-window grant card: the grant is a member click in Settings → Budgets (the app's own UI,
  not web content).
- The anchor transaction on 40204 (S7.3, O-5, AnchorRegistry address).
