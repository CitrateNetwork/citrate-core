---
created: 2026-10-01
branch: hup/n4-learn-e2e
updated: 2026-10-04 (hup/n7-skill-publish-abi, review)
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Hermes verified self-learning (HUP-S3.4, US-3.4)

Hermes may keep a skill or a memory from its own work, under four rules:

1. **Only verified work.** A proposal must come from a workflow run whose verifiers all passed.
   The model saying it succeeded never counts.
2. **Nothing is kept without you.** Every proposal waits for Accept or Reject. Each decision is
   written to the sidecar's HIC decision log before anything is saved.
3. **Publishing is HIC-1.** Sending a saved skill to the on-chain SkillRegistry goes through the
   Signature Ceremony, one approval per publish. It is off for now (see below).
4. **Contradictions are surfaced, never merged.** A memory that disagrees with one you already
   have must be acknowledged, and then both are kept as Belnap `both` (unresolved) in the
   learned-memory ledger and linked by a quarantined `contradicts` edge in the memory graph.
   Nothing is merged or overwritten. You settle it yourself: "Keep this one" on a memory sets
   the others aside (see below). Until you do, memory recall and search leave the unresolved
   memories out, so Hermes does not recall a claim you have not settled.

## How it flows

| Step | Where | What happens |
|---|---|---|
| Teach | Agents, "What Hermes learned", "Teach Hermes" (`TeachHermesCard`, `learnLauncher.ts`) | You write a task and the phrases its answer must contain. The app opens a session on your local model and runs it as a one-step workflow with one `answer_contains` verifier per phrase. Only when every check passed does it ask Hermes, in the same session, whether anything is worth keeping; Hermes proposes through `learn_propose`. The session is closed afterwards. |
| Run | sidecar `POST /sessions/:id/workflows` (core `hermes_workflow_run`) | A declarative workflow runs in the session. Each step is judged by deterministic verifiers (tool succeeded or not called, answer mentions, JSON field equals, forge tests pass, SARIF below a severity, medusa no failures). The run is `verified` only when every verifier of every step passed. |
| Propose | sidecar `POST /learn/proposals` (core `hermes_learn_propose`), or Hermes's `learn_propose` tool | A skill (`SKILL.md`) or memory (`key`, `value`) is proposed from a verified run of that session. A session that read untrusted content cannot propose. |
| Review | Agents, "What Hermes learned" (`LearnedPanel`, `LearnProposalCard`) | The card shows the content, every verifier verdict, the judged attempts, the trajectory (session, message count, SHA-256), the model, and any conflicts. |
| Accept | `hermes_learn_accept` | Core sends the member id (your wallet address, or `local-member`) and the conflicts you ticked. A skill is written to `hermes/skills/<name>/SKILL.md`. A memory comes back to core as a typed record. |
| Reject | `hermes_learn_reject` | Recorded and final, also across restarts. |
| Resolve | `hermes_learn_resolve` (sidecar `POST /learn/memories/resolve`) | On a contradicted memory, "Keep this one" (or, against a memory you already had, "Set this one aside"), then confirm. One HIC-1 decision per memory set aside is written to the decision log before anything changes. |

## Where things live (app data folder)

| Path | Holds |
|---|---|
| `hermes/learn/decisions/` | The sidecar's HIC decision log for learning (append-only, hash-chained). |
| `hermes/learn/proposals.json` | Undecided proposals and recent decided ones, so a restart loses nothing. On start the decision log reconciles a file left one decision behind. |
| `hermes/skills/` | Accepted skills. Core also passes this folder to the sidecar as `CITRATE_HERMES_SKILLS`; the sidecar reloads it on accept, so a skill is offered without a restart: to new sessions, and to a session already open on its next turn (when that session was opened with skills; one opened with none gets it in the next session). |
| `hermes/learned-memories.json` | The learned-memory ledger, keyed by proposal id (accepting the same proposal twice is not a duplicate). |

## Memories and Belnap `both`

An accepted memory is stored in your memory graph (`memory.assert`, tenant `personal`). When it
contradicts an earlier learned memory:

- both ledger entries are marked `both`, each listing the other in `contradicts`;
- the new graph node is linked to the old one by a quarantined `contradicts` edge (advisory,
  never load-bearing until confirmed);
- the stored text says the memory contradicts an earlier one and is unresolved.

If the memory store is not running, the memory waits in the ledger as `pending` and the panel
offers "Store waiting memories". A store error is shown as `failed` with its reason.

### Resolving a contradiction

