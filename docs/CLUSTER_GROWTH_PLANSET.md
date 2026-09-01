---
created: 2026-09-01
branch: docs/cluster-growth-planset
author: Claude (Opus 4.8), directed by @SaulBuilds
status: planset (Stage-2 — red-teamed 2026-09-01; several D-decisions REOPENED, see Red-team findings)
red_teamed: 2026-09-01
planset: cluster-growth
code: GROW
repo: citrate-core (app) + citrate-landing (web join) + citrate-comms/relay (rendezvous) + rewards backend
companions:
  - docs/CONNECT_REALIGN_PLANSET.md   # S0–S5 identity/invite groundwork this builds on
  - docs/connect/CONNECT-S5-wallet-identity-spec.md
working_title: "MUSTER (placeholder — owner names it; alts: RALLY, CREW, GATHER)"
---

# Cluster Growth — viral invites, real connection, and the earn-together flywheel

## Why this exists

The connection primitives exist (portable identity S5, a claims-inbox S1, a people-picker S2), but two
real users still can't reliably connect, the invite is a `citrate://` blob that dies at cold-start, and
there is no reason-to-share loop. The alpha goal is concrete: **stand up 10–20 flagship ~2,000-member
clusters, each with smaller focused sub-groups inside, organized around cluster goals — data storage,
training, inference, and building Dapps — with members paid in SALT to soak-test and secure the network
while they connect and transact inference as a cluster.** This planset turns "connecting a person" into
"grow your crew, earn together" — and makes it work on infrastructure that is actually on.

## Core invariant

**A person joins a cluster from a single shareable link — one click, one approval — every add is
relay-authorized and ceremony-signed, and rewards flow to real contribution, never to raw recruitment.**

## The flywheel (north-star loop)

1. You create a cluster → get a human referral link `citrate.ai/join/<code>` (not a `citrate://` blob).
2. You post it anywhere (X, Discord, iMessage). Anyone opens it in a normal browser.
3. A **web join page** renders *dynamically from the code*: who invited you, which cluster, its goal,
   and what it earns → **Get Citrate** (or Open in app). No app → install → the code rides through →
   land signed-in and already joined. Have app → deep-link in, one approval, joined.
4. Both sides see it land: your cluster +1, projected reward bump, the new member can transact inference
   as part of the cluster immediately.
5. Rewards accrue to the cluster on **size × network-state uniqueness × usage/contribution**, split
   among members by contribution → everyone keeps inviting and contributing.

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| D-1 | First invite sources | **Your people (Citrate members, S0/S2) + a universal share link.** Discord server-members is a fast-follow; X is broadcast + handle-verify only, NOT a contact-list pull. |
| D-2 | Why not X/Discord "friends import" | Platform reality: OAuth exposes **your own handle only**. Discord CAN expose **co-members of shared servers** (via `guilds`+`guilds.members.read` / a Citrate bot). X following-lists need a **paid, rate-limited, fragile** API tier → out of scope. No false friend-import (Rule 1; consistent with CONNECT-S3). |
| D-3 | Invite carrier | A short server-resolvable **referral code** → `{cluster, inviter, goal}`, behind `citrate.ai/join/<code>`. Replaces the ephemeral-key-in-the-link. |
| D-4 | Cold-start | The web join page works with **no app**: dynamic CTA (inviter + cluster + goal) → install → deep-link handoff carries the code → auto-join. New users get a wallet-identity automatically (S5) — zero crypto setup. |
| D-5 | How two users actually connect (alpha) | **Relay rendezvous**, not peer-to-peer. Invite claim + MLS welcome + group traffic route through the DO-hosted server-blind relay, so connection does NOT depend on the soak-gated libp2p mesh. (P2P mesh stays @rule8/soak-gated for later.) |
| D-6 | Reward basis | Cluster SALT = f(**size**, **network-state uniqueness**, **usage/contribution**), split by contribution. Curved/capped on size + sybil-gated so it rewards *securing + using* the network, not headcount farming. |
| D-7 | Referral dignity (anti-tacky, user protection) | See Design Principles §"Dignity & protection". No auto-posting, no contact spam, private attribution, mission-framed, contribution-weighted. Non-negotiable. |
| D-8 | Cluster shape | Flagship clusters (~2k) each with a **goal/theme** (storage / training / inference / Dapp) and nested **sub-groups**; **campaigns** are time-boxed goals a cluster works toward for bonus SALT. |
| D-9 | Security unchanged | RBAC at the relay, ceremony-signed role changes/adds, server-blind, identity = wallet-derived comms key (S5, one per human/device → sybil floor). |

