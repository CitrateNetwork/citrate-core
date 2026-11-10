---
created: 2026-10-18T00:00:00Z
branch: beta/k0-five-place-nav
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: active
wp: 0.5.2 K0, K2 (Hermes Work, Learn, Controls depths 2 and 3)
---

# Five-place route map

Version 0.5.2 groups the app into five places (Hermes, Groups, Wallet, Node,
Store) plus Settings, which keeps only account-level things.

**Compatibility rule: nothing is deleted. Old routes keep working as deep
links and open the new home.** Every route id the app used before, and every
Settings section, resolves to a canonical route. `store.go`, the sidebar and
the hash router all go through one function, `resolveRoute` in
[`src/shell/places.ts`](../../src/shell/places.ts). That file is the source of
truth; this page describes it. Tests:
[`src/shell/places.test.ts`](../../src/shell/places.test.ts) (every redirect,
written out by hand) and
[`src/shell/placeView.render.test.tsx`](../../src/shell/placeView.render.test.tsx)
(each place renders).

## Places and tabs

| Place | Tabs (canonical route) |
|----|----|
| Hermes | Chat `hermes/chat` · Journal `hermes/journal` · Work `hermes/work` · Learn `hermes/learn` · Controls (sub-tabs 1 · Switches `hermes/controls`, 2 · Details `hermes/controls/details`, 3 · Records `hermes/controls/records`) |
| Groups | Chat `groups/chat` (sub-tabs Groups `groups/chat`, Alerts `groups/chat/alerts`) · Rounds `groups/rounds` · Members `groups/members` · Cluster `groups/cluster` · ALF `groups/alf` (ALF members only) |
| Wallet | `wallet` (no tabs) |
| Node | Status `node/status` · Services `node/services` · Storage (sub-tabs Files `node/storage/files`, Memory `node/storage/memory`, Pins `node/storage/pins`, Backup `node/storage/backup`) · Rewards `node/rewards` · Advanced `node/advanced` |
| Store | Apps `store/apps` · Models `store/models` |
| Settings | Settings `settings` · Connections `settings/connections` |

The Node tab is named Rewards, per the chain team's UI truth contract §1.
An earlier draft called it Earnings; `node/earnings` redirects to `node/rewards`.

A bare place id (`hermes`, `store`) or a tab with sub-tabs (`node/storage`)
opens its first leaf. `groups/community` is routable but in no tab strip, as
Community was hidden from the old sidebar (its data is not wired).

## Old route ids

| Old route id | Opens | Place › tab |
|----|----|----|
| `dashboard` | `hermes/chat` | Hermes › Chat |
| `agent` | `hermes/work` | Hermes › Work (the approvals badge moves to Hermes) |
| `journal` | `hermes/journal` | Hermes › Journal |
| `groups` | `groups/chat` | Groups › Chat › Groups |
| `comms` | `groups/chat/alerts` | Groups › Chat › Alerts |
| `people` | `groups/members` | Groups › Members |
| `cluster` | `groups/cluster` | Groups › Cluster |
| `train` | `groups/rounds` | Groups › Rounds |
| `alf` | `groups/alf` | Groups › ALF (members only) |
| `community` | `groups/community` | Groups (hidden leaf) |
| `wallet` | `wallet` | Wallet |
| `node` | `node/status` | Node › Status |
| `files` | `node/storage/files` | Node › Storage › Files |
| `storage` | `node/storage/memory` | Node › Storage › Memory |
| `pins` | `node/storage/pins` | Node › Storage › Pins (K4) |
| `backup` | `node/storage/backup` | Node › Storage › Backup (K4) |
| `commissary` | `store/apps` | Store › Apps |
| `models` | `store/models` | Store › Models |
| `settings` | `settings` | Settings (keeps the last account-level section) |
| `connections` | `settings/connections` | Settings › Connections |
| `node/earnings` (earlier draft name) | `node/rewards` | Node › Rewards |

## Settings sections (`#/settings/<id>`)

Each section is addressable as `settings/<id>`. The ones that moved redirect
to their new home and open on that section.

| Section | Opens | Place › tab |
|----|----|----|
| `settings/hermes` (Hermes controls, the Controls panel) | `hermes/controls`, section `hermes` | Hermes › Controls |
| `settings/ai` (Providers and keys) | `hermes/controls/details`, section `ai` | Hermes › Controls › Details |
| `settings/escalation` (Escalation and spend caps) | `hermes/controls/details`, section `escalation` | Hermes › Controls › Details |
| `settings/mcp` (MCP servers) | `hermes/controls/details`, section `mcp` | Hermes › Controls › Details |
| `settings/web` (Web search and decisions) | `hermes/controls/details`, section `web` | Hermes › Controls › Details |
| `settings/terminal` (Terminal and folders, from App) | `hermes/controls/details`, section `terminal` | Hermes › Controls › Details |
| `settings/budgets` (Budgets) | `hermes/controls/details`, section `budgets` | Hermes › Controls › Details |
| `settings/personas` (Personas and voice, from App) | `hermes/controls/details`, section `personas` | Hermes › Controls › Details |
| `settings/node` (Node configuration) | `node/advanced`, section `node` | Node › Advanced |
| `settings/account` | `settings`, section `account` | Settings |
| `settings/connections` | `settings/connections` | Settings › Connections |
| `settings/api` | `settings`, section `api` | Settings |
| `settings/keys` | `settings`, section `keys` | Settings |
| `settings/billing` | `settings`, section `billing` | Settings |
| `settings/privacy` | `settings`, section `privacy` | Settings |
| `settings/app` | `settings`, section `app` | Settings |

