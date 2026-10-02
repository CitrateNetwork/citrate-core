---
created: 2026-10-01
branch: hup/n5-chain-faucet
author: Larry Klosowski + Claude Opus 5.5
status: implemented (HUP-S6.5); off by default; faucet ADR proposed, O-1 to O-4 pending owner sign-off
---

# The in-app faucet for deploy gas

HUP-S6.5. Design: [ADR-2026-10-01-faucet-for-deploy-gas](adr/ADR-2026-10-01-faucet-for-deploy-gas.md)
(**proposed**). This page says what the build does. Until the owner accepts the ADR, the switch
stays off for every member and nothing here sends a request.

## What the member sees

Settings, Budgets, **Faucet for deploy gas**:

- **Off** (the default): "Core never asks the faucet for you."
- **Turn on** opens a confirmation that lists exactly what is allowed: the member's own wallet,
  only when a deploy they started is short of gas, at most one request every 24 hours, revocable.
  **Allow** is the member's HIC-1 grant of this HIC-2 budget. **Turn off** revokes it at once.
- The faucet's state (up and able to drip, up but unable, unreachable), the next time a top-up
  is possible, and the history of requests (when, who asked, what came of it).
- **Open the faucet page** opens the faucet's own page in an in-app window with the address
  filled in. The member can fund a deploy by hand there, or solve the faucet's CAPTCHA when the
  operator has turned it on.
- In the signing review of a READY deploy: **Short of gas? Ask the faucet**. The answer is shown
  as it is, including when nothing was sent and why.

Hermes (and any node-MCP client) reaches the same request through the `faucet_request` tool,
which takes only the deploy's init code hash.

## What core does (`src-tauri/src/faucet.rs`)

1. Recipient = the member's active wallet, read from custody. No caller can choose it.
2. The deploy gate must hold a READY record for the init code hash (HUP-S6.4). No READY deploy,
   no request.
3. Need = 2,000,000 gas (the limit `contract_deploy` puts on a deploy) × `eth_gasPrice`. A
   balance (`eth_getBalance`) that covers it means no request.
4. One request per 24 hours per wallet, counted from the app's own history (a drip that went
   out, or might have). A refusal from the faucet with a next eligible time blocks until then.
   Nothing retries.
5. One unsigned `POST {faucet}/faucet {"address": <wallet>}`. The faucet signs and pays the drip
   with its own key. Core signs nothing; no sidecar holds a key.
6. The outcome is recorded in `<app data>/faucet-budget.json` (0600) and shown.

Honest outcomes: `sent`, `rate_limited` (with the next time), `challenge_required` (CAPTCHA),
`refused` (with the faucet's reason), `unreachable`, `unknown` (counted against the window).

The faucet URL is `https://faucet.citrate.ai`; `CITRATE_FAUCET_URL` overrides it with another
https URL or a loopback http URL (for local testing against a faucet on this machine).

## What the faucet does (citrate-chain `faucet/`, branch `hup/n5-chain-faucet`)

`GET /ready` (readiness), `GET /eligibility?address=` (read-only next eligible time), stable
refusal `code`s, and opt-in limits: an hourly cap over all callers, a membership-SBT check that
fails closed, the CAPTCHA page with address prefill, and the exact desktop webview origins for
CORS. Each is off unless the operator sets it. See `faucet/README.md` there.

## Pending owner sign-off

| Item | This build |
|---|---|
| ADR acceptance | Proposed. The switch is off by default. |
| O-1 drip size | The faucet's 10 SALT. No gas-only drip. |
| O-2 membership | One request per 24 h per wallet in the app. The faucet's membership check exists, off until the operator sets `FAUCET_MEMBER_SBT`. |
| O-3 CAPTCHA | Solved by the member in an in-app window on the faucet's own page. |
| O-4 placement | Settings, Budgets; off by default. |
| Window and cap | 24 hours, one request (placeholders). |

## Operator step (not done by this build)

Deploying the faucet change and choosing its settings is a DGX-team step, tracked on the HUP-S6
sprint issue. Until the new faucet is deployed, core still works with today's faucet: it falls
back to `/health`, reads the old "Rate limited: … remaining" text for the next time, and says
when the faucet does not report readiness.