## Design principles (app ↔ web ↔ groups ↔ campaigns)

**Dignity & protection (D-7 — the anti-tacky spine):**
- **Reward contribution, not recruitment.** Size is one factor, curved and capped; the weight sits on
  real usage + unique network-state contribution. No pure "refer N friends = $" mechanic (that's the
  Ponzi smell). This is framed as *"you're paid to secure and soak-test the network,"* not "get rich."
- **No auto-spam, ever.** The app NEVER posts to a social account or messages contacts on its own.
  Sharing is always a single, user-initiated action (a link + the OS share sheet).
- **Private attribution.** Who invited whom is private by default; no public recruiter leaderboards
  unless a member opts in. The join page shows only what the inviter authorized (display handle +
  cluster name + goal) — never PII.
- **Consent both ways.** Invitee approves the join; owner/admin approves the add (ceremony). No surprise
  enrollment, no pre-checked boxes, no dark patterns.
- **Anti-phishing.** The referral code resolves server-side to a signed invite; the join page cannot be
  spoofed to harvest credentials — it only ever routes to the real app download / deep link.
- **Honest numbers.** Projected rewards are shown as ranges tied to real signals, never a fabricated
  "you'll earn $X" (Rule 1).

**Experience:**
- **One link, one click, one approval.** No copy-pasting claims. A Share sheet, not a hex blob.
- **The inviter always sees status** — pending invites, who joined, cluster growth, the reward curve.
- **The cluster has a purpose on screen.** Its goal/theme + current campaign + progress are front and
  center, so joining feels like joining a *mission*, not a chatroom.
- **Cold-start is one tap for the invitee** and degrades gracefully (advanced "join by code" fallback).
- **Web and app share one visual identity** (the citrate-landing system) so the link → app feels seamless.

## Architecture at a glance

```
  Inviter (app)                 citrate.ai/join/<code>            Invitee
  ─ mint referral code ─────►  web join page (dynamic CTA) ─────► Get Citrate / Open in app
        │                         │  (resolves code→cluster,          │ install → deep-link(code)
        │                         │   inviter, goal via join svc)      │ → auto-join request
        ▼                         ▼                                    ▼
  ┌───────────────────────── DO-hosted server-blind RELAY (rendezvous) ─────────────────────────┐
  │  claims-inbox (S1) · KeyPackage dir · MLS welcome delivery · per-cluster usage metering feed  │
  └───────────────────────────────────────────────────────────────────────────────────────────┘
        │                                                                    │
        ▼                                                                    ▼
  Rewards accounting  ◄── usage/contribution (inference gateway, DGX) ── cluster ledger → SALT split
  (size × uniqueness × usage)                                             (attribution: referral-aware)
```

## Surfaces we consume (reuse map — don't rebuild)

- **Identity**: S5 wallet-derived comms identity (portable across devices) — the sybil floor + "who is this."
- **People directory / picker**: S0 `buildPeopleDirectory`, S2 `addablePeople` + one-click add.
- **Claims-inbox**: S1 relay claims-inbox (comms-relay `submit_claim`/`poll_claims`) — the round-trip.
- **Ceremony**: every add / role change is ceremony-signed (Rule 3).
- **Relay**: citrate-comms `comms-relay` + `comms-member-daemon` (needs DO deployment — see Open deps).
- **Web**: citrate-landing (Vercel) for the join page; shares the design system.
- **Usage signal**: the DGX inference-gateway meters inference bought/sold (per-cluster) — the reward's
  "usage" axis. (Needs a per-cluster metrics feed — see Open deps.)

## Scope — sprints & work packages

