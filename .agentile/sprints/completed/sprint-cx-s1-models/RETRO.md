---
created: 2026-08-27
branch: chore/close-cx-s1-s2
author: Claude Opus 4.8, directed by @SaulBuilds
status: archived
sprint: CX-S1 (Commons — model catalog & switcher + HF/GitHub OAuth), Lane A
closing_commit: 3b6889c (CX-S1.6 #147 merged)
purpose: Sprint retrospective for CX-S1 (agentile:retro) — the honest accounting.
---

# Retro — CX-S1 (Commons model catalog & switcher, Lane A)

## Outcome
| | |
|---|---|
| Goal achieved? | **YES.** Commons has a real model switcher: search Hugging Face + GitHub Releases, download with checksum-verified integrity, and hot-swap the active local model — keyless-signed OAuth, honest-when-empty. |
| WPs closed | S1.1 HF OAuth (`Service::HuggingFace`), S1.2 GitHub token-exchange `Accept` fix, S1.3 `ModelDescriptor` + per-model `ModelManager`, S1.4 HF Hub + GitHub Releases resolver, S1.5 runtime `llama-server -m` switch + download/select by id, S1.6 Models switcher UI. |
| Merged PRs | #142 (S1.1), #143 (S1.2), #144 (S1.3), #145 (S1.4), #146 (S1.5), #147 (S1.6). |

## Metrics delta (ratchet — no axis decreased)
| Axis | S1 start | S1 close |
|---|---|---|
| Rust lib tests | 285 | **293** |
| Frontend tests | 344 | **348** |
| New warnings | — | 0 |

## What worked (concrete + causal)
- **The parallel-safe scaffold held.** Every WP branched off `main`, touched only its s1-owned files, and passed `cx-ownership-check.sh s1` before merge — zero races, even merging six PRs back to back.
- **DTO-first reconciliation (S1.3→S1.4).** Reshaping the Rust `ModelDescriptor` to serialize 1:1 to the frozen TS DTO *before* wiring the resolver meant S1.6's bridge lined up with no rework; `download_url()` derived (never stored) so a descriptor can't drift from its own URL, and the HF form provably reproduces `DEFAULT_MODEL_URL`.
- **Verifiable-only catalog (Rule 1).** The resolver emits a descriptor only when the source yields a real sha256 — HF via the LFS oid, GitHub via a `<asset>.sha256` sidecar. A file with no trustworthy hash is skipped, never surfaced as an unverifiable download.
- **Fixture-injected transports.** The catalog resolver over an injected `HttpClient` and the model manager over a fixture `ModelTransport` meant every parser + the download/verify/quarantine path is CI-tested with no network and no 5 GB pull.

## What was corrected mid-sprint
- **S1.5 readiness gate.** `select_model` originally could stop the running model before checking the target was ready; corrected to gate on `is_file_ready` *before* stop/swap, so a not-ready target leaves the current model serving (fail-closed).
- **Test assumption fixes.** The S1.3 per-file test wrongly assumed no status side-file existed pre-verify; corrected to assert the real invariant (no Ready without verify).

## Lessons carried forward
- Reconcile the cross-boundary DTO at the earliest WP that owns the type, not at the UI WP.
- "Verifiable-only" is the honest default for any catalog of downloadable artifacts — skip, don't surface, what you can't check.

## Status
Lane A COMPLETE. All six WPs merged, green, reviewed. No open items.
