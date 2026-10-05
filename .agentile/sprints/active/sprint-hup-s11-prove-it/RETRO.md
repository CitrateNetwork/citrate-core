---
created: 2026-10-04T00:00:00Z
branch: hup/n7-docs-almanac-retro
author: Larry Klosowski + Claude Opus 5.5
status: draft (program retro; final at sprint close, when v0.5.0 ships)
type: retrospective
sprint: HUP program close (S0 to S11, planset 2026-09-30-hermes-upskill)
planset: 2026-09-30-hermes-upskill
wp: HUP-S11.4
---

# Retro: the Hermes upskill program (HUP), toward v0.5.0

This is the program retro that HUP-S11.4 asks for. It is written as a **draft on 2026-10-04**,
during fan-out 7, before the stacks merge and before v0.5.0 ships. The sections marked "at close"
are filled when the release is cut and the sprint moves to `completed/`. Until then this file is
the program-level view, and the per-run retros below stay the record of each run (Rule 9: this file
links to them and does not copy them).

| Run | Retro | Actions |
|---|---|---|
| Fan-out 1, 2026-09-30 | [RETRO-2026-09-30-fanout.md](../sprint-hup-s1-one-agent/RETRO-2026-09-30-fanout.md) | A1 to A11 |
| Fan-out 2, 2026-09-30 | [RETRO-2026-09-30-fanout-2.md](../sprint-hup-s1-one-agent/RETRO-2026-09-30-fanout-2.md) | A12 to A22 |
| Fan-out 3, 2026-10-01 overnight | [RETRO-2026-10-01-overnight.md](../sprint-hup-fanout-3/RETRO-2026-10-01-overnight.md) | A23 to A36 |
| Fan-out 4, 2026-10-01 day | [RETRO-2026-10-01-day.md](../sprint-hup-fanout-4/RETRO-2026-10-01-day.md) | A37 to A52 |
| Fan-out 5 | none of its own; reviewed and finished in fan-out 6 | |
| Fan-out 6, 2026-10-04 | `sprint-hup-fanout-6/RETRO-2026-10-04.md` (core PR #211) | A53 to A62 |
| Fan-out 7, 2026-10-04 | per-lane reports on the private sprint issues | at close |

## What the program set out to do

The planset's north star: a member types "help me make a mint project", answers a short interview,
watches a landing page with a working mint card appear in a pop-out browser, and Hermes refuses to
deploy until the contract passes a real audit gate, then deploys on 40204 through the signing
ceremony, verifies the source and pins the site. The core invariant: Hermes may propose anything,
every effect passes a Human In Control gate whose evidence is recorded, and nothing is reported as
done unless a verifier outside the model says so.

## Outcome so far (2026-10-04)

Gates, from [gates.yaml](../../planset/2026-09-30-hermes-upskill/gates.yaml):

| Gate | State on 2026-10-04 |
|---|---|
| gate0 Theory locked | met: decisions, red-team, Rule-3 ADR, and the eight TLA+ specs (g0-formal, evidence in [EVIDENCE-g0-formal-2026-10-04.md](EVIDENCE-g0-formal-2026-10-04.md)) |
| gate1 Stable + one agent | partial: downloads met; the loop, approval audit, no-block tripwire, render, sidebar, unlink and eval criteria have branch evidence or owner calls open |
| gate2 Controlled + knowledgeable | met on branch: HIC, knowledge (T1 77.3 % pass, 81.5 % citation hit against the owner's 75 % / 80 % target), personas, MCP |
| gate3 hello mint | partial: the deploy gate is met; the updater, licence, browser and the e2e on 40204 wait on owner sign-off, the component key ceremony, or a member at the packaged app |
| gate4 Chain-native, fleet, learning | pending: every item needs a redeploy, an activation after sign-off, or two machines |
| gate5 Everyday + proven | pending: size and OS runs need Linux and Windows hardware; the red-team pass runs on merged heads; docs are drafted on branches (this file, the essay, five Almanac pages) |

Test counts went up in every run. Stack-top numbers per run are in each run's SCOPE or retro. As one
arc: citrate-core cargo went from 745 at the fan-out 3 base to 2,142 at the fan-out 6 top, vitest from
796 to 2,186, and citrate-agent-runtime from 857 to 2,119. These are lane and stacking reports, not a
re-run by this lane.

Nothing was deployed, signed or broadcast on 40204 by any agent, and no version was bumped (D-41).

## What went well (program level)

- **Verifiers, not self-grading.** The rule that only a verifier says a step is done held for Hermes
  and for the team. Its best moment: the hello-mint gate, run with the supply cap removed, was NOT
  READY because of the template's own forge test, after Medusa, Slither and Aderyn had all passed the
  broken contract.
- **Builder plus adversarial reviewer, then a stacker.** Every lane in every fan-out was reviewed by
  a second agent that re-ran the gates and broke guards on purpose. The stackers were a third check
  and repeatedly found the joins between lanes that each lane alone could not see.
- **TLA+ before code on the risky seams.** Eight specs were written and checked at small bounds, with
  mutation checks. Writing `WebSigningBudget.tla` surfaced eight ambiguities in an accepted ADR
  before any budget code shipped (now drafted as an amendment, below).
- **Off by default.** Every new ability that could surprise a member ships off, with a "pending owner
  sign-off" note where the default is an owner call. The owner's 2026-10-01 and 2026-10-04 decisions
  moved several of these at once without code churn.

## What went wrong (program level)

- **Parallel lanes on one machine.** Shared cargo targets gave false compile errors, cleanups removed
  other lanes' worktrees, `git stash` mid-merge lost work, and load averages made live browser tests
  flaky. Every run's retro has a version of this. The brief rules (A40, A61) were not always followed,
  and the infrastructure fix (A55) did not land during the program.
- **Claims ran ahead of proof.** Implemented, wired, runtime-proven and ready were mixed up in every
  run and narrowed on review. A62 fixes the vocabulary; it has to be enforced by reviewers.
- **Open PR stacks grew deep.** By fan-out 6 the core stack was over twenty PRs deep before the first
  merge. Each run built on unmerged work, which made every later run depend on stack order.
- **External work queued up.** Linux and Windows runs, the component key ceremony, the registry
  redeploy, live FL rounds and the packaged-app QA by a person were known from the planset and are
  still the long pole.

## At close

To be written when v0.5.0 ships:

- the merged PR list per repo and the release tag;
- the final gate table, with every flip linked to its merged evidence;
- which owner decisions were taken and which defaults shipped as they are;
- the final test counts measured on the release commit;
- what moved to the next program.

## Program actions

Continuing the run actions (A1 to A62 stay where they are). These are program-level and carry past
v0.5.0.

| # | Action | Owner |
|---|---|---|
| P1 | Before the next program: one cargo target per lane or serialised final runs, sized to the disk (A55 made a precondition, not a wish) | orchestrator |
| P2 | Merge each run's stack before the next run builds on it, or cap stack depth, so a run never depends on more than one unmerged layer | owner + orchestrator |
| P3 | Keep the planset's external list as a tracked board from day one (hardware runs, ceremonies, redeploys), with a named person per row | owner |
| P4 | Accept or correct the Rule-3 amendment for A-1 to A-8 ([ADR-2026-10-04-rule3-amendment-1-budget-ambiguities.md](../../../docs/adr/ADR-2026-10-04-rule3-amendment-1-budget-ambiguities.md)) | owner + @rule8 |
| P5 | Keep "off by default, pending owner sign-off" as the release rule for any new agent ability | owner |

## Claim honesty

This draft states program-level patterns and points to per-run records for numbers. Nothing in the
"outcome so far" section is merged as of writing. The essay for this program is
[off-by-default-and-the-verifier-says-done.md](../../../docs/essays/off-by-default-and-the-verifier-says-done.md).