| Sprint | Goal | Key WPs (each names its data source) | Depends on |
|---|---|---|---|
| **GROW-S0** | In-app **Invite** surface: your people + share link | Referral-code primitive (code→{cluster,inviter,goal}); Invite surface with "Your people" (S2 picker) one-click add + "Share your link" (OS share sheet); pending/joined status for the inviter | S0/S2; join svc (S1b) |
| **GROW-S1** | **Web join** page + cold-start handoff | `citrate.ai/join/<code>` dynamic CTA (inviter+cluster+goal, resolved via join svc); install → deep-link/universal-link carries code → auto-join; advanced "join by code" fallback | citrate-landing; **DGX: join-code resolver host** |
| **GROW-S1b** | **Join service** (code resolution) | Short-code mint/resolve → {cluster, inviter, goal}; signed, rate-limited, private attribution | **DGX: where it lives (relay vs small DO svc)** |
| **GROW-S2** | **Real connection via relay rendezvous** | Point the member-daemon at the DO relay; invite claim + MLS welcome + traffic round-trip through it so two machines connect with NO p2p mesh; honest error if relay unreachable | **DGX: relay deployed + public wss + daemon env** |
| **GROW-S3** | **Discord server-members** lane (fast-follow) | `guilds`+`guilds.members.read` (or Citrate bot); list co-members of shared servers; invite = hand them the link / bot DM; consent + privacy surface | Discord app/bot creds |
| **GROW-S4** | **Cluster model**: sub-groups + goals + campaigns | Cluster theme/goal; nested sub-groups (hierarchy); time-boxed campaigns with progress; the "mission" UI | S4 role navigator |
| **GROW-S5** | **Rewards flywheel** | Cluster ledger (size×uniqueness×usage); referral-aware attribution; sybil gating (S5 identity + KYC/PoW-VRAM tier); rewards dashboard; SALT distribution | **DGX: per-cluster usage feed; owner: reward mechanism/contract** |

**Build order:** S0 + S1/S1b (shareable link + cold-start) and S2 (make connection actually work via
relay) come FIRST — connection working + shareable is the foundation. S3 (Discord) is the "find your
friends" upgrade. S4/S5 (clusters + rewards) are the amplifier. Get people connecting before you pay them.

## Out of scope (v1)

- Peer-to-peer libp2p mesh for partner traffic (stays @rule8/soak-gated; alpha uses relay rendezvous).
- X following-list import (paid/fragile API).
- Public recruiter leaderboards (privacy default).
- On-chain reward settlement if an off-chain alpha ledger is faster to soak (revisit at mainnet).

## Open dependencies / infra (what blocks us — see the DGX prompt)

1. **Relay deployed to DO with a stable public `wss://` endpoint** + the member-daemon configured to use
   it as rendezvous (`CITRATE_MEMBER_*`). Without this, **no two users connect** — this is the #1 blocker.
2. **Join-code resolver host** — where `citrate.ai/join/<code>` resolves the code (relay endpoint vs a
   small DO service). Owner/DGX call.
3. **Per-cluster usage metering** from the DGX inference gateway — the reward "usage" axis.
4. **Reward mechanism owner** — is there a contract/plan for cluster SALT (size/uniqueness/usage), or do
   we design it? (T1 money — its own spec + red-team.)
5. **Uniqueness signal** — how "network-state uniqueness" is measured (needs a concrete definition).

## Gates (exit criteria — Stage-1 sketch, to harden in gates.yaml)

- G0: referral code round-trips (mint in app → resolve on web → deep-link back → join request). Test-backed.
- G1: a **fresh machine** opens a shared link and lands **joined** with zero manual crypto steps.
- G2: **two real users on two machines connect** through the DO relay (no p2p mesh). Live soak proof.
- G3: rewards ledger attributes size/usage/uniqueness to a cluster from **real signals** (no fabrication).
- G4: dignity checklist (D-7) satisfied — no auto-post, private attribution, contribution-weighted, honest numbers.

## Red-team findings (Stage-2, 2026-09-01) — THESE SUPERSEDE the naive text above

