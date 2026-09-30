---
created: 2026-09-30T00:00:00Z
branch: hup/s0-stabilize
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S0
---

# HUP-S0 Evidence

Every gate criterion flips to `met` only with an entry here: command, result, commit.

| Criterion | Evidence | Commit | Date |
|---|---|---|---|
| g1-approval-audit | Already met on main (PBA-L7b-002, #102): `src/shell/agentToolGates.test.ts` class tripwire — every non-read chat tool stops at `requestSig`/wallet review; 21/21 pass (`npx vitest run src/shell/agentToolGates.test.ts`). Planset baseline claim of an approval gap was from stale code. | 63d138c (origin) | 2026-09-30 |
| g1-no-block | **S0.1** `main_thread_tripwire.rs`: scans every `src/*.rs` at test time, transitive (fixpoint) call-graph over blocking markers; RED listed 58 sync commands; GREEN after converting all 58 to async wrappers over unchanged `*_sync` bodies via `blocking::off_main`. Plus `max_tokens` (2048) + connect 10 s / overall 300 s on every AI call; Hermes control calls bounded at 30 s. **S0.2** `invoke.test.ts`: chat 330 s, Hermes 45 s, downloads/large files unbounded, default 12 s. (Frame-timing QA on hardware still pending.) | this branch | 2026-09-30 |
| g1-downloads | **S0.3** `model_tests.rs` (+6): segmented ranged reads (64 MiB, 180 s each) auto-resume from the exact byte after a failed segment or a mid-body drop; retries bounded (8, backoff) with the good partial kept; single-flight `DownloadGuard` refuses a second writer; finished-but-unverified file is verified, never re-downloaded; `check_range_status` requires 206 on resume. `model_catalog_tests.rs` (+2): `.download.json` markers → `model_catalog_partials` lists interrupted downloads after restart. `models.test.ts` (+3): Resume rows. **Open: S0.3b HF token for gated repos** (needs a custody token reader, T1 review). | this branch | 2026-09-30 |
| g1-render | **S0.4** `Markdown.test.tsx` (12): snake_case/file names never italicized, bold + bold-italic, arithmetic `*` literal, GFM tables, nested lists, one ordered list across blanks + `start`, fences with any info string/tildes/indent, unclosed fence as code, strikethrough, no raw HTML, **termination fuzz (400 random inputs < 200 ms each)** — the old renderer looped forever (OOM) on ```c++. `chatFailure.test.ts`: `**` preserved in streamed tokens. **S0.5** llama-server `--jinja --reasoning-format deepseek` + `strip_template_tokens` on every parsed reply (3 tests). **S0.7** failed turns inline with Retry, never sent back as context (3 tests). Real SSE streaming lands with S1.1. **S0.6** `renderStorm.test.ts` (4): empty `setState` never notifies; the desktop `tickPatch` is `{}` while idle (cosmetic `pollIn`/pin countdowns now sim-only — they re-rendered the whole app ~1.7×/s forever); a 200-token reply renders < 15 times (tokens coalesced ~32 ms). Per-component selector subscriptions deferred to S1.1 (webview becomes a view over the sidecar event stream). | this branch | 2026-09-30 |
| g1-sidebar | — | — | — |

## Test counts

| When | cargo test --workspace --locked | vitest | Commit |
|---|---|---|---|
| Kickoff (static count) | 675 `#[test]` | 523 cases | 28a74ba |
| Kickoff (run) | 669 passed, 6 ignored | 536 passed | 28a74ba |
| After S0.1 + S0.2 | 676 passed, 6 ignored | 540 passed | ef5fd75 / 591c2de |
| After S0.3 | 684 passed, 6 ignored | 543 passed | 20f6b68 |
| After S0.4 + S0.5 + S0.7 | 687 passed, 6 ignored | 559 passed | 90481e1 |
| After S0.6 | 687 passed, 6 ignored | 563 passed | this branch |
