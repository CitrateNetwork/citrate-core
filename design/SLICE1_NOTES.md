---
created: 2026-10-18
branch: beta/design-slice1-hermes
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: review (design slice 1, due 2026-10-23); chain-team review of 04e7b43 addressed
---

# Design slice 1: Hermes + Controls

Slice 1 of the 0.5.2 design prototype in `design/CitrateCore.dc.html`. It draws the five-place shell (Hermes, Groups, Wallet, Node, Store, plus Settings) with Hermes in full: Chat, Journal, Work, Learn and Controls at three depths. Groups, Wallet, Node and Store are labelled stubs. Store also has a stub install sheet so click path 2 can run.

Inputs, in order of authority: the C1 UI truth contract v1 (2026-10-19), the betanet plan (core plan K2, design team plan), then the existing tokens and design system. Following chain-team ruling 6, Hermes screens no longer show chain numbers. They appear only in Hermes chat replies (network status, rewards), where each reply carries the fixture pill and a footnote naming the export label, file and block.

## How to review

Serve the folder (`python3 -m http.server` in `design/`) and open `CitrateCore.dc.html#/hermes/chat`. Use the **Prototype** button (bottom right) to:

- pick a persona: P1 is a fresh install (empty states); P2 and P3 have daemons, approvals and records;
- run click path 2 with the checklist and "Go to step 1";
- switch light (charter) or dark (instrument);
- switch model routing between local only and ask first;
- hide the annotation strip.

The **annotation strip** at the top of every screen names the screen, its route id, its bridge calls and its genesis flag. On Hermes screens it also has the **state** switch (data, empty, error, unavailable). It is a design aid and is not part of the member build.

Deep links follow the K0 map: `#/hermes/<chat|journal|work|learn|controls>`, `#/groups`, `#/wallet`, `#/node`, `#/store`, `#/settings`. The 0.5.1 routes still work and open their new home: `dashboard` → hermes/chat, `agent` → hermes/work, `journal` → hermes/journal, `commissary` and `models` → store, `storage` and `files` → node, `comms`, `people`, `cluster` and `train` → groups, `connections` → settings.

## Screens

The bridge names were checked against `src/bridge/domains.ts`, `src/bridge/tauri/*.ts`, `src/daemons/api.ts`, `src/agent/*.ts`, `src/budgets/budgets.ts` and the invoke handler in `src-tauri/src/lib.rs` at b521d98. "None yet" means no command exists; the plan item that owns it is named in brackets.

