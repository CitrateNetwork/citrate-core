---
created: 2026-10-01
branch: hup/n4-learn-e2e
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
   Nothing is merged or overwritten. The app does not yet hide a contradicted memory from
   memory recall, and there is no screen to resolve one yet.

## How it flows

| Step | Where | What happens |
|---|---|---|
| Run | sidecar `POST /sessions/:id/workflows` (core `hermes_workflow_run`) | A declarative workflow runs in the session. Each step is judged by deterministic verifiers (tool succeeded or not called, answer mentions, JSON field equals, forge tests pass, SARIF below a severity, medusa no failures). The run is `verified` only when every verifier of every step passed. |
| Propose | sidecar `POST /learn/proposals` (core `hermes_learn_propose`), or Hermes's `learn_propose` tool | A skill (`SKILL.md`) or memory (`key`, `value`) is proposed from a verified run of that session. A session that read untrusted content cannot propose. |
| Review | Agents, "What Hermes learned" (`LearnedPanel`, `LearnProposalCard`) | The card shows the content, every verifier verdict, the judged attempts, the trajectory (session, message count, SHA-256), the model, and any conflicts. |
| Accept | `hermes_learn_accept` | Core sends the member id (your wallet address, or `local-member`) and the conflicts you ticked. A skill is written to `hermes/skills/<name>/SKILL.md`. A memory comes back to core as a typed record. |
| Reject | `hermes_learn_reject` | Recorded and final, also across restarts. |

## Where things live (app data folder)

| Path | Holds |
|---|---|
| `hermes/learn/decisions/` | The sidecar's HIC decision log for learning (append-only, hash-chained). |
| `hermes/learn/proposals.json` | Undecided proposals and recent decided ones, so a restart loses nothing. On start the decision log reconciles a file left one decision behind. |
| `hermes/skills/` | Accepted skills. Core also passes this folder to the sidecar as `CITRATE_HERMES_SKILLS`, so an accepted skill is offered in sessions after the next Hermes start. |
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

## Publishing to the SkillRegistry

The sidecar builds `registerSkill` calldata only and records the HIC-1 decision. Core checks the
payload (target is the address-book SkillRegistry, owner is your wallet, chain 40204, no value,
no broadcast, `registerSkill` selector) and opens a pending Signature Ceremony. Nothing signs
outside the ceremony.

**Off, pending owner sign-off** (`SKILL_PUBLISH_ENABLED = false` in `hermes_learn.rs`). On
2026-10-01 a read-only check found contract code with the `registerSkill` selector at the
address-book SkillRegistry address on 40204 (`totalSkills() = 0`). Whether that deployment is the
one members should publish to after the fresh-keys reroll is the owner's call. Until then the
publish button is disabled with that note; when it is turned on, a live `eth_getCode` check also
keeps it disabled if the registry is missing.

## Not done yet

- No app flow launches a verified workflow yet; the commands and bridge methods exist
  (`workflowRun`, `workflowStatus`, `learnPropose`), and Hermes can propose through its
  `learn_propose` tool after a verified run.
- Resolving a `both` contradiction (keeping one claim) has no UI yet.
- An accepted skill joins a running Hermes only after it restarts.
- No IPFS pin of a skill bundle, so a publish registers an empty manifest CID ("pending pin").
