---
created: 2026-10-19
branch: beta/design-slice2
author: Larry Klosowski + Claude Opus 5.5 (MAC lane)
status: review (design slice 2, due 2026-10-28); chain-team review signed at 6d80821; slice 1 752e8e5 merged and Codex P2s at 6d80821 addressed 2026-10-22
---

# Design slice 2: Node, Store, web join and download

Slice 2 of the 0.5.2 design prototype in `design/CitrateCore.dc.html`, on top of slice 1 (`SLICE1_NOTES.md`). It draws Node (core plan K3, K4), the Store (K5), the web join and download pages (GROW-S1 §3 and §4), and a redrawn 0.5.2 onboarding with the new Activate validator step. Click paths 1 and 3 run end to end, and click path 2's Store hop is now the real Store listing and install sheet.

Inputs, in order of authority: UI truth contract v1 as of citrate-chain #319 at f366a7be and the chain-team rulings (see `SLICE1_NOTES.md`), the betanet plan (K1, K3, K4, K5, join path, design team plan), the GROW-S1 join design §3 and §4, SPEC-001 and ADR-0001 for service states, then the tokens and design system.

## How to review

Serve the folder (`python3 -m http.server` in `design/`) and open `CitrateCore.dc.html`. A fresh install now opens the 0.5.2 onboarding. The **Prototype** button has:

- **Start path 1 (fresh P1)**: opens `citrate.ai/join/7KQ2M9XD` in a browser frame and tracks seven steps.
- **Start path 3 (P2)**: opens Node › Services with IPFS Quarantined and tracks four steps.
- Click path 2 (unchanged steps; step 2 is now the Store listing).
- Shortcuts to the join page, the download page, and the 0.5.1 onboarding (reference only).

The annotation strip works as in slice 1 (screen, route, flag, bridge, and the data, empty, error and unavailable switch), with one more row of state switches where a screen has more states:

- Node › Status and Advanced: every node word from contract §1 (Off, Starting, Syncing, Syncing · not checked yet, Synced, Sync stalled, No peers, Update required, Resync required, Error), or live. Status also switches the validator word (Not a validator, Bond locked, Registered, Validator active).
- Node › Services: all running, or IPFS quarantined.
- Store: the catalog state, either **no key yet (the 0.5.2 default)** or the **post-ceremony** variant. In the post-ceremony variant, the listing and install sheet also switch signature: verified, failed, or tier-locked.
- Join page: the seven invite states, verified or unverified inviter, OS (macOS, Linux, Windows, phone), and the Windows build (not built, unsigned).
- Download page: OS and Windows build.

Deep links: `#/node/status`, `#/node/services`, `#/node/storage/files|memory|pins`, `#/node/rewards`, `#/node/advanced`, `#/store`, `#/store/catalog/<apps|daemons|widgets|skills|models|mcp>`, `#/store/listing/<id>`, `#/store/installed`, `#/web/join/<code>`, `#/web/download`. The 0.5.1 ids still work: `storage` and `files` open Node › Storage › Files, `models` opens Store › Models, `commissary` opens the Store.

Prototype state moved to `citrate-core-proto-v4` in localStorage, so saved slice 1 state does not carry over.

## Screens

Bridge names were checked against `src/bridge/domains.ts`, `src/bridge/tauri/*.ts` and the invoke handler in `src-tauri/src/lib.rs` on this branch. "None yet" names the plan item that owns the missing command.