| Screen | Route (today in Sidebar.tsx) | Bridge | Flag at genesis |
|---|---|---|---|
| Shell: sidebar, five places + Settings | `hermes`, `groups`, `wallet`, `node`, `store`, `settings` (today 17 NAVI ids; see deep links above) | Node line: `node.status`. Approvals dot: `agentHarness.pendingApprovals` | none |
| Hermes › Chat | `hermes/chat` (`dashboard`) | `agentHarness.sessionOpen / sessionSend / sessionEvents` (`hermes_session_open`, `hermes_session_send`, `hermes_session_events`), `chat.backend`. Ask before gateway: **none yet (K1, S7.5a)**. Rail: `daemons_list`, `hermes_pending_approvals`. Network status only as a chat reply (ruling 6): fixture in the prototype, live §2 reads **none yet (K3)** | none |
| Ask before gateway card (in Chat) | `hermes/chat` | **none yet (K1, S7.5a)**. The decision is recorded with `hic_record_decision` | none |
| Hermes › Journal | `hermes/journal` (`journal`) | none: local app state (`jPages`, `ensureDailyEntry`). Its chat uses the same Hermes session (`hermes_session_send`) | none |
| Hermes › Work: Awaiting your review | `hermes/work` (`agent`) | `agentHarness.pendingApprovals / resolve` (`hermes_pending_approvals`, `hermes_resolve`), `shellPending / shellDecide`, `mcpPending / mcpDecide` | none |
| Hermes › Work: Daemons | `hermes/work` | `daemonsApi.list / setPaused / setAllPaused / save` (`daemons_list`, `daemon_set_paused`, `daemons_set_all_paused`, `daemon_save`). The "awaiting your review" daemon state: **none yet (K2)** | none |
| Hermes › Work: Run log | `hermes/work` | `daemonsApi.runsBetween` (`daemon_runs_between`) | none |
| Hermes › Work: Agents, Skills | `hermes/work` | `agentHarness.status` (`hermes_status`), `agentSkills.list`, `agentHarness.registrySkills` (the SkillRegistry count was removed per ruling 6; it belongs in Store) | none |
| Hermes › Learn | `hermes/learn` (Agent and Settings) | `hermes_learn_status / proposals / accept / reject / memories`. Teach Hermes: `hermes_workflow_run` (learnLauncher.ts). Knowledge import: `memory_import_knowledge`. Training opt-in: `trajectories_settings_get / set` | none. Copy: "Group rounds not live yet" |
| Controls depth 1: six switches | `hermes/controls` (Settings › AI providers, Escalation, MCP, Web search, Budgets) | routing: **none yet (S7.5a)**; web search: `hermes_web_settings_get / set`; terminal: `hermes_terminal_get / set`; daemons: `daemons_set_all_paused`; learn: `trajectories_settings_set`; voice: store `hermesReadAloud` (local) | none |
| Controls depth 2: Providers and keys | `hermes/controls/providers` | `ai_provider_status`, `ai_set_provider`, `ai_clear_provider` (the ModelRegistry count was removed per ruling 6) | none |
| Controls depth 2: Escalation and spend caps | `hermes/controls/escalation` | `escalation_endpoints`, `escalation_endpoint_add / remove`, `escalation_budget`, `escalation_budget_set` | none |
| Controls depth 2: MCP servers | `hermes/controls/mcp` | `hermes_mcp_settings`, `hermes_mcp_set`, `mcp_servers_runtime` | none |
| Controls depth 2: Web search | `hermes/controls/web` | `hermes_web_settings_get / set`; managed Chromium via `components.status` | the browser needs the component signing key from the ceremony (`keyConfigured`) |
| Controls depth 2: Terminal and folders | `hermes/controls/terminal` | `hermes_terminal_get / set`, `agent_grants_view / add_folder / revoke` | none |
| Controls depth 2: Budgets | `hermes/controls/budgets` | `daemon_save` (Budget); web sign-in budgets: `web_budget_status / grant / revoke` | none |
| Controls depth 2: Personas and voice | `hermes/controls/personas` | `hermes_personas`, `hermes_persona_check`; read aloud: `hermesReadAloud` | none |
| Controls depth 3: Approval records | `hermes/controls/records` | writes: `hic_record_decision`. Read: **none yet (K2)**: `HicLog::records` exists in hic_records.rs but no command serves it | none |
| Controls depth 3: Spend ledger | `hermes/controls/spend` | `escalation_budget` (history, capMicros, usedMicros, confirmedMicros); `daemons_list` (spendTodaySalt) | none |
| Controls depth 3: Daemon run history | `hermes/controls/runs` | `daemon_runs_between` | none |
| Groups (stub) | `groups` (groups, people, cluster, train, comms) | groups.*, cluster.*, flRounds.* (later slice) | "Group rounds not live yet" |
| Wallet (stub) | `wallet` | wallet.*, membership.grantStatus (slice 2) | none |
| Node (stub) | `node` (node, storage, files) | node.status, K1 registry (slice 2) | K6 network switch |
| Store (stub) and Store install sheet stub | `store` (commissary, models) | **none yet (K5)**: signed catalog and install path; install writes `daemon_save` with paused | catalog key from the ceremony |
| Settings (trimmed) | `settings` (settings, connections) | `auth.status / userinfo`, `membership.entitlement`, `membership.grantStatus` (grant record), `connections.status`, `custody.status`, `config.read / write`. Price: one source, mirroring core `src/data/membership.ts` (#349) | none |

Each stub can open the 0.5.1 screen under a red "reference, not a port target" bar, so core can see what is being replaced.

## States drawn

- **Model routing:** local only (the gateway is never used) and ask before gateway (an HIC-1 card in Chat with "Use gateway once" or "Keep it local"; both decisions go to the approval records).
- **Daemons:** paused, scheduled, running, budget used up today, awaiting your review, all paused from Controls, and "needs web search, which is off".
- **Spend:** the ledger is at zero by default (cap $0.00, so every escalation asks). Dollar amounts are the provider's list price on the member's own key and never sit next to a SALT amount. Daemons say "Daemons can't spend SALT" instead of "0 SALT". P3 shows one confirmed escalation so the populated table is visible.
- **Empty, error and unavailable** for Chat, Journal, Work, Learn and each Controls depth. Empty uses the screen's own empty states (a fresh install). Error and unavailable name the bridge call and what it returned, and show unknown, never zero. In Controls the depth switcher stays visible.
- **Light and dark** (charter and instrument registers) for every screen.

## Click path 2 (everyday)

Status: **works end to end in the prototype, with the Store hop stubbed.**

1. Hermes › Controls › Web search on. A nudge appears: "Daemons that use it are in the Store."
2. Store (slice 2 stub): one first-party listing, "Morning web brief", with declared permissions (web search, journal write that asks every time) and a declared budget. The signature check is marked "slice 2".
3. Install paused. The daemon arrives in Hermes › Work as **paused**, "installed from Store".
4. Start. If web search is off, or all daemons are paused, it refuses with a reason. It runs once and finishes with a journal page **awaiting your review**.
5. Approve. The page is added to the Journal, the daemon goes back to scheduled, and the decision shows in Controls › Records.

The Prototype panel tracks the five steps and can reset the path.

## UI truth contract decisions

Revised after the chain-team review of 04e7b43 (B1 to B4):

- **B1, fixture label.** The Hermes rail's Network widget and the SkillRegistry and ModelRegistry counts were removed (ruling 6). The only fixture numbers left in Hermes are in chat replies. Each such reply carries the "fixture" pill and a footnote with the verbatim export label ("FIXTURE — captured from 40204, not live"), the file (`fixtures/40204-2026-10-19.json`) and the read block (499,058). "1 produces blocks" is labelled "chain team, sampled 2026-10-19" until the v1.1 export adds `producing_validators_1000`.
- **B2.** The Node stub and the 0.5.1 Node tab say "Rewards", never "Earnings".
- **B3.** The rewards reply follows §1 Rewards: no "today", no estimates, no projections, fixture numbers labelled inline. The network reply labels fixture reads separately from the sampled number. The bond reply uses core's `grantCopy.ts` wording (#349).
- **B4.** Settings › Memberships & billing uses the one grant explanation from `grantCopy.ts`, plus the bond state word "Bond locked · not staked", the lock end shown as unknown, the §5.7 disclaimer and the §5.6 betanet line. The price comes from one place, which mirrors core's `src/data/membership.ts` (the membership service's checkout price; there is no price endpoint yet), with "It buys membership. It does not buy SALT." The onboarding and Commissary prices read the same source.
- No finality as running. The sidebar uses the §1 node words, and there is no locally ticking height.
- No SALT price, and no USD value of SALT.
- Reviewer notes also addressed: N1 (onboarding now sits under a red "0.5.1, not a port target" bar), N3 ("price" became "cost" in the escalation copy), N12 (moot, because the counts were removed).
- Open for slice 2: N4, N5, N7, N8, N9, N10 and N11.

