---
created: 2026-08-30
branch: feat/adr-social-identity-privacy
author: Claude (Opus 4.8), directed by @SaulBuilds
status: accepted (Stage-1) — owner-ratified 2026-08-30
supersedes: none
companions:
  - docs/CONNECTIONS_RUNBOOK.md (§1 Social identity)
  - docs/CX_SURFACES_USER_STORIES.md (§7 Social discovery)
  - docs/adr/ADR-2026-07-26-oauth-redirect-and-token-custody.md (the MCP OAuth pattern this reuses)
---

# ADR — Social identity privacy model (Connections · social discovery)

## Context

The Connections surface lets a member link **X / LinkedIn / Discord** so people in their groups can
recognize and reach each other, and so invites can become "add @handle" instead of "paste a 0x
address". This is the front end of the growth/gamification loop (referral → signed roster assertion
→ milestone reward, see the runbook §7).

The sensitive primitive is the **binding between a wallet address and a real social identity**. Get
it wrong and Citrate becomes a doxxing engine: a public, permanent, scrapeable map from pseudonymous
on-chain addresses to real people. This ADR fixes the privacy model **before any code is written**
(the runbook flagged it as the gate for all social work). It reuses the desktop OAuth + OS-keyring
custody pattern already accepted in ADR-2026-07-26 (MCP connections); it does **not** re-decide OAuth
mechanics.

The owner ratified four load-bearing choices on 2026-08-30. This ADR records them and their mechanics.

## Decision

### D1 — Storage: local, source-of-truth on device; shared server-blind, never on-chain
The address↔identity binding lives **on the user's device** as the source of truth. It is shared
**only** to members of a group the user is in, over the **same ciphertext-only (server-blind) relay**
the comms layer already uses (the relay never sees plaintext). There is **no central plaintext
store**, and the binding is **never written on-chain** by default.

- The relay only ever transports an encrypted, group-scoped blob; it cannot read the binding.
- On-chain publication is available **only** as an explicit, per-link, one-time action a user chooses
  (for someone who *wants* a public provable identity) — it is never the default and never implicit.
- **Consequence — device loss:** because the device is source of truth, losing it means **re-linking**
  (re-proving ownership). Group members retain their shared copy until the user re-establishes or
  revokes. This is an accepted trade for keeping no server-side plaintext honeypot.

### D2 — Visibility: private by default; opt-in widen to groups-only; never public unless chosen
A freshly-linked identity is visible to **no one** until the user explicitly widens it. The only
widen target is **groups-only** (visible to people who share a group with them). There is **no
public visibility** unless the user takes the explicit on-chain action in D1.

- Visibility is **per link** (X can be groups-only while LinkedIn stays private).
- Widening is reversible at any time; narrowing to Private takes effect immediately and stops future
  sharing (see Revocation).
- Rosters/peer lists/message senders show a linked identity **only** when the viewer is inside the
  visibility scope; otherwise they show the recognizable address fallback (initials avatar + `0x…`).

### D3 — Verification: full proof (OAuth ownership + wallet-signed binding via the ceremony)
A **"verified"** badge requires BOTH:
1. **OAuth proof** that the user controls the social account (the account-ownership half), and
2. A **wallet-signed challenge** binding the resolved handle to the wallet address, recorded as a
   **signed `IdentityBinding`** that passes through the **Signature Ceremony** (the human approves it;
   the wallet — not a sidecar — signs; Rule 3).

Self-typed handles are permitted for display but are shown **"unverified"** and are never treated as
proven. Only a full-proof binding earns the verified badge and is eligible to back invites/roster
faces to others.

### D4 — Discoverability: claimable invite; addresses are never resolved or leaked
Inviting **@handle** creates a **claimable invite request** the handle-owner accepts. The invitee's
`0x` address is **never resolved or revealed to the inviter** — not before acceptance, not after a
decline, not via any lookup. There is **no directory** that maps handle→address for scraping.

- Accepting a claimable invite lands the invitee in the group's roster as a **signed roster
  assertion** (the same primitive Community counts) — the address becomes known to the group at that
  point because they've joined, not because it was looked up.
- Declining reveals nothing and leaves no trace the inviter can mine.

## The data model (normative)

A **LinkedIdentity** record (on-device; the shareable form is the encrypted blob):

```
LinkedIdentity {
  network:      "x" | "linkedin" | "discord"
  handle:       string            // the display handle, e.g. "@dana"
  verified:     boolean           // true only for a full-proof binding (D3)
  visibility:   "private" | "groups"   // default "private" (D2)
  binding?:     IdentityBinding   // present iff verified
  linkedAt:     unix
}

IdentityBinding {                 // the signed proof (D3); never contains a token
  network:      string
  handle:       string
  address:      string            // the member's wallet address
  nonce:        string            // one-time challenge nonce
  sig:          string            // wallet signature over {network,handle,address,nonce}
  proofRef:     string            // opaque ref to the OAuth ownership proof (NOT the token)
}
```