| Screen | Route | Bridge | Flag at genesis |
|---|---|---|---|
| Node › Status | `node/status` (today `node`) | `node_status` (its `syncPct` is never drawn), `node_start`, `node_stop`. One read for the node words (eth_syncing, local and public head, peers): **none yet (K3)**. Validator word: `node_proposer_identity`, then `validatorInfo` / `isActive` / `MemberBond.activated()`: **none yet (K3)** | K6: "Update required" at genesis |
| Node › Services | `node/services` | Registry snapshot: **none yet (K1, SC1, SC2)**. Today per service: `node_status`, `agent_status`, `memory_status`, `model_serve_status`, `ipfs_status`, `hermes_status`, `cluster_status`; start and stop pairs for node, memory, model serve, IPFS and Hermes. Logs: `node_logs` only; the other eight **none yet (SC4)**. `node_last_crash` | none |
| Node › Storage › Files | `node/storage/files` (today `storage`, `files`) | `storage_add`, `storage_list`, `storage_retrieve`. Export and backup: **none yet (K4)** | none |
| Node › Storage › Memory | `node/storage/memory` | `memory_status`, `memory_constellation`. Export: **none yet (K4)** | none |
| Node › Storage › Pins | `node/storage/pins` | `storage_pin(cid, "0")` with no bond, `storage_unpin`. `IPFSIncentivesV3.getPin` / `getChallenge`: **none yet (K4, S2.2)**. Bond parameters: C1 fixture | pin bonds: the pin-bond contract funded at genesis (S2.2) |
| Node › Rewards | `node/rewards` | `node_validator_earnings` (rewardsOf for the proposer key). Validator claim: **none yet** | none; pin and round rewards not live |
| Node › Advanced | `node/advanced` (today `node`, Settings › Node) | `config_read`, `config_write` (node.toml), `node_logs`, `node_last_crash` (the last record only; a list: **none yet**), `NodeStatusView.notice` from `node_genesis.rs` and `node_withdrawn.rs` | K6: "Update required at genesis"; the withdrawn-branch mover |
| Store › Catalog | `store/catalog[/<kind>]` (today `commissary`, `models`) | 0.5.2 default fails closed: "Store catalog not available yet: signing key not issued" (core PR #356, `KeyNotIssued`). Post-ceremony: signed catalog fetch and verify **none yet (K5)**; today `commissary_catalog`. Models: `model_catalog_local / search / download / select`. Skills: `SkillRegistry.totalSkills()` (fixture). MCP: `hermes_mcp_settings / set` | catalog key from the key ceremony (K5) |
| Store › Listing | `store/listing/<id>` | Listing and signature: **none yet (K5)**. Daemon install: `daemon_save`, which needs paused, source and declared requirements (slice 1 question 11). Tier: `membership.entitlement`, fail closed | catalog key (K5) |
| Store install sheet | over `store/listing/<id>` | Signature, hash and staging: **none yet (K5)**. The decision is recorded with `hic_record_decision` (kind `store.install`) | catalog key (K5) |
| Store › Installed | `store/installed` | Installed manifest: **none yet (K5)**; `daemons_list`, `model_catalog_local`, `model_delete`, `hermes_mcp_settings` | none |
| Web join page | citrate-landing `app/join/[code]` | `resolveCode` on the `join_codes` row; `GET /api/join/<code>` and `/api/join/jwks`: **not live yet (GROW-S1)**. OS: `detectOs`, `provenArch`, `Sec-CH-UA-Arch` (`src/lib/download.ts`). App side: `handleDeepLink` for `citrate://join/<code>#K`, kept in the Rust store: **none yet (Join 2)**; redeem: `group_invite_redeem` | join service live (GROW-S1, S1b) |
| Web download page | citrate-landing `app/download/get` | CDN `downloads/manifest.json`, `SHA256SUMS`, `SHA256SUMS.minisig`: **none yet (GROW-S1 §4.1)**; fallback constants in `download.ts` | Windows Authenticode signing; release minisign key |
| Onboarding 0.5.2 (8 steps) | `onboarding/<step>` (today Onboarding.tsx S0 to S6) | Sign in: `auth_login`. Verify: `kycStart`, `userinfo`. Membership: `membership.checkout`, `membership.entitlement`. Bond: `wallet_ensure_ready`, `membership_grant_status`; MemberBond reads **none yet (C4)**. Node: `node_start`, `node_status`. Activate: `node_arm_mining`, `node_proposer_identity`, `node_register_validator` through the signature ceremony. Join: `group_invite_redeem` | Activate: registration on the genesis registry, bond model per C4. Join: GROW-S1 and GROW-S2 |
| Hermes › Chat rail: your group | `hermes/chat` | `groups_list` (the joined group) | none |

Every screen above has its empty, error and unavailable variant. Errors and unavailable states name the call and what it returned, and show unknown, never zero.

## Node words and service states

- **Status** is one sentence in contract §1 words: the node word and the validator word, for example "Synced · Validator active". It carries no peer number: the fixture's `peer_count` (3) belongs to the public RPC node, so it appears only in its own row, "Public RPC node's peers", with the fixture tag. This node's direct peers read "— · not read". No height ticks locally, there is no sync percentage, and nothing says final.
- **Validator words.** After activation, the word is "Registered · active from epoch N", where N = `validatorInfo(pubkey).actEpoch`. The prototype has no per-validator read, so it draws "Registered · active from epoch —"; it never says "next epoch". "Validator active" requires `isActive(pubkey)` and `actEpoch ≤ currentEpoch()`, with the note that consensus follows the epoch snapshot and can lag by up to an epoch.
- **Resync required** appears only when this node has applied block 365,785, its RPC returned that block, and the hash differs from the canonical checkpoint. Below that height, or on a null or error response, the word is "Syncing · not checked yet", and Advanced › Withdrawn branch shows "not checked yet · —". The checkpoint values in Advanced are labelled "expected (bundled release)".
- **Network rows**: Registered validators (`activeCount`, which includes registrations pending activation), Eligible validators (live registry), Consensus validator set ("unavailable until a snapshot RPC exists", no number), and Producing validators over a stated window.
- **This node** rows say where each value comes from. The prototype has no live local reads, so local height is drawn as a relation ("within 5 blocks of the head", "N blocks behind") rather than a made-up number. "Producing blocks" is "— · not read" because no command scans proposers yet.
- **Services** use the two SPEC-001 axes: process phase (Stopped, Starting, Alive, Backoff, Failed, Stopping, Quarantined) and readiness (Not assessed, Awaiting, Ready, Stale, Unavailable). Node, node agent, memory and IPFS are process-only, so they read Not assessed when Alive. Llama chat, Llama embed and Hermes have HTTP probes; Comms and Cluster have connect-only probes.
- **Quarantined** shows the reason and what is still owned. Restart stays disabled until cleanup actually finishes, matching SPEC-001: recovery needs the old work to end and a fresh explicit start, never a timer. In the prototype the held `ipfs init` exits about three seconds after you open the log.
- **Node agent.** Its phase is its own (`svc.agent`), and its prerequisite is the node being Alive. While the node isn't Alive, the agent reads Stopped. Starting it then is refused with "the node is not Alive" in its log, and nothing claims it started. Stop and Start change what the row and detail show.
- Each service shows its probe, prerequisites, log budget (256 KiB or 500 lines, 16 KiB per line), crash files (two of 16 MiB) and module.

## Click paths

**Path 1 (join): works end to end.**

1. `citrate.ai/join/7KQ2M9XD#…` (verified inviter, direct invite).
2. Download for Mac. The page flips to "After installing, open Citrate once, then click Open invite."
3. Open invite in Citrate. The onboarding carries the invite: "You'll join Orchard Lab once setup finishes."
4. Sign in, verify identity, membership, wallet and bond, run your node until Synced.
5. Activate validator: the node arms its block producer, then the signature ceremony shows `MemberBond.activate()`. The result is "Registered · active from epoch —" (N is `actEpoch`, not read in the prototype).
6. Join Orchard Lab with one explicit click.
7. Land in Hermes › Chat with a greeting and the group card in the rail; Groups shows the group as joined.

**Path 2 (everyday): works end to end on the post-ceremony Store variant.** "Go to step 1" switches the Store to that variant, because the 0.5.2 default fails closed and lists nothing; a reload returns to the fail-closed default. Controls › Web search on, then Restart Hermes (Hermes reads web search when it starts, as core #341 shows), then "Find daemons in Store" opens the Morning web brief listing. Install paused opens the install sheet (signature, hash, stage, declared permissions and budget). Approve, start it from Work, approve its first change.

**Path 3 (operator): works end to end.** Node › Services shows IPFS Quarantined. Open its log; once cleanup finishes, restart it (Starting, then Alive · Not assessed). Storage › Pins pins a file on this node with "No bond. Pin bonds not live yet."

Each Store daemon declares what it writes (`writes`: a new page, today's journal, or nothing), and a run proposes exactly that for approval. Group thread digest proposes its summary for today's journal; before, only the web brief and web daemons did.

## Revised 2026-10-22

- Merged slice 1 at 752e8e5. That brings in the one pending ask-first turn, reload-safe daemon runs, saved-vs-effective Controls, the K0 hash router, Privacy & recovery in Settings, and no price anywhere (N-a).
- The hash router (`resolveHash`) now carries slice 2's routes on the K0 map:
  - `node/<page>[/<sub>]`, `store/<page>[/<id>]`, `web/join/<code>` and `web/download`;
  - K0's `store/apps` and `store/models` open the catalog on that kind;
  - `storage` opens Node › Storage › Memory and `settings/node` opens Node › Advanced (core `places.ts`);
  - `node/earnings` opens Rewards.
- Codex P2s at 6d80821, with tests in `scripts/design-prototype.test.mjs`:
  - the node agent's displayed phase now follows its own state;
  - Store daemons declare their output.
- Chain-team notes at 6d80821:
  - N-c: the Store resets to the fail-closed default on load;
  - N-d: "No pin is in these states yet";
  - N-e: the Settings comment says grantCopy.ts is planned.
- Follow-ups from core #393: on Linux, “Copy the install command” now copies the command with the invite key (before, it only said it had); the secondary actions sit beside the primary instead of inside its conditional, so “Pay for membership now, set up later” (phone) and “Email me when it’s ready” (Windows with no build) render.
- Join 3 copy review V-03 (citrate-federation JOIN3_COPY_REVIEW.md): the Verify step said “Membership needs a verified identity”, which contradicts the app and ADR-2026-07-25. It now says verification is optional for membership and unlocks features that need a verified identity, including withdrawing an unlocked bond (“Unlocked · verify identity to withdraw”), and the step offers Skip for now. Tests in `scripts/design-prototype.test.mjs`, with a tripwire on the old line.

## UI truth contract decisions

- **Fixture**: `fixtures/40204-2026-10-19.json` as re-exported in citrate-chain #319 at f366a7be. It was captured 2026-10-19 06:32 UTC, from head 510,030 to 510,047, in epoch 510. Each read is latest state at its own `observed_head` (`consistency: none`). Every group of numbers carries the pill and a footnote with the file, revision, capture time and head range; rows name their `observed_head`. Numbers shown: network head 510,030, block time ~2 s, 5 registered and 5 eligible validators (0 pending activation), 1 producing validator over blocks 509,033–510,032, base fee 1 gwei, epoch 510, minimum stake 32,000 SALT, block reward 10 SALT, public RPC peers 3, chain id and genesis, the two checkpoints, pin bond 10 SALT and reward 4 SALT over 4 rounds, 0 skills and 1 model. No consensus-set number is shown. The epoch length (1,000 blocks) is from contract §2, not the export, and is shown with blocks and minutes ("~33 min"). The Hermes chat replies (slice 1) use the same figures and words.
- **Peers**: the export's `peer_count` is the public RPC node's. It has its own labelled row and never appears in the status sentence.
- **Rewards** (never Earnings): Validator rewards, Claimable now and Maturing from `rewardsOf`, all "—" because the export has no per-validator read. An unregistered key is drawn as "Not a validator", not as 0. Pin and round rewards say "Pin bonds not live yet" and "Group rounds not live yet". No projections or per-day figures, and no "today": the producer count is "in the fixture sample, over blocks 509,033–510,032".
- **Grant**: one explanation (core `grantCopy.ts`, planned in PR #349), "Bond locked · not staked", lock end unknown, with the §5.7 disclaimer and §5.6 betanet line.
- **No in-app price** in the 0.5.2 onboarding: the Membership step's button is "See membership pricing", which opens pricing and checkout in the browser. "It buys membership. It does not buy SALT." stays.
- **Store**: the 0.5.2 default fails closed with core PR #356's exact string, "Store catalog not available yet: signing key not issued". Nothing is listed and nothing can be installed. The verified catalog is drawn only as the labelled post-ceremony variant. "Daemons can't spend SALT". No prices on listings.
- **Download page**: sizes and SHA-256 are the published v0.5.1 GitHub release assets, labelled "sample manifest", standing in for `downloads/manifest.json`. The copy says "Checksums published; signature coming": the checksums aren't signed yet. Only Apple notarization proves where the macOS build came from.
- **Web join page**: never shows member counts or member names. Unverified inviters get a generic card with neither the inviter's name nor the group's. The privacy line is on every state: "This page doesn't know who you are. The part of the link after # never leaves your browser." The key is masked on screen and copied in full only by the copy buttons.

## Open questions

For core:

1. **Status sentence.** K3 says "synced to finalized height N". The contract bans finality words, so the prototype draws "Synced" plus direct peers and the validator word. Confirm.
2. **Services snapshot.** Which command serves the K1 registry snapshot (phase, readiness, receipts, failures)? Today each service has its own status call, and IPFS returns only a bool.
3. **Logs for eight services.** Only the node has `node_logs`. SC4 bounds capture for all nine; is a per-service log read in 0.5.2 scope?
4. **Stuck quarantine.** If the held process never ends, what can the member do? The prototype offers nothing beyond waiting. Is "Quit Citrate" the documented answer?
5. **Crash records list.** `node_last_crash` returns only the last record. Advanced draws a list; does core add one?
6. **No-bond pin.** Does `storage_pin(cid, "0")` skip the signature ceremony? The prototype says "No signature needed".
7. **Validator claim.** There is no command to claim validator rewards. Is it in 0.5.2?
8. **Activation inside onboarding.** `node_arm_mining` restarts the node with `--mine`. Should onboarding show that restart as its own state, and can a member skip activation and do it later from Node › Status (the prototype allows both)?
9. **Store installs.** What does "install" mean for an SDK listing (a kit folder, or only the install command)? `DaemonInput` still needs paused, source and declared requirements (slice 1 question 11). Which command lists what the Store installed?
10. **Explore free with an invite.** Can a free member join the inviter's group? The prototype keeps the invite but skips the join.
11. **Invite in the Rust store.** `handleDeepLink` must keep `{code, K}` until used (GROW-S1 §3.3). Confirm the step that consumes it is the explicit Join click.

Answered by the chain team (review of 37c5d36):

12. "Local pin · no bond" is accepted; the chain team adds it to §1.
13. The rehearsal-1 export adds one member node's peers, height and `rewardsOf`.
14. Activation sponsorship is undecided, so the ceremony keeps "base fee 1 gwei (fixed)" and doesn't say "sponsored".
15. `EPOCH()` is 1,000 blocks per §2; the export will add it. "~33 min" at the measured 2 s is accepted.

For web (GROW-S1):

16. **Unverified invites.** §2.7 hides the inviter and group only in the link preview; the prototype hides them on the page too. The chain team accepted the stricter version; web to confirm.
17. **Variant mapping on the join page.** The annotation strip maps empty to unknown, error to expired, and unavailable to resolver unavailable. The seven states are also on their own switch.
18. **Team ID in public copy.** The macOS line names Team DDHUG44QC7 as in GROW-S1 §4.2. Confirm it may appear on the public page.

## Known debt

- The 0.5.1 onboarding stays reachable from the Prototype panel as a reference, under its red bar.
- Groups and Wallet are still stubs (Groups shows the joined group from path 1).
- The 0.5.1 Node and Commissary screens remain behind the reference bar and still show the V2 to V9 behaviour listed in contract §8.
