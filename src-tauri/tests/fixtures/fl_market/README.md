---
created: 2026-11-11
branch: feat/flm-core-1-offers-read-model
author: Larry Klosowski + Claude Opus 5.5 (V60-FLM lane)
status: active
---

# fl_market fixtures (recorded, not hand-written)

Recorded by `scripts/fl/flm-market-anvil.sh` with `FLM_FIXTURE_OUT` set, against the merged
WorkMarket (citrate-chain 39b75f88) deployed on a loopback anvil, chain id 40204.

- Market: `0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0`
- Block 0: `0x26159e7c146dc8e50b775856e49d32087e1fe49944ad0b5da828fa5fdb98f4ab`
- `logs.json`: the node's `eth_getLogs` answer for OfferOpened/OfferClosed from block 0.
- `get_offer_N.hex`: `getOffer(N)` after the run.
- `bafkrei…`: the requirements files. Offers 1, 2 and 4 commit the keccak256 of their own file;
  offer 3 commits the hash of `bafkreiflmopen` but names `bafkreiflmswapped`.
- Offer 2 is a group offer for the MLS group id `0x11…11` (32 bytes). Offer 4 was refunded after
  its deadline (OfferClosed, Expired).
