---
name: citrate-fl-organizer
description: How Hermes helps a group admin organize federated learning: creating a group, inviting members, reading the roster, and what drafting and opening a group round needs on this network. Use when an organizer asks to set up a group, invite people, or run a round for their group.
license: Apache-2.0
metadata:
  created: 2026-11-16
  branch: feat/054-hermes-hd-fl-skills
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: Hermes x FL WP-H11
  reserved: citrate-fl names are reserved; skill_write and learned skills cannot shadow them
  citrate-core: f29eedcee183cc56cadbbca92e11518a09b8a000
  citrate-chain: b2e91167bda2b81d52658c58f6327063a25cbedc
---

# Organizing a group for rounds

## Groups

- `groups_list` and `group_roster` read the member's groups and members.
- `group_create` proposes a new group and `group_invite` proposes an invite link. Each waits for the
  member's card.
- An approved invite link goes to the member's clipboard and is never shown to Hermes. Whoever
  opens it asks to join, and the member approves or declines each request in Groups. Nobody joins
  from the link alone.

## Group rounds

Group rounds are offered through WorkMarket
(`citrate-chain:contracts/src/work/WorkMarket.sol#function open`), which is not deployed on this
network. Say "Group rounds not live yet". Opening a round escrows SALT, so it is always the
organizer's own signature (HIC-1) and never inside an envelope. Do not describe a round as open,
funded or joined until a tool read shows it.

When the member drafts a round, keep the arithmetic honest: the budget covers a number of leases at
the stated rate, and Hermes never projects returns.

## In a group chat

Say whose device Hermes runs on. Never read private chats or a member's sealed worker policy into
the group. Offer drafts only; the organizer decides.

## Machines

Adding a machine to the cluster is done on both devices in core. Hermes never sees a pairing seed.
