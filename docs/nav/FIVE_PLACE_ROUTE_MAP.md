---
created: 2026-10-18T00:00:00Z
branch: beta/k0-five-place-nav
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: active
wp: 0.5.2 K0
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
| Hermes | Chat `hermes/chat` · Journal `hermes/journal` · Work `hermes/work` · Learn `hermes/learn` · Controls `hermes/controls` |
| Groups | Chat `groups/chat` (sub-tabs Groups `groups/chat`, Alerts `groups/chat/alerts`) · Rounds `groups/rounds` · Members `groups/members` · Cluster `groups/cluster` · ALF `groups/alf` (ALF members only) |
| Wallet | `wallet` (no tabs) |
| Node | Status `node/status` · Services `node/services` · Storage (sub-tabs Files `node/storage/files`, Memory `node/storage/memory`, Pins `node/storage/pins`) · Earnings `node/earnings` · Advanced `node/advanced` |
| Store | Apps `store/apps` · Models `store/models` |
| Settings | Settings `settings` · Connections `settings/connections` |

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
| `commissary` | `store/apps` | Store › Apps |
| `models` | `store/models` | Store › Models |
| `settings` | `settings` | Settings (keeps the last account-level section) |
| `connections` | `settings/connections` | Settings › Connections |

## Settings sections (`#/settings/<id>`)

Each section is addressable as `settings/<id>`. The ones that moved redirect
to their new home and open on that section.

| Section | Opens | Place › tab |
|----|----|----|
| `settings/ai` (AI providers) | `hermes/controls`, section `ai` | Hermes › Controls |
| `settings/escalation` (Escalation and spend) | `hermes/controls`, section `escalation` | Hermes › Controls |
| `settings/mcp` (MCP servers) | `hermes/controls`, section `mcp` | Hermes › Controls |
| `settings/web` (Web search and decisions) | `hermes/controls`, section `web` | Hermes › Controls |
| `settings/budgets` (Budgets) | `hermes/controls`, section `budgets` | Hermes › Controls |
| `settings/node` (Node configuration) | `node/advanced`, section `node` | Node › Advanced |
| `settings/account` | `settings`, section `account` | Settings |
| `settings/connections` | `settings/connections` | Settings › Connections |
| `settings/api` | `settings`, section `api` | Settings |
| `settings/keys` | `settings`, section `keys` | Settings |
| `settings/billing` | `settings`, section `billing` | Settings |
| `settings/privacy` | `settings`, section `privacy` | Settings |
| `settings/app` | `settings`, section `app` | Settings |

Settings now lists only Account and RBAC, Connections, API endpoints and keys,
Keys and security, Memberships and billing, Privacy and recovery, and App. If
a moved section was the last one open, Settings opens on Account.

## Tab contents in this build

This change is navigation and composition. Each tab renders an existing
surface unchanged:

| Tab | Content |
|----|----|
| Hermes › Chat, Journal, Work | Dashboard, Journal, Agent surfaces |
| Hermes › Controls | The five moved Settings sections, unchanged, under a "Controls" heading. They write the same config as before. The Controls toggle panel replaces this slot. |
| Groups › Chat, Alerts, Rounds, Members, Cluster, ALF | Groups, Comms, Train, People, Cluster, ALF surfaces |
| Wallet | Wallet surface |
| Node › Status | Node surface (its Operations, Earning and Pinning views) |
| Node › Storage › Files, Memory | StorageFiles and Storage surfaces |
| Node › Advanced | The Node configuration Settings section |
| Store › Apps, Models | Commissary and Models surfaces |
| Settings, Settings › Connections | Settings (account-level sections) and Connections surfaces |

Tabs with no content in this build render a "Not available yet" state that
says what is missing and, where one exists, links to what this build has
today. They show no data:

- **Hermes › Learn**: points to Hermes › Work, where learned-skill proposals are reviewed today.
- **Node › Services**: points to Node › Status.
- **Node › Storage › Pins**: opens the Pinning view on Node › Status.
- **Node › Earnings**: opens the Earning view on Node › Status.

## Follow-ups

- Copy elsewhere still names old places in prose, for example "Settings >
  Connections" (still correct) and agent prompts that mention Settings for
  the sidecar loop (App section, still correct). The one user-facing pointer
  to a moved section, the escalation "no endpoint" message, now names
  Hermes › Controls.
- The Hermes `app_navigate` tool still takes old route ids. They redirect, so
  it keeps working; a later change can teach it the canonical routes.
