---
created: 2026-08-31
branch: main
author: Claude (Opus 4.8), directed by @SaulBuilds
status: planset (Stage-1 draft)
planset: connect-realign
code: CONNECT
repo: citrate-core (+ citrate-comms relay for the server-blind claims inbox)
companions:
  - docs/CONNECTIONS_RUNBOOK.md
  - docs/adr/ADR-2026-08-30-social-identity-privacy-model.md
---

# Realigning people, groups & clusters — connection redesign

## Why this exists

Connecting a person today is painful and the pieces don't add up to a system:

- **Adding by @handle takes ~7 manual hops** across two apps and two people: owner types @handle →
  *Create invite* → **DM the `citrate://` link** on X/Discord → invitee **pastes the link** in-app →
  invitee **copies the claim** → invitee **DMs the claim back** → owner **pastes the claim** → *Accept*.
  It's entirely clipboard/DM driven — there is **no relay claims-inbox**, so the invitee's claim has to
  travel back by hand (`invites.rs:9-11`).
- **There is no people-picker or directory anywhere.** Every add starts from a raw `0x` address you
  already have, or a hand-typed `@handle`. Citrate never resolves a handle to an address.
- **X/Discord source nothing useful.** The OAuth link fetches *only the username string* (scopes
  `users.read`/`identify`); no followers, no friends, no contacts. So linking them verifies *your own*
  handle but gives the user nothing to act on — which is exactly why they feel pointless.
- **There is no "associate" concept.** Every group membership is independent; re-adding the same person
  to a second group repeats the whole dance. The only cross-group carryover is a cosmetic *face*.
- **There is no groups-with-role navigator.** Role context lives only inside the selected group's
  roster; there's no list of "my groups/clusters, where I'm owner/admin/member" to jump between.

## Core invariant

**Connecting a person is a pick and an approval — never a copy-paste of links or claims — and every add
is still relay-authorized and ceremony-signed.**

## Locked decisions

| # | Decision | Choice |
|---|---|---|
| D-1 | Reusable identity layer | **Associates** — a first-class, device-local list of people you've connected with (comms address + verified faces). Connect once, add to any group/cluster with one pick. |
| D-2 | Connect gesture | **One click + one approval.** The approval *is* the consent + authorization gate — not a bypass of it. |
| D-3 | Kill the DM-back | Invite claims post to the owner through the **server-blind relay** (a token-keyed connect/requests inbox), not the clipboard. Owner approves from an in-app **Requests** inbox. |
| D-4 | X/Discord role | A verified handle is your **face** (trust) + the **address & delivery channel** for invite-by-@handle. **No friend-list import** — X/Discord APIs don't expose it; we will not promise it. Social linking is optional-but-useful, never a dead-end "connect for nothing." |
| D-5 | Identity key | The **comms address** is identity; **consume the deferred wallet↔comms attestation** so "who is this / am I admin" is coherent app-wide, and retire the session-only `iCreated` hack. |
| D-6 | Security | Unchanged: **RBAC enforced at the relay** (ADR-001), all role changes / adds / bindings **ceremony-signed** (D3), **server-blind** (D1), cluster admits by `address ∈ roster`. |

## The model — before → after

