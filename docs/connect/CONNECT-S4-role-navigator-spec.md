---
created: 2026-08-31
branch: feat/connect-s4-role-navigator
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (implemented)
planset: connect-realign
code: CONNECT-S4
repo: citrate-core
companions:
  - docs/CONNECT_REALIGN_PLANSET.md
  - src/surfaces/groupsNavigator.ts
  - src/surfaces/People.tsx
  - src/surfaces/Cluster.tsx
---

# CONNECT-S4 — groups & clusters role navigator

## The user & the outcome

From one place, a member sees **every group they're in badged with their own role**, can filter to
**"where I'm admin"**, and **jumps to any group or its cluster in one click** — no hunting through the
Groups rail to remember where they own vs. just belong. The Cluster surface gains a **Join** action so
a member can join the selected group's mesh without leaving it.

## Why this step exists

Role context lived only inside the selected group's roster; there was no single "my groups/clusters,
where I'm owner/admin/member" list to navigate by (the last gap the realignment named). And the Cluster
surface could only *select* among clusters, never *join* one — the join primitive existed
(`joinCluster`, `bridge.cluster.join`) but was unused by the UI.

## Data-source trace (Rule 7)

| Shown | Source |
|---|---|
| Groups you're in + kind | `bridge.groups.list()` (id, name, kind) — the real daemon list |
| Your role per group | that group's roster (`bridge.groups.roster`) → your seat, keyed on your **comms address** (`bridge.groups.selfAddress`), read in the same pass as the People directory (`store.refreshPeople`) |
| "where I'm admin" | `iManage = role ∈ {owner, admin}` OR created-this-session (`groupsSlice.names`), the same seam the Groups surface uses |
| Join / Leave a cluster | `joinCluster` / `leaveCluster` → `bridge.cluster.join` / `.leave` (session `joined` set; live peers are real daemon state) |

Note: on the packaged build `groups.list()` returns `owner:""` and `members:[]`, so role is read from
each roster (authoritative), **not** from the DTO — a role that hasn't loaded stays `null` → "—",
never guessed.

## Acceptance criteria

- [x] A single list badges your role per group (`buildRoleNavigator`, unit-tested; `s.myGroups` derived
  live in `store.refreshPeople`). Rendered in the People surface under "Your groups & clusters".
- [x] A "Where I'm admin" filter (`managedGroups`) shows only groups you own/admin; offered only when
  you manage at least one.
- [x] One-click quick-switch: **Open** (`selectGroup` → Groups) and **Cluster** (`selectClusterGroup` →
  Cluster) per row.
- [x] The Cluster surface gains a **Join / Leave** action for the selected group (`joinCluster` /
  `leaveCluster`), with honest copy and an honest error on an un-provisioned daemon.
- [x] Never fabricated (Rule 1): a group with no loaded role shows "—"; empty state says you're in no
  groups yet.
- [x] Tests: `groupsNavigator.test.ts` (9) + `peopleNavigatorHonesty.test.tsx` (5). Suite 406 (+14).

## Also in this step (honest-copy cleanup)

- People's per-person "Add to group" button was a **disabled** control claiming "lands in the next step
  (CONNECT-S2)" — S2 shipped. It now routes to Groups (where the people-picker adds in one click), and
  the footnote is corrected. No dead/misleading affordance remains.

## What this step does NOT change

- No new bridge method; `bridge.groups` / `bridge.cluster` are untouched. RBAC stays enforced at the
  relay/daemon; the navigator only decides which controls to show and where to jump.
- The "quick-switcher" is the People-surface navigator (a list you click to jump), not a global
  command palette — that would be a separate, larger surface.

## Out of scope (v1)

- Cross-machine cluster onboarding (separate CL-S3 lane).
- A shell-level (topbar) command palette.