Independent adversarial review, verified against the code. The findings below **override** the locked
decisions and design text where they conflict. Nothing past GROW-S1 is built until the "MUST fix" items
are resolved.

**F1 — CRITICAL: the "sybil floor" is fictional; the size reward is free money.** S5 identity is
`HKDF(BIP39 entropy)` = **one-per-WALLET**, and a wallet is a free local mnemonic — not one-per-human,
not one-per-device, no cost, no hardware. D-4's zero-setup auto-wallet makes it *scriptable*: loop →
mint mnemonic → auto-join → own a "2,000-member cluster" that is one person, collecting the size reward
for 2,000 fake seats. "Curved/capped" only trims marginal reward; every fake seat is still pure profit.
→ **Reward eligibility must bind to a SCARCE, verifiable unit — staked SALT per seat (staking exists
in-tree; a bonded seat costs real capital) and/or proof-of-personhood/hardware — NEVER a comms identity.
Reward must be capped by verifiable network WORK, not headcount.** You cannot have all three of
{frictionless auto-wallet (D-4), size-weighted reward (D-6), sybil resistance}: keep D-4 for
*connectivity*, gate *reward eligibility* behind a scarce proof.

**F2 — HIGH: recruit-to-earn is a pyramid dynamic.** Referral-aware attribution = money flows up the
recruitment tree; at cold-start (little real usage yet) recruits earn mainly by recruiting → Howey /
endless-chain exposure for a T1 money repo. → **Referral reward = a small, flat, NON-compounding,
single-level bounty for a *verified real* invite — never a share of the invitee's ongoing earnings,
never invitee-of-invitee. Dominant reward axis = provable work the network consumes. Securities counsel
reviews the reward spec BEFORE GROW-S5 (hard gate).** Kill any UI copy projecting earnings from
recruiting. (GROW-S0 copy already scrubbed of this.)

**F3 — HIGH: the join link is a phishing/impersonation + malware-installer funnel.** Server-side signing
authenticates the *code*, not that the displayed inviter/cluster name is who they claim. Attacker mints
a real `citrate.ai/join/<code>` for a cluster named "Citrate Foundation — Genesis Airdrop" and links a
4.2GB "installer." → **Reserved/verified cluster + inviter names (block Citrate/official/airdrop/…);
flagship clusters get a cryptographic verified badge; unverified invites render an explicit warning.
Installer signed+notarized from ONE canonical HSTS-pinned origin with the hash shown (treat as @rule8
gated-download). Defensively register look-alike domains.**

**F4 — HIGH: "server-blind" is contradicted by the relay's own job.** Content-blind ≠ metadata-blind,
and the reward system REQUIRES the relay to observe metadata: full cluster rosters (rendezvous),
who-invited-whom (attribution), per-cluster usage (metering). One operator/breach/subpoena reconstructs
the whole social graph + activity. → **Stop claiming "learns nothing." State precisely: content-blind,
metadata-EXPOSED, with an explicit inventory. Move usage metering OFF the rendezvous path
(member-attested signed usage receipts, aggregated by a SEPARATE accountant). Attribute to cluster (or
blinded tokens), not inviter-chain. Split trust domains: rendezvous relay ≠ join-code resolver ≠ reward
accountant. No IP logging + retention limits on the resolver.**

**F5 — HIGH: one DO relay is the network's single point of failure/censorship/observation.** All
connection + invites + welcome delivery + metering + RBAC through one box, at up to 40k members, on a
public advertised `wss://`. A welcome storm or DDoS takes down *all* clusters; the relay can silently
censor/reorder adds; it sees everything (F4). → **Capacity model + load-test to 40k BEFORE GROW-S2;
per-cluster/region sharding (no single global instance at flagship scale); DDoS/WAF + rate limits on the
public endpoint; tamper-evident, client-verifiable roster transcripts so members can DETECT relay
censorship/reorder; treat the RBAC-bearing relay as @rule8; P2P/mesh fallback on the roadmap.**