**Add a known person to a group**
- *Before:* find their `0x` comms address out-of-band → Roster tab → paste → Add (presupposes you have the address and they've published a key package).
- *After:* open group → **pick them from your Associates** → *Add* → approve the RoleAssertion in the ceremony. **1 pick + 1 approval.**

**Connect a new person you only know by @handle**
- *Before:* the ~7-hop clipboard/DM round-trip above.
- *After:* owner *Invite → pick @handle / paste link once*; the invitee opens the link (or clicks in-app), their client **posts a signed claim to the relay under the invite token** (server-blind — encrypted to the invite's key, embedded in the link); the owner sees a **pending Request** and clicks **Approve** (ceremony-signed). **No claim copy-paste back. ~2 actions each side.** They're now an Associate, reusable everywhere.

**Add to a cluster**
- Unchanged in spirit (cluster admits by group roster), but exposed as a first-class **one-click Join / Add** in the navigator, and the standalone Cluster surface gets the Join action it currently lacks.

## The two lists you asked for

### 1. People — the Associates directory
A single view of everyone you've connected with, reusable across all groups/clusters.

- **Per associate:** avatar/initials, **name = verified face** (`@handle` with a `network ✓` badge) else short address; the comms address (mono); the **groups & clusters you share**; and, where relevant, **their role there** (e.g. "admin in Design").
- **Sources (no new social scraping):** (a) people you've mutually connected with; (b) **verified faces** ingested server-blind across any shared group (already stored device-local); (c) `@handle` you've invited. A person with a verified face shows as a real identity, not a hex string.
- **Actions (one click each):** *Add to group…* (picker of your groups where you can manage) · *Add to cluster…* · *Message* · *Remove*. Adds route through the owner-signed RoleAssertion → relay.
- **Discovery filters:** "shared with me", "verified faces", "pending requests".

### 2. Groups & Clusters — the role navigator
A consolidated switcher so the user can jump between everything they belong to, with role context.

- **Per entry:** group/cluster name, **your role badge** (Owner / Admin / Member), member count + cluster online count, and a **jump** (one click to open).
- **Filters:** **"Where I'm admin"** (owner or admin), "Where I'm a member", "Clusters". This is the "list of where the user has admin / where others have admins" — each entry shows *your* role, and opening it shows the full roster with each member's role.
- **Placement:** promoted into the **shell sidebar** as per-group entries under "Your Groups" (today it's a single flat "groups" link), plus a command-style quick-switcher. The standalone Cluster surface reads from the same navigator.

## Scope

**In (v1):** the Associates layer + directory; the server-blind relay claims-inbox + Requests approve flow; the people-picker for group/cluster add; invite-by-@handle with DM deep-link + auto-claim; verified faces surfaced in the picker; the Groups/Clusters role navigator + shell switcher + cluster Join; the wallet↔comms attestation consumed (retire `iCreated`).

**Out (follow-ons):** importing X/Discord friend graphs (API-restricted — not feasible); LinkedIn (confidential relay, separate); cross-org federation directory; on-chain anchoring of associates.

## Sprints

| Sprint | Goal | Key acceptance (data source) |
|---|---|---|
| **CONNECT-S0** | Associates model + People directory | An Associate persists device-local (comms addr + faces) and appears in a directory with shared groups/roles. Sourced from ingested verified bindings + membership overlap (`social.rs` bindings, `groups_roster`). |
| **CONNECT-S1** | Server-blind claims inbox (kill the DM-back) | An invitee's signed claim reaches the owner's **Requests** inbox via the relay (encrypted under the invite token); owner **Approve** = ceremony-signed RoleAssertion → member added. No clipboard round-trip. *(Needs a citrate-comms relay addition: a token-keyed, server-blind rendezvous.)* |
| **CONNECT-S2** | People-picker + one-click add | From a group/cluster, add an Associate in **1 click + approve**; invite-by-@handle mints a link + DM deep-link and auto-claims on connect. No raw-address paste required for known people. |
| **CONNECT-S3** | X/Discord repositioning | A verified handle renders as a face in the picker + directory; the Connections surface states each network's real job (face + reachability) and is optional, not a "pending backend" dead-end; **no false friend-import claim** (Rule 1). |
| **CONNECT-S4** | Groups/Clusters role navigator | A single list badges your role per group/cluster, filters "where I'm admin", and offers a shell quick-switcher; the Cluster surface gains a Join action. |
| **CONNECT-S5** | Identity-seam cleanup + harden | Consume the wallet↔comms attestation so identity/role is coherent app-wide; retire the session-only `iCreated`; tests + honest empty states; RBAC-at-relay unchanged. |

Dependencies: S1 depends on the citrate-comms relay inbox (the one genuinely cross-repo item). S2–S4 are citrate-core UI/state on top of S0/S1. S5 can run parallel.

## Security invariants (must hold)

- **Authorization at the relay, never the UI.** The picker and one-click adds only *offer* actions; the relay authorizes and returns honest errors (ADR-001). One-click ≠ auto-authorize.
- **Every add / role change / binding is ceremony-signed** (D3) — the human approves the RoleAssertion; the vault key signs only at `ceremony.approve`.
- **Server-blind (D1).** The claims inbox carries ciphertext keyed to the invite; the relay learns nothing; private links resolve to no one (D2).
- **Cluster admission stays roster-derived**; cross-machine mesh remains Rule-8 / soak-gated.
- **The approval is the consent gate** on both sides: the invitee consents by claiming; the owner authorizes by approving. Nothing is added silently.

## Honest limits (stated up front)

- **No X/Discord friend/contact import.** X's follower/following API is paid/restricted and Discord's relationships aren't available to apps — so "pick from your X friends" is not buildable. The achievable, honest win is **verified faces + invite-by-handle**, not a social-graph import.
- **The claims-inbox needs relay work** in citrate-comms (S1) — it's the one piece that isn't pure citrate-core.
- **The addressing seam is real:** identity is the comms address while invites/faces traffic in wallet/@handle; S5 closes it by consuming the wallet↔comms attestation, which also removes the `iCreated` UI hack.

## Acceptance criteria (tied to the pain)

- [ ] Add a known Associate to a group or cluster in **1 click + 1 approval** — no address/handle paste.
- [ ] Connect a new person with **≤ 2 actions each side and no claim copy-paste back** (relay inbox + Approve).
- [ ] A **people-picker** lists connectable people: Associates, shared co-members, verified faces, and by @handle.
- [ ] **X and Discord each have a stated, delivered purpose** (verified face + invite-by-handle) or are clearly optional — no "connect for nothing."
- [ ] A **People/Associates directory** and a **Groups & Clusters role navigator** exist, with role badges, a "where I'm admin" filter, and a shell quick-switcher.
- [ ] Every membership change still routes through **relay RBAC + ceremony-signed** assertions; server-blind preserved; no new key exposure.
