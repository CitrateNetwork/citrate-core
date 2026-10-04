---
created: 2026-10-01
branch: hup/n5-learn-rest
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# WP HUP-S3.4 (fan-out 5): the rest of verified self-learning

Story: US-3.4 "Learns only what's proven" (04_FEATURES_BDD). Closes the buildable items left
open by fan-out 4 (core PR #163, runtime PR #40, sprint issue #280).

| Not-done item from fan-out 4 | Now |
|---|---|
| No app screen launches a verified workflow | "Teach Hermes" card on Agents: task + checks, one-step `answer_contains` workflow in a fresh local-model session; only a fully passing run asks Hermes to propose (`learnLauncher.ts`, `TeachHermesCard.tsx`) |
| No UI to resolve a `both` contradiction | "Keep this one" + confirm on a contradicted memory; sidecar `POST /learn/memories/resolve` records HIC-1 first; ledger `false` / `true`, graph `supersedes` (confirmed); sync with the sidecar applies a lost answer |
| An accepted skill joins a running Hermes only after a restart | The sidecar reloads its skills library on accept; the next session is offered the skill (`skills_reloaded`) |
| No IPFS pin of a published skill bundle | Core pins `SKILL.md` to the local IPFS node, reads it back, checks the hash, and sends its CID as `manifestCID`; the payload must carry it (publish itself stays off) |

## Formal

citrate-agent-runtime `agent-learn/formal/ContradictionResolve.tla` (new): 6 invariants, TLC
clean at 12,900 and 22,296 distinct states; 11 guard mutants each caught. It found two restart
cases in the runtime (a resolution lost with the accept before it; a `persist_failed` memory not
counted as accepted) and the ordering rule core follows (only the kept memory is settled when one
resolution is applied). Each finding is a Rust test.

## Tests

| Suite | Before | After |
|---|---|---|
| runtime `citrate-agent-learn` | 60 | 67 |
| runtime `agent-sidecar` lib | 236 | 241 |
| core `cargo test --lib` | 1,284 passed, 10 ignored | 1,295 passed, 10 ignored |
| core vitest | 1,650 passed, 10 skipped | 1,670 passed, 10 skipped |

Gates: core `cargo fmt --all -- --check`, `tsc --noEmit`, clippy 1.98.1 `-D warnings` (core and
the two runtime crates) clean. Rust mutants: 8 runtime and 7 core guard mutants each fail a test;
the two runtime survivors are equivalent (documented in `agent-learn/formal/README.md`).

## Not done

- Publishing stays off pending owner sign-off (`SKILL_PUBLISH_ENABLED = false`).
- The teach card checks answers with `answer_contains` only; track workflows are not launched
  from it (S3.3 scope).
- Recall does not hide an unresolved memory.
- No packaged-app run yet.