**F6 — MED-HIGH: the 4.2GB install kills the viral loop.** click → page → 4.2GB download (models
bundled) → install (Gatekeeper, disk, managed Macs) → join. Cold-share end-to-end conversion is low
single-digit %; reaching 2k *real* members needs ~40k–200k cold clicks/cluster — pressure that
*manufactures* sybil seats (F1). macOS-only further caps reach. → **Split the client: a lightweight
join/identity client (tens of MB) for the invite path; download models lazily only when the member runs
local inference. Show download size + progress honestly. Signed/notarized installer. Re-baseline the 2k
target against real funnel math.**

**F7 — HIGH: "uniqueness" is undefined and "usage" is wash-tradeable.** Two of three reward axes are
ungameable only because they don't exist yet. "Unique network state" invites junk-but-unique farming
(2,000 random 1KB blobs); per-cluster usage is wash-traded (A buys inference from B, both mine, SALT
round-trips) — combined with F1+F4 one actor controls numerator, denominator, and meter. → **No reward
axis ships without an adversarial spec (hard blocker before GROW-S5). "Uniqueness" = useful, VERIFIED
contribution (proof-of-retrievability against real demand; inference validated against held-out
challenges), never novelty-of-bytes. Usage nets out intra-cluster/common-control self-dealing; reward
only externally-originated demand; cap per-seat.**

**F8 — MED-HIGH: 2k-member MLS + nested sub-groups is aspirational, not modeled.** No sub-group/nesting
code exists; MLS is OpenMLS-in-sidecar; the cluster transport is one-group-per-daemon, single-node,
soak-gated OFF. 2k leaves = expensive ratchet tree + welcome/commit storms (every add = a Commit all ~2k
process) + `setRoster`-on-every-change races; epoch churn can drop members (the S5 bug at scale). →
**Add a SCALE GATE between G2 (two users) and any flagship push: prove 200, then 2,000 members connect +
sustain MLS epochs under join-churn through the relay. Design the sub-group model concretely (separate
MLS groups vs app-layer partitions — it changes everything) before promising it. Model/rate-limit
welcome-commit fan-out. Reconcile `cluster.rs`'s one-group-per-daemon reality with the multi-cluster
goal.**

**F9 — HIGH: the identity model contradicts itself across three docs.** Planset D-9 says "one per
human/device"; S5 spec says one-per-wallet; `cluster.rs` STILL documents the pre-S5 per-device random key
("wallet ≠ comms"). → **Freeze the identity model in ONE canonical ADR, fix the stale `cluster.rs`
comment, and re-derive all sybil/reward/admission claims from that single source before any reward work.**

### Amended locked decisions (post-red-team)

- **D-9 — REOPENED.** Delete "sybil floor / one per human/device"; S5 identity is not a sybil gate.
- **D-6 — REOPENED.** Two axes undefined + size axis farmable; needs the adversarial reward spec +
  counsel review before it can re-lock. Referral reward = flat, single-level, non-compounding.
- **D-5 — QUALIFIED (not reversed).** Relay-rendezvous for alpha is fine, but re-locks only WITH the
  F4/F5 mitigations (trust-split, metering-off-path, sharding, DDoS, tamper-evidence, honest privacy).
- **D-8 — DOWNGRADED to unproven.** 2k + nesting is a target, not a settled fact; gated on the F8 scale gate.
- **D-4 — KEPT but SCOPED.** Frictionless auto-wallet stays for *connectivity*; explicitly DECOUPLED
  from reward eligibility (which needs a scarce proof, F1).
- **D-1, D-2, D-3, D-7 — sound** (fix D-3's anti-phishing scope per F3; strengthen D-7's referral-weight
  enforcement per F2).

### Gate impact
- **Before GROW-S5 (rewards):** F1, F2, F7 resolved + a standalone red-teamed T1 reward spec + counsel review.
- **Before GROW-S2 (relay at scale):** F4, F5 mitigations designed; F5 capacity model.
- **Before any flagship 2k push:** F8 scale gate (200 → 2,000) passed.
- **GROW-S0/S1 (invite plumbing + web page) may proceed** — they carry no reward logic; keep copy
  contribution-framed, not recruit-to-earn (F2), and ship the signed-installer/verified-name work with S1 (F3/F6).
