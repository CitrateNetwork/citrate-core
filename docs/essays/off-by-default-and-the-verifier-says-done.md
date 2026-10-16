---
created: 2026-10-04T00:00:00Z
branch: hup/n7-docs-almanac-retro
author: Larry Klosowski + Claude Opus 5.5
status: essay
program: HUP (Hermes upskill, planset 2026-09-30-hermes-upskill), release v0.5.0
---

# Off by default, and the verifier says done

The Hermes upskill set out to make an agent a person would choose over a frontier agent they could
open in a browser tab. In five days it added folders, a shell, a browser, web search, a toolchain,
a deploy gate, skills that learn, personas, a node MCP server and a fleet wizard. Written down like
that, it sounds like the riskiest release Citrate Core has shipped. It should be one of the least
surprising for a member who updates and never opens Settings. Two rules carry most of that weight,
and both are cheap enough to copy.

## Rule one: the model never says done

The open-source agent this work started from has a well-documented habit. It grades its own runs,
nearly always as a success, and then saves the run as a skill. A wrong answer becomes a method, and
the method is loaded the next time. Most of the complaints collected in the planset's external
research trace back to that loop.

The fix is not a better prompt. It is a change of who is allowed to say "done". In the Hermes
workflows, a step is finished when its verifiers pass: a forge test run, a Slither report below a
severity, a Medusa campaign with no failures, a required phrase in an answer, a JSON field with an
expected value. The model can propose that a step is done; it cannot make it so. Learning is gated
on the same signal: a skill or memory can be proposed only from a run whose verifiers all passed,
and only a person can accept it.

This rule found real problems during the program. The hello-mint gate was run with the template's
supply cap removed. A full Medusa campaign, Slither and Aderyn all passed the broken contract. The
template's own forge unit test was the only check that caught it, and the gate said NOT READY
because that test failed. If the model had been the judge, it would have read three clean tool
reports and called the contract ready. The lesson is narrower than "tools are good": the verifier
that matters is often the dullest one, the test someone wrote for the specific property.

The same rule applied to the people and agents building Hermes. Reviewers broke guards on purpose
in every lane, and at least eight guards survived the builders' own tests in one run. Each now has
a test that fails when the guard is removed. A claim of "closed" was accepted only when the proof
was tests or a real run, and several were narrowed back to "implemented" or "wired" when the proof
was not there. The agent and the team were held to the same standard, which is the only way the
standard stays honest.

## Rule two: off by default is a promise, not a setting

The second rule is about what a member gets without asking. Web search, page reading, the managed
browser, the shell, the toolchain, the node MCP server, Hermes's own access to it, the faucet,
sign-in budgets and skill publishing all start off. Folder grants start with no folder. A persona
starts as Hermes's own voice, which changes nothing.

It would be easy to treat this as a list of toggles. It is more useful to treat it as a promise
with a test behind it: with every new switch at its default, the app a member gets is the app they
had, plus a better chat. That framing changes how work is reviewed. A new environment variable for
the sidecar has to be on an explicit list, so a setting cannot quietly override the control bind or
the capsule folder. A setting whose default would change a member's app is an owner decision, and
the code ships the conservative value with a "pending owner sign-off" note until the owner rules.
Dozens of those notes exist across the release. They are not loose ends; they are where the
promise is written down.

Off by default also keeps the hard proofs small. The signing ceremony is unchanged for every
signature that is not on the closed list of budgetable ones, and that list is empty for a member
who never grants a budget. The TLA+ models for the agent loop, folder grants, sign-in budgets, the
deploy gate, the spend budget, skill persistence, anchor batches and device links check designs that
a member reaches only by turning something on and approving it.

## What the two rules share

Both rules move a decision away from the component that is most likely to be wrong about it. The
model is the worst judge of whether its own run worked, so a verifier judges. A default is the worst
place to make a risk decision on a member's behalf, so the default takes no risk and the member, or
the owner for everyone, makes the call.

Neither rule makes Hermes clever. They make it trustworthy enough that cleverness can be added a
switch at a time. That is the trade the program chose, and it is the one worth defending when the
next release is tempted to turn something on for everyone.

## Further reading

- The planset's core invariant: [00_OVERVIEW.md](../../.agentile/planset/2026-09-30-hermes-upskill/00_OVERVIEW.md)
- The verifier-gated learning flow: [HERMES_LEARNING.md](../HERMES_LEARNING.md)
- The closed list of budgetable signatures: [ADR-2026-09-30-rule3-budgetable-signatures.md](../adr/ADR-2026-09-30-rule3-budgetable-signatures.md)
- The program retro: [RETRO.md](../../.agentile/sprints/active/sprint-hup-s11-prove-it/RETRO.md)
