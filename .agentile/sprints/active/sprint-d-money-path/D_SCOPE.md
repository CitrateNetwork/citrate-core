---
created: 2026-07-14
branch: docs/phase-d-scope
author: Claude Fable 5, directed by @SaulBuilds
status: scoped — Phase D opened (owner cleared the counsel gate; deploy to make it live+testable)
program: CORE Phase D — the money path (maps to beta-wiring Phase D + product CORE-S5/S6)
rule8: yes throughout — payments, treasury custody, grant issuance, sponsored gas
grounded_in:
  - beta-wiring 00_STATE_AND_PLAN Phase D (D1-D4); product planset 05 CORE-S5/S6
  - DEPLOYED (D1): CitrateMemberSBT 0xb8a52197..04D3 + MembershipStakeVault 0x1271..7A2C on 40204
  - vault.grant()/lapse/renew/release + SBT mint are onlyOwner → the grant-orchestrator must OWN them
---

# CORE Phase D — the money path

A member pays the annual pilot fee, is KYC-verified, and receives 32k SALT staked in the vault + a minted
MemberSBT + a raised entitlement — end to end. This is the phase gated on real-money and
securities concerns; the owner has cleared the counsel gate to get it **live and testable in
beta** (sign-off follows a working beta, not precedes it).

## Where D1 stands
- ✅ **CitrateMemberSBT** `0xb8a52197E5E3b03625E6817bC54D03F5820f04D3` — deployed, owner=0x98a3 (transferable).
- ✅ **MembershipStakeVault** `0x127142194416A58280E638f286ECBcDc493d7A2C` — deployed, pool=LiquidStakingPool.
- ⬜ **D1b — E-8 paymaster (sponsored UserOps).** citrate-chain #67 changed CitratePaymaster +
  factory + DeployAA. Redeploy = new paymaster addr → **fund the EntryPoint deposit** → set up
  the first-op **registrar** → **re-pin the new AA addresses across the federation** (bundler,
  wallet-factory consumers, citrate-core). @rule8 AA-stack operator work. Only gates *sponsored*
  (gasless) UserOps — the core grant path works self-funded without it.

## The owner-gated prerequisites (Phase D can't fully close without these)
- **Stripe entity + keys** — a real Stripe account for the annual Pilot rail (test-mode first).
- **Treasury custody** — the grant SALT source (32k/member) + the droplet signer's authority.
- **Ownership transfer** — the vault + SBT are `onlyOwner`; ownership moves from the deployer
  (0x98a3) to the **grant-orchestrator / droplet-signer** address once D2 stands it up.
- **Counsel memo** (cleared per owner) — kept as a filed artifact for the audit trail.

## Work packages
### D1b — deploy the E-8 paymaster @rule8 (AA-stack)
Redeploy the fixed paymaster via `DeployAA.s.sol`; fund the EntryPoint deposit; configure the
first-op registrar; capture + re-pin the new AA addresses (40204.json + federation drift +
citrate-core). Verify a sponsored first-op deploys a wallet proxy (EntryPoint event). Build-and-
stop → independent review. *(Heavier than the membership deploy — federation address ripple.)*

### D2 — the `core-membership` service (NET-NEW, @rule8) — the largest chunk
A new repo/app: **Next.js on Vercel + Neon** (field-level encryption of sensitive columns, keys
outside Neon) + a **DigitalOcean droplet signer** (treasury keys NEVER on Vercel).
- **D2.1 scaffold + OIDC RP:** the app, order state machine, an audit hash-chain, citrate-identity
  OIDC RP (reuse the dataroom/comms RP pattern). Acceptance: state-machine property tests; audit
  chain verifies on replay; raw-Neon dump shows ciphertext for protected columns.
- **D2.2 Stripe rail (D-10):** annual checkout + customer portal + webhook settlement, idempotent
  by Stripe ids; Enterprise contact CTA. Acceptance: test-mode payment settles exactly once under
  webhook replay.
- **D2.3 grant orchestrator (the heart):** sub + KYC + payment **triple-check** → **vault
  `grant()` (32k SALT → staked + attributed)** → **SBT mint** → **entitlement raise** (auth.citrate.ai
  claim). The droplet signer owns the vault+SBT (ownership transferred here). Acceptance: staging
  run — vault attribution, pool shares held by vault, SBT tokenOfOwner, raised /userinfo claim all verified.
- **D2.4 treasury signer custody @rule8:** the droplet worker (operator-gated, per-day caps,
  dual-control runbook); Vercel env audit shows no signing material. Fund the grant SALT source.
- **D2.5 license token API + in-app refresh/grace/lapse states.**

### D3 — wire citrate-core to the live flow @rule8
Onboarding **S3 (pay)** → Stripe checkout + webhook status; **S5 (grant ceremony)** → poll vault
attribution + SBT mint + entitlement; **Wallet Identity** (MemberSBT + AgentSBT reads, identity
registry link). Sponsored UserOps once D1b lands. Acceptance: the S3→S5 onboarding leg lights up
against the live service.

### D4 — Commissary @rule8
Signed catalog manifest + signed download URLs (first-download/TTL, SHA256) + entitlement gate +
tier/org rendering; gateway-key issuance (E-2). Acceptance: 03 commissary gherkins; expired-URL retry.

## Phase-D gate (the honesty gate)
A real member pays the annual pilot fee (Stripe test→live) → KYC-verified → receives **32k SALT staked + a minted
MemberSBT** (real 40204) → entitlement raised → can download a tier-gated app with a signed URL.
Every step real or honestly "coming" (Rule 1).

## Sequencing + first dispatch
1. **D1b** paymaster deploy (unblocks sponsored UserOps; self-contained AA op) — *or* defer to
   after D2 if self-funded grants are acceptable for the first beta.
2. **D2.1** core-membership scaffold + OIDC RP (the net-new service foundation) — the critical path.
3. Then D2.2/D2.3 (Stripe + grant orchestrator) → D3 (wire citrate-core) → D4 (Commissary).

Recommended first dispatch: **D2.1 (core-membership scaffold)** — it's the long pole and the
whole money path sits on it. D1b (paymaster) can run in parallel (citrate-chain, independent).

## Owner decisions needed to proceed (I'll ask at dispatch)
- New repo name/location for `core-membership` (+ Vercel project + Neon DB provisioning).
- Stripe account (test-mode keys to start).
- The grant-orchestrator/droplet-signer address (→ transfer vault+SBT ownership to it) + the
  treasury SALT source for the 32k grants.
- D1b now vs after first self-funded beta.

## Out of scope (still, per planset)
Mainnet (stays 40204). CORE-S8 credential conformance (post-v1). Reputation (D-8).