The design's depth-two ids also work: `hermes/controls/providers`, `/escalation`,
`/mcp`, `/web`, `/terminal`, `/budgets` and `/personas` open Details on that
section.

Settings now lists only Account and RBAC, Connections, API endpoints and keys,
Keys and security, Memberships and billing, Privacy and recovery, and App. If
a moved section was the last one open, Settings opens on Account.

## Tab contents in this build

This change is navigation and composition. Each tab renders an existing
surface unchanged:

| Tab | Content |
|----|----|
| Hermes › Chat, Journal | Dashboard, Journal surfaces |
| Hermes › Work (K2) | `HermesWork`: the Agent surface (roster, skills, Awaiting your review, contracts) with the Daemons card and the daemon run log in its main column. The chat, learning panel and MCP switches moved to Chat, Learn and Controls › Details. The Work tab carries the approvals badge. |
| Hermes › Learn (K2) | `HermesLearn`: What Hermes learned (with Teach Hermes), knowledge import, training opt-in. |
| Hermes › Controls › 1 · Switches | The Controls panel (`src/surfaces/hermes/ControlsPanel.tsx`). Each row's Details opens its depth-two section. |
| Hermes › Controls › 2 · Details | The Settings sections for Hermes, unchanged components, under a "Details" heading: Providers and keys, Escalation and spend caps, MCP servers (Hermes's connected tools plus the member's own servers), Web search and decisions, Terminal and folders, Budgets, Personas and voice. |
| Hermes › Controls › 3 · Records | `ControlsRecords`: approval records (web sign-in decisions; Hermes approval records have no read command yet), spend ledger (`escalation_budget`), daemon run history (`daemon_runs_between`). |
| Groups › Chat, Alerts, Rounds, Members, Cluster, ALF | Groups, Comms, Train, People, Cluster, ALF surfaces |
| Wallet | Wallet surface |
| Node › Status (K3) | `Node` (`src/surfaces/Node.tsx`): one sentence in UI truth contract §1 words, Start and Stop, this node's reads, the network rows, and the validator card (grant story, recorded activation, Activate validator through the one gate). |
| Node › Services (K1a-2) | `NodeServices`: the services registry snapshot (`services_snapshot`). |
| Node › Storage › Files, Memory, Pins, Backup | StorageFiles, Storage, StoragePins and StorageBackup surfaces (K4) |
| Node › Rewards (K3) | `NodeRewards` (`src/surfaces/node/NodeRewards.tsx`): `ValidatorRegistry.rewardsOf` for the member's validator key, Claim through the ceremony, pin and round rewards not live yet. |
| Node › Advanced (K3) | `NodeAdvanced` (`src/surfaces/node/NodeAdvanced.tsx`): network identity, withdrawn-branch notice, the last crash record, the full node log, then the Node configuration Settings section. |
| Store › Apps, Models | Commissary and Models surfaces |
| Settings, Settings › Connections | Settings (account-level sections) and Connections surfaces |

Tabs with no content in this build render a "Not available yet" state that
says what is missing and, where one exists, links to what this build has
today. They show no data:

- None in the Node place since K3 (Rewards, Advanced) and K4 (Pins, Backup).

## Follow-ups

- Copy elsewhere still names old places in prose, for example "Settings >
  Connections" (still correct) and agent prompts that mention Settings for
  the sidecar loop (App section, still correct). The one user-facing pointer
  to a moved section, the escalation "no endpoint" message, now names
  Hermes › Controls.
- In-app links to a moved section use its `settings/<id>` route so they land
  on the new home. Connections › Cloud providers links to `settings/ai`
  (Hermes › Controls, AI providers). No caller links to plain `settings` for
  a moved section.
- Hermes › Controls has two writers of the saved web settings (the Web search
  switch on 1 · Switches and the Web search form on 2 · Details). Each re-reads
  before it writes and changes only its own fields. After either saves, it
  announces what core saved (`src/agent/webSettingsSync.ts`); any other view
  mounted at the same time shows it and re-baselines, so neither shows or
  diffs against a stale copy. Each view's initial read goes through
  `readWebSettingsUnlessSuperseded`, which drops an answer that arrives after
  another view's save. A tripwire test
  (`src/agent/webSettingsReaders.tripwire.test.ts`) keeps every subscribing
  view on that path. The MCP switches save the same way (re-read, change the
  pressed switch, write).
- The Hermes `app_navigate` tool still takes old route ids. They redirect, so
  it keeps working; a later change can teach it the canonical routes.
