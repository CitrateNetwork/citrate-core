---
created: 2026-10-19
branch: beta/fl-round-cards
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Group round cards (0.5.2, first cut, behind a flag)

Group rounds go live after mainnet. FederatedRoundLedger and PatronageLedger deploy dormant at
genesis (audit phase A). In 0.5.2 the app can show where they stand, behind a flag, and nothing more.
This is the first item cut if the code freeze is at risk.

## The flag

There is no runtime feature-flag system in core, so this is one build-time switch, default OFF:

```sh
VITE_FL_ROUND_CARDS=1 npm run build   # ON; any other value, or none, is OFF
```

`src/fl/roundCardsFlag.ts` reads it. With the flag OFF, Groups › Rounds renders `Train` and group
chats render exactly as before. `src/fl/groupRoundCardsFlagOff.test.tsx` pins both against
snapshots taken before the cards were added.

## What the cards show (flag ON)

The same card appears on Groups › Rounds (where it replaces Train's PatronageLedger read; the
federated-rounds panel stays below it) and at the top of each group's chat.

| State | When | Wording |
|---|---|---|
| Not live | The address book has no entry for either ledger (today) | "Group rounds not live yet". No chain read is made |
| Not live (no code) | The book pins both, but `eth_getCode` is empty at one | "Group rounds not live yet", naming the ledger |
| Unknown | The code read failed | "Round ledgers: —" with the reason. Never zero, never dormant |
| Deployed · dormant | Both pinned and both have code | "Group rounds are not open yet": deployed, dormant while audited, rounds open after mainnet |

Data source (Rule 7): `src-tauri/addresses/40204.json` for the addresses, then core's
`contract_code_size` (`eth_getCode` on 40204). No round, amount or projection is shown.

## Fixtures

The design's round states (FederatedRoundLedger `Status`: Committed, Accepted, Rejected) live in
`src/fl/fixtures/groupRoundStates.fixture.tsx`, labelled "fixture" on the card, and are rendered only
by tests. A test fails if any production module imports that file.

## Not in this cut

- Reading a group's actual rounds. That needs a group → cluster id mapping (`clusterIdOf(coordinator,
  salt)`) that core does not have yet.
- "Deployed · dormant" is not yet in the UI truth contract's state vocabulary (§1); it is requested
  there.