"Keep this one" on a `both` memory sets aside every learned memory it still contradicts, after
you confirm. For each one:

- the sidecar records the decision (`learn.memory.resolve`, HIC-1) and marks the other proposal
  `retracted`; it no longer counts as known, so Hermes may propose it again later and you decide
  again;
- the ledger marks it Belnap `false` ("Set aside"), kept for the record, never deleted;
- the kept memory becomes `true` once nothing else contradicts it. It is stored again with its
  settled text, and the new graph node supersedes the old nodes (a confirmed `supersedes` edge,
  which retires them in the memory graph);
- if the app missed the sidecar's answer, the next time the panel loads it syncs with the
  sidecar's proposal list and applies it. A memory left `both` with nothing to contradict is
  settled by that sync only when the sidecar shows it standing.

The rules are model-checked in citrate-agent-runtime `agent-learn/formal/ContradictionResolve.tla`,
which found two restart cases and an ordering case before they shipped.

### Against a memory you already had

A memory can also contradict one the app held before Hermes learned anything (passed to the
sidecar as a known memory; the ledger lists it as `memory:<id>`). The same two choices apply:

- "Keep this one" keeps the learned memory and sets the other aside. The sidecar records the
  decision and lists it under the learned proposal's `set_aside`; the ledger drops it from the
  learned memory's contradictions and settles the learned memory when nothing else contradicts
  it. When the id is a memory graph node, the settled node supersedes it.
- "Set this one aside" keeps the memory you already had and retracts the learned one (Belnap
  `false`, kept for the record).

A lost answer is applied by the next sync, as above. This path is covered by tests in both repos,
not by the TLA+ model.

### Recall while unresolved

Memory recall and search (the app's own and the read-only memory tools Hermes uses through MCP)
leave out every learned memory that is `both`, on both sides of the contradiction, and any node
whose stored text says it is unresolved. The ledger is read on every call, so a resolution shows
on the next one. If the ledger cannot be read, every learned memory is left out until it can.
`memory.neighbors` prints titles without node ids, so there only the stored text counts: a
neighbor whose text says it is unresolved is left out, but the older side of a contradiction
between two learned memories (stored as settled before the newer one contradicted it) can still
show there by its title.

## Publishing to the SkillRegistry

Core first pins the accepted `SKILL.md` to the local IPFS node (Storage), reads it back, and
checks it against the accepted content hash; the CID becomes the registry's `manifestCID` and the
payload must carry it. If IPFS is not running the publish stops with that reason. The sidecar
builds `registerSkill` calldata only and records the HIC-1 decision. Core checks the payload (target is the address-book SkillRegistry, owner is your wallet, chain 40204, no value,
no broadcast, `registerSkill` selector, calldata that encodes exactly the fields shown, and a
skill id equal to the registry's `skillHashOf(owner, name, version)`) and opens a pending
Signature Ceremony. Nothing signs outside the ceremony.

The skill id follows the redeployed SkillRegistry (citrate-chain PR #272):
`keccak256(abi.encode(owner, name, version))`. The earlier deployment used `abi.encodePacked`,
under which ("skill1", ".0") and ("skill", "1.0") share one id. A test decodes the publish
calldata against that contract's ABI JSON (`src-tauri/tests/fixtures/skill-registry/`) and checks
the selector, every argument and the id against Foundry (`cast call skillHashOf` on a local anvil
deployment).

**Off, pending owner sign-off** (`SKILL_PUBLISH_ENABLED = false` in `hermes_learn.rs`). The
recommended default is to turn it on once the redeployed SkillRegistry is live on 40204 and the
address book points at it; until then it stays off. On
2026-10-01 a read-only check found contract code with the `registerSkill` selector at the
address-book SkillRegistry address on 40204 (`totalSkills() = 0`). Whether that deployment is the
one members should publish to after the fresh-keys reroll is the owner's call. Until then the
publish button is disabled with that note; when it is turned on, a live `eth_getCode` check also
keeps it disabled if the registry is missing.

## Not done yet

- Publishing stays off pending owner sign-off and the SkillRegistry redeploy on 40204 (chain
  operator, federation #289).
- "Teach Hermes" checks answers with `answer_contains` only. Track workflows (forge, slither and
  the other toolchain verifiers) are not launched from this card.
- `memory.neighbors` lines are filtered by their text only (they carry no node id), so the older
  side of a contradiction between two learned memories can still show there by title.
- Nothing here was run in the packaged app yet (the teach card needs the local model running).
