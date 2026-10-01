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

Written and checked against the runtime loader. **Not bundled yet:** the app does not set
`CITRATE_HERMES_SKILLS` for the sidecar, and these files are not in the Tauri resources. Wiring
them in is part of the bundled-knowledge work (HUP-S3.1/S3.2), not this pack.
