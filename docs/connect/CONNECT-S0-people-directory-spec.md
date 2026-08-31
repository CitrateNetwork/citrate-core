---
created: 2026-08-31
branch: feat/connect-s0-people-directory
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (Stage-1)
planset: connect-realign
sprint: CONNECT-S0
repo: citrate-core
companions:
  - docs/CONNECT_REALIGN_PLANSET.md
---

# CONNECT-S0 — People directory (derived associates)

## Outcome (one sentence)

A member opens a **People** view and sees everyone they share a group with — shown by their verified
name (@handle/face) where available — with the groups they have in common and each person's role, so
they stop dealing in raw `0x` addresses and have a foundation to act on people (add/message) in later
sprints.

## Why this first

S0 is the read-only backbone the rest of CONNECT builds on. It introduces **no new storage and no new
trust surface** — it is a live aggregation of data the app already has (group rosters + verified faces).
The persistent "associate you don't yet share a group with", the connect flow, and one-click actions
are deliberately later sprints; S0 makes the *directory* real and honest first.

## Data sources (Rule 7 — traced before behavior)

| Value shown | Source |
|---|---|
| The set of people + their role | `bridge.groups.list()` → for each `bridge.groups.roster(groupId)` → `GroupMember { address, role }` (`comms.rs` `groups_list`/`groups_roster`) |
| Display name (face) | `bridge.social.resolve(addresses)` → `ResolvedIdentity { address, network, handle }` — verified, group-visible bindings only (`social.rs` `social_resolve`) |
| "You" (exclude self) | the member's own comms/wallet address from `store.identity()` |
| Shared groups + role per person | computed by joining the rosters above, keyed on `address` |

**No fabrication.** Every row corresponds to a real roster membership; a person with no verified face
renders as a short address, never an invented name. Zero groups → an honest empty state.

## User stories

1. As a member, I want **one list of people I share groups with**, so I don't open each group's roster separately.
2. As a member, I want each person shown by their **verified @handle/face** when available (not a hex string), so I recognize them.
3. As a member, I want to see **which groups I share** with each person and **their role** there, so I have context.
4. As a member with **no groups yet**, I want an honest empty state that points me to create/join a group, not a fake list.
5. As a member, I want to **search/filter** the list by name/handle/address/group, so I find a person fast.
6. As a member, I want each person row **designed to host actions** (add to group, message) that light up in S2 — so the directory is the jumping-off point, without pretending those actions work yet.

## Acceptance criteria (each names its source + verifying test)

- **AC1** — The People view lists the **union of distinct member addresses** across every group returned by
  `bridge.groups.list()` (via `bridge.groups.roster`), **excluding the member's own address**. *Test:*
  `peopleDirectory.test.ts` — stubbed groups+rosters with overlap yield one row per distinct non-self address.
- **AC2** — Each row's display name is the **verified handle** from `bridge.social.resolve` (with a `network ✓`
  marker); an unresolved address falls back to `shortAddr`. *Test:* a resolved address renders `@handle`, an
  unresolved one renders `0x…`.
- **AC3** — Each row shows the **shared group names + the person's role in each**, aggregated (a person in two
  shared groups appears once, listing both). *Test:* a person across two groups shows both with their roles.
- **AC4** — With **zero groups or zero shared members**, the view renders an **honest empty state** and no person
  rows (Rule 1). *Test:* empty `groups.list()` → empty-state copy, zero rows.
- **AC5** — A **search box** filters the aggregated rows client-side by handle / address / group name. *Test:* a
  query narrows the visible rows to matches only.
- **AC6** — A **"People"** entry appears in the sidebar (`SECTIONS`, "Your Groups") and routes to the surface.
  *Test:* the sidebar IA test includes a `people` id routing to the People surface.
- **AC7** — Row action affordances (Add to group / Message) render **disabled with an honest "coming in the next
  step" affordance** (or are absent), never a control that silently no-ops. *Test:* actions are not clickable-live in S0.

## BDD scenarios (branching behavior)

```gherkin
Scenario: A person resolves to a verified face
  Given I share the "Design" group with address 0xAA
  And 0xAA has a verified, group-visible X handle "@dana"
  When I open People
  Then 0xAA's row shows "@dana" with an X ✓ marker, not the raw address

Scenario: A person has no verified face
  Given I share a group with address 0xBB which has no verified binding
  When I open People
  Then 0xBB's row shows a shortened 0xBB… address and no ✓ marker

Scenario: A person shared across multiple groups
  Given address 0xCC is in both "Design" (admin) and "Ops" (member) with me
  When I open People
  Then 0xCC appears once, listing "Design · admin" and "Ops · member"

Scenario: No groups yet
  Given I belong to no groups
  When I open People
  Then I see an empty state inviting me to create or join a group, and no person rows
```

## Out of scope (S0 — named, not implied)

- Persistent associates you do **not** currently share a group with — created by the connect flow (S1/S2).
- **One-click add / message** actions doing real work (S2).
- The **connect / claims-inbox** and the server-blind relay rendezvous (S1).
- **X/Discord repositioning** and any social-graph reads (S3) — S0 only *displays* faces that already resolve.
- The **Groups & Clusters role navigator** and shell quick-switcher (S4).
- Closing the wallet↔comms addressing seam (S5).

## Simulation boundary

**None.** S0 reads only real bridge data. In web/sim mode the app's existing honest-empty/persona bridge
applies — the People view shows the sim's honest-empty state, never fabricated people. There is no
`RealXxx`-returns-hardcoded backend here: the aggregation is pure over live `groups`/`social` reads, and
a test disconnecting those sources yields an empty directory, not stale rows.

## Security / invariants preserved

- Read-only: S0 adds no write, no signature, no new key surface.
- Faces come only from **verified, group-visible** bindings (`social_resolve` already enforces D1/D2/D3);
  S0 never resolves private links or unverified handles.
- No relay authorization is bypassed — S0 shows what the member already has access to (their own rosters).
