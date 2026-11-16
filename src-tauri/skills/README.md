---
created: 2026-10-01
branch: hup/n3-faucet-adr-literacy
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S7.7
---

# First-party Hermes skills

Citrate's own `SKILL.md` skills for Hermes, in the agentskills.io format that the runtime loader
accepts (`citrate-agent-runtime` `agent-loop/src/skills.rs`, HUP-S3.2). Third-party skills are
reviewed separately and pinned in `skills.lock` (HUP-S3.6); nothing here is third-party.

| Skill | What it teaches |
|---|---|
| `citrate-paraconsensus` | Belnap four-valued logic: the values, the knowledge and truth orders, off-chain classification and reduction, `learning_root` independence, the memory CRDT, the ContradictionLedger |
| `citrate-belnap-aggregate` | the `0x0110` precompile byte by byte, with a worked example checked on chain 40204 |
| `citrate-precompiles` | the Citrate precompile map: addresses, gas, shapes, and which ones contract code can reach |
| `citrate-sidecar-consensus` | agreement beside the chain: the learning daemon, and how the keyless Hermes sidecar decides while core signs |
| `citrate-fl-member` | taking part in rounds: what is live on this network, consent per round, the data rule, HIC-1 and HIC-2 |
| `citrate-fl-organizer` | groups, invites and rosters, and what a group round needs (WorkMarket, not deployed yet) |
| `citrate-fl-datasets` | the local dataset store: create, sample, clean, versions, the publish gates, irreversibility |
| `citrate-fl-collect` | the compliant collection policy: sources, robots.txt and terms, never-collect, limits, provenance |
| `citrate-fl-adapters` | the LoRA eval gate, its checks, load and unload |
| `citrate-fl-payouts` | dark payouts and the SALT-only copy rules |
| `citrate-fl-diagnose` | rejected rounds (FRL fault codes), coordinators, reverted and unknown-outcome transactions |

0.5.4 (Hermes x FL WP-H11, the same rule as #716): every name that reads as `citrate-…`, lookalike
letters included, is reserved.
`skill_write` and learned skills cannot take one (`skills_local::reserved_skill_name`), because saved
and learned skills load before these and would shadow them. `src/agent/skills/literacySkills.test.ts`
checks that every tool an FL skill names exists in this build.

## Rules for these files

- The frontmatter uses only keys the loader accepts. Agentile Rule-5 fields (`created`, `branch`,
  `author`, `status`) live under `metadata`, because the loader refuses unknown top-level keys.
- Every source claim cites `repo:path` or `repo:path#symbol` in backticks, and the repo is pinned
  in `metadata` by full commit. `src/agent/skills/literacySkills.test.ts` checks the format and,
  when the source repos are checked out (`QA_SOURCES_ROOT`), that every cited file exists at its
  pin and every `#symbol` occurs in it.
- The QA questions for this material are `src/agent/eval/qa-literacy-v1.json`
  (`eval/QA-literacy-v1.md`).

## Status

Written and checked against the runtime loader. HUP-S3.2 (2026-10-01, branch
`hup/m2-knowledge`): bundled. Every bundle config ships `skills/**/*` as a resource, and
`src-tauri/src/hermes.rs` lists this folder in `CITRATE_HERMES_SKILLS` after the member's learned
and saved skills (and before the reviewed third-party skills in `skills-bundle/`), so the
sidecar's one loader offers them and ranks them per turn with the rest.
