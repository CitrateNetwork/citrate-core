---
title: "citrate-core — Feature Map, Sitemap & Wiring Status (beta assessment)"
created: 2026-07-21
branch: main
author: Claude (Opus 4.8) for SaulBuilds
status: assessment — for owner review + copy revision, feeds the 2026-07-21-core-beta-qa planset
source: 6 parallel surface audits (onboarding/money, node/daemons/validator/earnings, wallet, settings, storage/MCP, comms/journal/chat) 2026-07-21
---

# citrate-core — Feature Map, Sitemap & Wiring Status

## How to read this
**The governing fact:** the app runs in one of two modes (`src/bridge/mode.ts`):
- **`tauri`** = the packaged desktop app → **real** OIDC / chain / daemons.
- **`sim`** = web preview (`vite dev`) → intentionally fabricated animations so the flow "walks."

So "is it real?" is answered **per-feature AND per-mode**. Below, wiring status is for the **tauri (packaged)** build — the only honest money path. Legend:
**REAL** (live) · **SIM** (web-preview fake only) · **STUB** (honest "unavailable") · **FABRICATED** (shown as live but isn't — a Rule-1 violation) · **DEAD** (button/link does nothing) · **HARDCODED** (asserts a fact with no real read).

The node now **cold-syncs the SRP-S2 chain to head** (verified live 2026-07-21). The gaps below are what stands between "syncs" and "a stranger runs it, earns SALT, and every button is honest."

---

## 1. Sitemap

```
Launch
└─ Onboarding (one flow, resumable)
   S0 Welcome → S1 Sign in → S2 Verify (KYC) → S3 Pay ($48) → S4 Wallet ready
   → S5 Grant + stake ceremony → S6 Node ignition → S6.5 Download local model → Dashboard
   (S0 "Explore free" → Dashboard, public tier, bypasses pay)

Shell (after onboarding / explore)
├─ Dashboard   — vitals strip, chat (agent), tutorials, recent tx
├─ Node        — run/pause/stop, height/peers/sync, LOG panel, earnings split, pinning
├─ Wallet      — Overview (balances, Send, Receive, Paymaster) / Staking (stake, withdraw, claim, rewards) / Activity (tx history) / Identity (SBT, linked wallets, agents)
├─ Storage     — memory constellation graph, semantic search, MCP endpoint, tenants
├─ Journal     — pages, daily notes, backlinks, @agent, pin, export, voice
├─ Comms       — ping center (notifications)
├─ Commissary  — catalog (its own program: 2026-07-20-commissary-program)
└─ Settings    — Account / Connections / AI / Node / API / Keys & security / Memberships & billing / App

Daemons (sidecars, supervised)  — NONE bundled today; all fail BinaryNotFound on a fresh install
├─ citrate node      — chain sync (works via dev override; not bundled)
├─ node-agent        — earnings claim bridge (not bundled)
├─ mem-mcp           — memory MCP (not bundled, never started, empty)
└─ llama-server      — local Gemma inference (not bundled — honest error)

Signing — every write routes through the Rust SignatureCeremony (Rule 3); no key ever leaves Rust.
```

---

## 2. Per-surface feature + wiring status

### Onboarding (S0–S6.5)
| Step | Wiring (tauri) | Notes / copy issues |
|------|----------------|---------------------|
| S0 Welcome / Explore free | REAL | clean; free-tier bypass correct |
| S1 Sign in (loopback OIDC) | REAL | web-preview attest checklist is SIM but reads as live |
| S2 Verify KYC | REAL flow — **BLOCKED**: server `recheckKyc` is a TODO (always fails closed) | no grant fires without KYC go-live or dev flag |
| S3 Pay $48 (Stripe popup) | REAL — settles only on real entitlement | **FABRICATED**: hardcoded `order ord_2026_84117` on the settled card |
| S4 Wallet ready | REAL if `wallet_address` claim present | **FABRICATED**: persona-derived address if the claim is absent (should show "—") |
| S5 Grant + stake ceremony | REAL settle (attributedStake≥32k + SBT); counter is honest animation | well-built; no fabricated tx hash |
| S6 Node ignition | REAL supervised spawn + live vitals | "validating" line is static copy |
| S6.5 Download model | REAL streamed download + SHA-256 verify | best-built step |

### Node
- **Buttons:** Start/Stop **REAL**; **Pause/Resume are COSMETIC** (paint the label, no supervisor call).
- **LOG panel — DEAD in tauri (the "no log action" you see):** the node's stdout is dropped (`Stdio::inherit`), no events are emitted, `refreshNode` never writes `s.logs`. In web it's a **FABRICATED** template array. → the #1 node fix.
- **height/peers = REAL** (RPC). **syncPct = STUB** (binary 0/100). **peer rows / cpu / ram / blocksProposed = DEAD** (never folded) or FABRICATED in sim.
- **"Validating" = COSMETIC label** (`staked≥32000`); **no real validator registration** (no ValidatorRegistry, no proposer key) → not a real validator, `blocksProposed` fabricated.
- **Earnings split (val/pin/compute) = SIM** but honestly labeled "off-chain estimate — not in claimable"; **claimable read = REAL** (honest 0 for a fresh node); **claim ceremony = REAL**.
- **Pinning (pin CID / bond) = STUB** (honest "not wired to a real 40204 tx" — no fabricated CID).

### Wallet
- **Balances (native/staked/claimable) = REAL**; **wSALT hardcoded "0.00"** (honest). Staked shows real self-stake + a `32000` grant constant gated by the real `hasGrant` read.
- **Send / Add stake / Withdraw request+claim = REAL** (real ceremony → EIP-155 sign → broadcast; no fabricated hashes; real 7-day queue).
- **P0 GAP — no human review modal:** money actions call `signing.broadcast(view, false)` immediately — the "Review & sign" button signs what the code built with **no human Approve step**. (Ceremony invariants hold; the HITL gate is code, not a person.)
- **Activity (tx history) = REAL** (CitrateScan) — but **C-5: failed-tx status is stripped by the bridge**, so a reverted tx looks like a success.
- **SBT card + on-chain emblem = REAL** (re-pinned `0x4CE39F89`), honest local fallback.
- **Paymaster bar = FABRICATED** (`sponsorUnits`, no chain source — 40204 has no paymaster). **Rewards-accrued = SIM-seeded** in tauri.
- **Linked wallets / add local key / recovery = STUB** (backend keystore real, no UI wired).

### Settings — **the worst offender for "excuses baked into the UI"**
~24 controls REAL/honest, but **11 FABRICATED-toast** + **6 HARDCODED-status** + several stubs:
- **FABRICATED-toast ("…isn't wired yet, a scheduled build"):** Manage account, Connect (×9 OAuth), Data-dir Move, Gateway-key Issue/Done/Rotate/Revoke, Keystore Export, Cancel membership, Receipt PDF, Check-for-updates, Diagnostics export.
- **HARDCODED/misleading:** RPC health ("healthy · TLS" — no probe), bootnodes list, "ADDRESS SET" pill, receipts date `2026-07-11 · $48.00`, "updater signature verified offline" footer (updater unwired), "citrate-core:// registered" claim.
- **STUB:** Unlock (passes empty passphrase — a real vault fails), Allowlist editor, Disconnect (local-map only) + header "tokens land in the keyring" (nothing runs).
- **REAL/solid:** all 8 config toggles (net/rpc/data-dir/cpu/autolock/channel/telemetry/sig-policy), AI-provider keyring flow (add/rotate/remove/route), custody lock, sign-out, billing plan/renew, SALT grant record, account claims.
- The file header comment even claims "no dead controls, no 'not wired' pattern" — **currently false.**

### Storage / MCP memory — **unusable today**
- **mem-mcp daemon = not bundled, never started, nothing ingests** → graph is permanently "offline/empty." Bridge protocol is REAL but unreachable.
- **"Enable semantic search" = FABRICATED** (a `Math.random()` bar claiming "sha256 verified" against a model never fetched — Rule-1 violation).
- **Socket path shown = FABRICATED** (`~/.citrate/core/memory/…` constant, not the real socket); "Copy agent config" wouldn't connect.
- Constellation render + tenant counts = REAL (but always empty). Plaintext-at-rest is honestly disclosed.

### Chat / agent harness (Dashboard)
- Default = **demo provider** (SIM narrator over **real** state numbers; honestly labeled "local demo agent · preview"). Configured key = **REAL plain chat** (AI1, keyring-sealed), **no tool loop**.
- **DEAD wire:** `ai_chat_local` (the BC-3 local llama path) has **no frontend caller** — the whole local-inference machine is built + tested but unreachable.
- Demo `memory_recall`/`docs_link` = **FABRICATED** narrative; demo `memory_assert` shows "approved" but **writes nothing**; `journal_append` = REAL.

### Comms
- Ping list = **honest-empty** for real users (seeded pings only in web sim). "Open ↗" = FABRICATED-toast (sim rows only). Notifications relay (WO-9) unwired — honestly framed. **Most honest surface.**

### Journal
- Pages / daily notes / backlinks / export / voice = **REAL** (localStorage). **Caption overstates:** says "encrypted at rest in your data dir" — it's localStorage. Pin = STUB (honest, no fabricated CID after the Tier-0 fix).

---

## 3. Consolidated honesty punch list (every fabricated / hardcoded / dead item)
This is the "no excuses or notes baked into the UI" list — each must become a **real read**, an **honest disabled+annotated state**, or be **removed**. Never a fabricated success or an apology toast.

**Settings (11+6):** Manage-account (wire — trivial, `openExternal` exists), Connections OAuth (wire or disable), gateway-key cluster (no backend — remove or build cm command), Move data-dir, Keystore export, Diagnostics export, Cancel membership (cm portal), Receipt PDF (cm), Check-for-updates (updater WO-2), RPC health (real probe), receipts date (real read), "ADDRESS SET" (real read), bootnodes (real read), updater/scheme footer claims, Unlock (passphrase modal), Allowlist editor.
**Node:** LOG panel (stream real), Pause/Resume (wire or relabel), syncPct/peer-rows/cpu/ram/blocksProposed.
**Wallet:** review-and-approve modal (P0), failed-tx display (C-5), Paymaster bar (hide in tauri), rewards-accrued (real read or "—").
**Storage:** semantic-search fake download, socket path, copy-config.
**Chat:** wire `ai_chat_local`, demo recall/docs_link/memory_assert honesty.
**Journal:** soften "encrypted at rest" caption.
**Onboarding:** fake order id, persona wallet address.

---

## 4. Structural gaps (what makes it a real product, not just honest)
1. **Bundle the daemons** — `tauri.conf.json` has NO `externalBin`; on a fresh install nothing runs. Need WO-2 platform binaries (node, node-agent, mem-mcp, llama-server) + the bundle config.
2. **Real streaming node logs** — pipe daemon stdout → Tauri events → the log panel.
3. **A real validator with 32k** — WO-1.3 (dgx) validator-registration spec + app registration flow + real validator-set/blocksProposed reads. **Earnings stay 0 until this + pinning run.**
4. **Working memory MCP** — bundle + start mem-mcp + an ingest path.
5. **Paid onboarding go-live** — server KYC recheck (TODO), Stripe live, DB secrets, treasury signer re-point to the SRP-S2 pair (SBT `0x4CE39F89`/vault `0x61E324cF`, owner `0xf42a1919`), Vercel redeploy, DB reset for a fresh walk.

---

## 5. Copy / UX notes (human side — for your revision pass)
- **Kill every "a scheduled build" / "isn't wired yet" string** — these are the "excuses baked into the UI." Replace with real behavior or a clean disabled state.
- **Never assert a security fact you can't back:** "updater signature verified offline," "tokens land in the OS keyring," "encrypted at rest," "sha256 verified," "hardware attestation proved" — each must be true in the running build or removed.
- **Onboarding copy** reads as a finished product; ensure S2/S3 reflect real KYC/payment states (pending/failed/review), not just success.
- **"Validating"** should mean a registered validator, not `staked≥32000` — the copy currently implies proposer eligibility that isn't real.
- Sitemap is stable; the labels are good. The work is making the surfaces *mean* what they say.

---

## 6. What's genuinely REAL + solid (ship as-is)
Money-path server code (Stripe/webhook/grant/SBT/entitlement/license), wallet money actions (send/stake/withdraw — real ceremony broadcast), on-chain grant verification, on-chain SBT art, real balances/activity/claimable/billing-expiry, config toggles, AI-provider keyring flow, custody lock, sign-out, checkout, **node cold-sync (new)**. The foundation is honest; the beta work is finishing the surfaces and lighting up the daemons + validator + money-path go-live.
