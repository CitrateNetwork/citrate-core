---
title: "citrate-core QA Triage — end-to-end, what's real vs fake vs DGX-gated"
created: 2026-07-16
branch: feat/core-qa-triage
author: Claude (Opus 4.8) for SaulBuilds
status: working-checklist
related:
  - docs/CITRATE_CORE_MASTER_CHECKLIST.md
  - docs/CITRATE_CORE_DGX_WORK_ORDER.md
---

# citrate-core QA Triage

End-to-end control-by-control QA (2026-07-16, three parallel audits). Already made
real this session and NOT re-listed: identity/RBAC, node vitals, wallet
liquid+claimable, auth gate. This is the working checklist for everything else.

Legend: 🔴 BROKEN/misleading (in our control) · 🟡 HONEST-STUB (relabel/low-pri) ·
🔵 DGX-GATED · ✅ REAL.

---

## TIER 0 — the root cause (fixing this repairs the most)

- [ ] **The signature ceremony is simulated.** `store.approveCer()`
  (`store.ts:618-635`) fabricates `cerHash: makeHash()` and mutates balances
  locally in BOTH modes — it never calls `bridge.signing.request/approve/broadcast`.
  Only `store.claimRewards()` (`store.ts:960-989`) does it right (real broadcast, no
  local mutation, honest web-preview toast). **Rule-3 violation.** Fixing this one
  method + the four `apply` callbacks below makes all of them honest. 🔴

Downstream of Tier 0 (each is an `apply` that fabricates on the sim ceremony):
- [ ] Wallet **Send** — `Wallet.tsx:71-97`, toast "Sent — witnessed on 40204" 🔴
- [ ] Wallet **Add stake** — `Wallet.tsx:99-122`, toast "position updated from chain" 🔴
- [ ] Wallet **Withdraw** — `Wallet.tsx:124-151` 🔴
- [ ] Node **Pinning bond** — `Node.tsx:141-172`, debits liquid, invents attested pin 🔴
- [ ] Journal **Pin securely** — `Journal.tsx:201-224`, fabricates a CID by string concat 🔴

---

## TIER 1 — in-our-control, fix now (BROKEN / misleading)

### AI / chat
- [ ] **OpenAI/provider key does nothing.** Masked + discarded at `Settings.tsx:161`;
  chat is `createDemoProvider` (`store.ts:132`) — never calls a model. Toasts claim
  keyring seal + gateway routing (false). Fix (cheap): make the AI providers section
  HONEST ("provider inference not wired — chat runs on the local demo agent"); drop
  the "OS keyring"/"routes to gateway" copy. Fix (real, @rule8): keyring command +
  Rust-side inference (OpenAI-compatible / Citrate gateway), swap the provider by
  `aiDefault`. 🔴 / 🔵(real path)
- [ ] "Default model route" buttons — `Settings.tsx:127-132`, toast asserts routing
  that never happens (`aiDefault` is unread). 🔴
- [ ] Dashboard chat-backend label `infer.citrate.ai · cgk_…` — `Dashboard.tsx:73`,
  cosmetic; say "local demo agent". 🔴

### Settings — misleading toasts / fake actions
- [ ] Gateway key **Issue/Store/Rotate/Revoke** — `Settings.tsx:194-215`, client-side
  `Math.random()` key, "stored in OS keyring"/"revoked server-side" all fabricated. 🔴
- [ ] Connections **Connect/Disconnect** — `Settings.tsx:104-116`, fake 1.3s OAuth
  timer + "token revoked from the keyring". 🔴
- [ ] **Check for updates** — `Settings.tsx:256-259`, fake setTimeout, always
  "current", copy claims "signature verified offline". 🔴 (real updater is @rule8 🔵)
- [ ] "Manage account ↗" — `Settings.tsx:317`, toasts but opens nothing. 🔴
- [ ] "Export diagnostics bundle" — `Settings.tsx:900`, toasts but exports nothing. 🔴
- [ ] Keys "Smart wallet ACTIVE / Machine ATTESTED" — `Settings.tsx:673-694`,
  hardcoded status strings, not read from any attestation. 🔴
- [ ] Billing **Renew ↗ / Cancel** — `Settings.tsx:781,784`, toast only — but
  `bridge.membership.checkout()` already exists. LOW-EFFORT real wire. 🔴
- [ ] Receipt **PDF ↗** — `Settings.tsx:821`, no download. 🔵 (needs core-membership)

### Dead links (accept a click, do nothing)
- [ ] Dashboard **tutorials** rows — `Dashboard.tsx:248-260`, inert spans. 🔴 (rides SSO/open_app)
- [ ] Commissary "open in Atlas / docs / signed-in · open" — `Commissary.tsx:301,340,360`. 🔴 (rides SSO/open_app)
- [ ] Wallet activity "↗" / identity "chain proof ↗" — `Wallet.tsx:383,409`, no onClick. 🔴

### Commissary / Storage / Node
- [ ] Commissary **Download** "✓ verified · sha256 match · audit-logged" —
  `Commissary.tsx:54-68,262`, pure setTimeout, no bytes/checksum. 🔴 (real needs signed URLs 🔵)
- [ ] Commissary **Open panel** micro-app — `store.ts:991-1005`, toasts "opened" but
  no webview spawns. 🔴
- [ ] Storage **Enable semantic search** — `store.ts:568-571`, fake model download
  claiming "sha256 verified". 🔴 (real download 🔵)
- [ ] Node **Pause/Resume** — `Node.tsx:65-73`, local state flip, no supervisor call;
  "heartbeat continues" asserted not verified. 🔴 (real pause needs bridge.node.pause 🔵)

---

## TIER 2 — honest-stubs (labeled; relabel/defer, low priority)

- Node config "Move data dir", Keys "Export keystore", "Allowlist rules editor",
  "Unlock passphrase prompt" — all say "in the wired build" (honest). Add real UI later.
- Comms "Open ↗" (sim-scoped, never shown to a real signed-in user). ✅ honest.
- Node pinning [SEAM] banner, Earning off-chain-estimate label — honest. ✅

---

## TIER 3 — DGX-gated (wait for work-order deliverables)

- Real OAuth connect tokens, real gateway-key issuance, receipt PDFs (core-membership).
- Commissary signed-download URLs + catalog manifest (WO-7).
- Real inference gateway transport (WO-3 model / gateway).
- Comms relay pings (WO-9), storage graph ingest + pinning (WO-4), staked/activity
  grounding (node-agent address book + indexer).

---

## Recommended order

1. **Tier 0** (the sim ceremony) — one fix, repairs 5 money-surface lies (Rule-3).
2. **AI-key honesty** (Tier 1) — the user's specific complaint; cheap one-file fix.
3. **Settings misleading toasts batch** (Tier 1) — relabel fakes honestly; wire the
   cheap reals (Billing Renew → membership_checkout).
4. **Dead links** — fold into the SSO `open_app` work (rides the auth gate).
5. Defer Tier 2/3.