Invariants:
- The **OAuth access/refresh token never enters** the LinkedIdentity or the binding — it seals in the
  OS keyring exactly as ADR-2026-07-26 requires; only the fact of a proof (`proofRef`) is recorded.
- The binding is **verifiable** by a third party in the same group (they can check `sig` over the
  claimed `{handle, address}`), which is what makes a roster face trustworthy — without any central
  authority and without publishing anything.

### The resolver (how faces appear)
Rosters, peer lists, and message senders call a **visibility-gated resolver**: given an address and
the viewer's context, it returns the LinkedIdentity **only if** the owner's visibility scope includes
the viewer (D2) and the binding is verified (D3). Otherwise it returns the address fallback. The
resolver runs on-device against shared group blobs — there is no server round-trip that could leak.

### Revocation
Unlinking (or narrowing to Private) removes the local record and **stops sharing**; group members are
sent a tombstone over the relay so their cached copy is dropped, and — because the group secret rotates
on membership/roster changes anyway — a departed member cannot retain a readable copy. An on-chain
binding (D1 opt-in) cannot be unpublished; the UI says so **before** the user takes that action.

## What we will NEVER do (data-minimization tripwires)

1. Never store the address↔identity binding in **server-readable plaintext**.
2. Never write a binding **on-chain by default** or implicitly.
3. Never **resolve or reveal** an address from a handle to anyone but the handle-owner (D4).
4. Never show a **"verified"** badge without both OAuth proof **and** a wallet-signed binding (D3).
5. Never let a **social token** cross the bridge or enter app state (keyring-only, per ADR-2026-07-26).
6. Never default a link to any visibility wider than **Private** (D2).

## Threat model (what this protects against)

| Threat | Mitigation |
|---|---|
| Mass de-anonymization / scraping | No directory; addresses never resolved from handles (D4); no server plaintext (D1) |
| Impersonation / spoofed identity | Verified requires OAuth + wallet-signed binding through the ceremony (D3) |
| Relay operator reading identities | Server-blind: relay only carries ciphertext, group-scoped (D1) |
| Accidental public exposure | Private by default; public only via explicit, warned, on-chain opt-in (D2) |
| Token theft via the app | Tokens seal in the OS keyring; never cross the bridge (D1/ADR-2026-07-26) |
| Stalking an ex-member | Revocation tombstone + group-secret rotation drop cached copies |

## Alternatives considered (and rejected)

- **Relay-published encrypted binding** (persists server-side, survives device loss): rejected as the
  default — it creates an encrypted honeypot and a larger data-at-rest surface. Device-loss re-linking
  is the accepted cost of keeping nothing server-side. (May revisit as an opt-in backup later.)
- **On-chain IdentityBinding attestation** (maximally verifiable): rejected as the default —
  handle↔address would be public and permanent. Retained only as an explicit per-user opt-in.
- **OAuth-only verification** (no wallet binding): rejected — a shared/compromised session could bind
  the wrong wallet; the wallet signature is what ties the identity to *this* member.
- **Opt-in discoverability directory** (handle→address resolvable for opted-in users): rejected for
  v1 in favor of claimable invites — a directory is a scraping surface even when opt-in. May revisit.
- **Public-by-default** visibility: rejected outright — incompatible with a pseudonymous chain.

## Consequences

- **For build:** the social layer needs a `social` bridge domain mirroring `ConnectionsDomain`
  (+ `verified`/`visibility` fields), `social_start` / `social_verify` / `social_disconnect` /
  `social_set_visibility` commands, the signed-`IdentityBinding` path through the ceremony, the
  visibility-gated resolver, and the claimable-invite flow (which reuses the group roster-assertion
  path). The Connections surface already renders the opt-in → verify → visibility affordances,
  currently flagged "pending backend" — this ADR is what un-gates them.
- **For the user:** stronger privacy, slightly more friction (private default means an explicit widen
  before faces show; device loss means re-linking). Both are intentional.
- **@rule8:** the IdentityBinding is a signing surface (the wallet signs the challenge) and touches
  identity — it routes through the ceremony and gets security sign-off before the social layer ships.

## Implementation surface (for the sprint that follows)

1. `social` bridge domain + tauri commands (`social_*`), sim honest-empty.
2. `IdentityBinding` build + ceremony route (wallet signs the challenge; Rule 3).
3. OAuth ownership proof reusing the ADR-2026-07-26 loopback-PKCE + keyring path (X + Discord public
   PKCE; LinkedIn via the hosted token-exchange relay).
4. Visibility-gated resolver consumed by Groups roster, Cluster peers, Groups messages.
5. Claimable-invite flow (invite @handle → pending request → accept → signed roster assertion).
6. Connections surface: wire the currently-flagged social rows to the above.

This ADR is **Stage-1 (owner-ratified)**. It becomes Stage-2 after a red-team pass on the resolver +
revocation + claimable-invite flows before the social layer ships.
