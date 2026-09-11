---
created: 2026-09-11
branch: feat/hermes-p3-model-register
author: Claude Fable 5
status: active
sprint: sprint-hermes-p3-headline
planset: 2026-09-11-hermes-agent
tier: T1
---

# Sprint — Hermes P3: Headline skills

Third phase. The two flagship skills that make Hermes do real work on-chain, both
ceremony-gated (Rule 3 — the agent proposes, the human signs).

## Work packages
- **WP3.1 — hf-model-pull-register.** Download (off-main-thread `model_catalog_download`)
  → verify → pin to IPFS (CID) → register on-chain so the model appears in the router's
  registry source. THIS SPRINT: the on-chain register half — a byte-exact
  `ModelRegistry.registerModel` calldata builder + a `models_registry_register` command
  that submits a PENDING SignatureCeremony, exposed through `modelsCatalog.register()`.
  Closes the loop with the P2/WP0.2b registry READ.
- **WP3.2 — contract-deploy.** DONE: a REAL contract-creation ceremony. The Agent-tab
  deploy now assembles caller-supplied compiled bytecode (+ optional constructor args)
  into a `to`-less creation tx and submits a pending SignatureCeremony; `txdecode` renders
  it "Deploy contract", the human approves, `signing.broadcast` signs + sends the real
  40204 creation tx (B1.4). The app never ships/fabricates bespoke bytecode (Rule 1) —
  bundling audited template bytecode (treasury/erc20/pin-vault) as one-click presets is a
  follow-on; fork-sim gas estimation is a follow-on (a gas param + default today).

## What shipped this sprint (WP3.1 register half)
- `model_register.rs` — `register_model_calldata(...)` (recursive head/tail ABI encoder
  for `registerModel(string,string,string,string,uint256,uint256,(string,string[],
  string[],uint256,string,string[]))`), byte-exact vs foundry `cast`. `REGISTRATION_FEE`
  = 0.1 SALT sent as tx value. `models_registry_register` command → PENDING ceremony
  (Rule 3); empty CID/name rejected up-front (mirrors the contract `require`).
- Bridge: `RegisterModelInput` + `modelsCatalog.register()` (tauri invoke; sim throws an
  honest "needs the desktop node").

## Dependencies / open
- **CID acquisition:** registration requires a real pinned IPFS CID. The pull path must
  `ipfs add` the verified GGUF to get a CID before registering — wired to `storage_pin`
  / the node IPFS sidecar (the CID is a command input today).
- **Fee funding:** the registering wallet must hold ≥ 0.1 SALT + gas.
- WP3.2 real bytecode templates.
