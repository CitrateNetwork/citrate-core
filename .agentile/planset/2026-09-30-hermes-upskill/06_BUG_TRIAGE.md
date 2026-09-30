---
created: 2026-09-30T00:00:00Z
branch: release/0.5.0-hermes-upskill
author: Larry Klosowski + Claude Opus 5.5
status: planset (Stage-2, red-teamed 2026-09-30)
planset: 2026-09-30-hermes-upskill
code: HUP
repo: citrate-core
companions: 05_SPRINTS_AND_WPS.md (HUP-S0)
---

# Bug Triage: owner-reported, 2026-09-29

Code-read triage against citrate-core `main` (read-only; nothing executed). Paths are
relative to citrate-core. The fixes are WPs in HUP-S0.

## B-1: Agent stalls with a macOS pinwheel

**Verdict:** a code defect, not machine load (load only makes it worse).

Ranked causes:
1. `ai_chat_local_tools` / `ai_chat_local` / `ai_chat_tools` are **sync commands**
   (`src-tauri/src/ai.rs:906–964`). On macOS they run on the main thread. Each does a
   blocking `ureq` POST with **no timeout** (`ai.rs:430–441`) and waits for the whole
   non-streamed generation. **No `max_tokens`** (`ai.rs:793–798`), so a reply can run to
   the 8192-token context. The loop repeats up to 6 turns (`src/agent/harness.ts:448`).
2. The 12 s frontend deadline (`src/bridge/tauri/invoke.ts:20,27–38`) rejects the UI
   promise while Rust still holds the main thread. A retry queues behind it.
3. `node_status` (`src-tauri/src/node.rs:765`) is sync and does local + remote RPC
   (6 s timeout) under a mutex, polled every 2 s (`src/shell/store.ts:467`).
4. Other sync I/O commands: `hermes_*` (no-timeout ureq, `hermes.rs:151–180`),
   `hermes_bridge_pending` (RPC), `model_catalog_search` (HF, with a comment that wrongly
   says it runs off the UI thread, `model_catalog.rs:344`), `model_catalog_select` /
   `model_serve_stop` (SIGTERM grace), `memory_*`, `storage_*`, `social_*`.
5. Render storm: whole-state subscription (`store.ts:3698`) + per-word `setState` +
   unmemoized markdown + a 600 ms global tick (`store.ts:448`).

The existing tripwire (`lib.rs:738–827`) scans 9 files and only direct callers, which is
why it missed these. **Fix:** HUP-S0.1, S0.2, S0.6.

## B-2: Hugging Face downloads don't finish / don't persist

1. `model_catalog_download` is not in `UNBOUNDED` (`invoke.ts:27–38`). The UI gives up
   at 12 s and clears `downloadingId`. Rust keeps downloading invisibly.
2. Retry appends a second writer to the same `.part` (append mode, `model.rs:554–558`).
   Interleaved bytes trip the overshoot guard, which deletes the `.part`
   (`model.rs:605–611`), or the file fails the SHA check (`:675–677`). There is no
   per-file lock.
3. No body/idle read timeout (`model.rs:376–380`). A dead connection mid-body blocks
   forever. No auto-retry.
4. Catalog downloads are not resumed after restart: the slice is in-memory
   (`src/shell/slices/models.ts:53`), and `read_local_models` skips `.part` files
   (`model_catalog.rs:305–313`).
5. Finished-but-unverified → re-download from byte 0 (`model.rs:490–502`,
   `store.ts:503`).
6. No 206 check on resume. There's also no HF token path for gated repos.

**Fix:** HUP-S0.2, S0.3.

## B-3: Agent text is bad

1. The italic regex `_([^_]+)_` (`src/components/Markdown.tsx:34`) mangles snake_case
   tool names and file names, which the system prompt itself lists
   (`src-tauri/src/ai.rs:102–116`).
2. Every streamed token is stripped of `**` (`store.ts:1901`). Bold never renders; `***`
   leaves stray `*`.
3. Plain-text-while-streaming then a markdown swap at the end (`AgentChat.tsx:130`)
   causes a reflow jump.
4. Streaming is simulated: `"stream": false` (`ai.rs:797`), then a word-by-word replay
   with sleeps (`harness.ts:497–505`).
5. Renderer gaps: no tables, no nested lists, `1. 1. 1.` numbering, fences with a
   language tag like `c++` fail, invalid block-in-span nesting (`AgentChat.tsx:127`).
6. Possible template/thought leakage: llama-server is spawned without explicit
   `--jinja` / `--reasoning-format` (`serve.rs:214–229`). Needs verification.
7. Failed turns vanish (`store.ts:1907`).

**Fix:** HUP-S0.4, S0.5, S0.7, and real streaming in S1.1.

## UX debt folded into S0.8

- Two download systems with different behavior.
- The skip toast points to Settings, but downloading lives only in onboarding
  (`store.ts:2517`).
- Model/provider choice in three places.
- Connections appears twice.
- The chat appears in three places.
- "Storage" is actually memory and "Files" is actually IPFS.
- Node "Pinning" overlaps Files.
- Social: People/Cluster duplicate Groups tabs; Community is unwired.
- The seven-column vitals strip truncates.
- Tutorials duplicate Commissary docs.