## Known debt outside slice 1

These are left for the slices that own them and are not port targets:

- Onboarding S0 to S6 still has pre-contract copy ("32,000 SALT staked to your validator", "Chain 40204 · testnet-beta"). The join path is redrawn in slice 2.
- The 0.5.1 Wallet, Node, Storage, Comms and Commissary screens (behind the reference bar) and the `tick()` simulation that feeds them still show V2 to V9 behaviour.

## Chain-team rulings (answered 2026-10-19, binding for 0.5.2)

1. **Producing validators in the export: yes.** v1.1 adds `producing_validators_1000`. Until then the prototype labels it "chain team, sampled 2026-10-19".
2. **Pill plus footnote: yes.** Every group of fixture numbers carries the pill, and the footnote names the fixture file and block. A number without a visible pill is a violation.
3. **Hermes quoting fixture numbers:** only in the prototype, labelled inline. In the shipped app, Hermes states a chain number only from a live §2 read, names its source, and says it can't read it when the read fails.
4. **USD for the member's own API keys: allowed.** It is shown at the provider's list price, labelled as that, and never next to a SALT amount.
5. **"0 SALT spend": don't show it.** Write "Daemons can't spend SALT".
6. **Chain numbers in Hermes:** only where a Hermes action needs them (approvals that move SALT, the member's own bond or reward state when asked, network status in the composer). The rail widget and registry counts were removed.
7. **Sample member bond:** the rehearsal-1 export includes one (principal, activated, unlockBlock, unlockTimestamp, isKycVerified, validator state). v2 draws "Locked until ⟨date⟩ (block N)" from it.

## Open questions for core

1. **Terminal default.** `src/agent/terminalAccess.ts` says terminal access is on by default (owner decision 2026-10-04). K2 says "terminal stays off". The prototype draws it off. Which is right?
2. **Routing default and local only with no model.** The prototype defaults a fresh install to "Ask before gateway". With "Local model only" and no verified local model, ModelRouter INV-Router-2 falls back to the gateway, which contradicts the switch. S7.5a needs an unavailable state for that case.
3. **What triggers ask-first?** The prototype uses a keyword stand-in for "the local model wants the gateway". What signal does S7.5a use?
4. **Approval origin.** `AgentApproval` has no daemon id or session origin, and `DaemonView.status` has no "awaiting review". Can core add `origin` / `daemonId` to approvals, so Work can link a daemon to its pending change?
5. **Journal writes.** `AgentApproval.kind` is code, chain or shell. Which kind is a daemon's journal page? Should the enum add journal or file?
6. **Reading HIC records.** `hic_record_decision` writes and `HicLog::records` exists, but no command reads the records. Depth 3 needs one (for example `hic_records_list`).
7. **Feature flags.** The app has no genesis feature-flag mechanism, so most screens say "none". Do we need flags, for example for the web browser component key or the Store catalog key?
8. **"Learn from my sessions".** The prototype maps it to trajectory consent (`trajectories_settings_set`), and proposals still need a yes. Should it also turn on agent-learn proposals from sessions, or only the recording?
9. **Voice.** Read aloud (`hermesReadAloud`) and dictation are separate today. Should one switch cover both?
10. **Start a daemon.** `daemon_set_paused(false)` only schedules. In the prototype, Start also runs once now, so the path can be reviewed. Is there, or should there be, a run-now command?
11. **Install from the Store.** `DaemonInput` has no `paused`, `source` or declared-requirements field (for example "needs web search"). The install path needs all three.
12. **Persona names.** The prototype doesn't show shipped persona names (they come from `hermes_personas`, some pending sign-off). Should v2 show them?
