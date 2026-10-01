---
created: 2026-10-01
branch: hup/n4-process-split
author: Larry Klosowski + Claude Opus 5.5
status: review
---

# HUP-S1.9 (rest): the process split

The parity suite landed in #124. This closes the other half of the row: the agent's tools that
run other programs no longer share a process with the agent loop.

## Process layout

```text
citrate-core ── supervises (kit/src/supervisor.rs) ──> sidecar = the agent loop
                                                         ├── toolchain worker (child process)
                                                         └── browser worker   (reserved, HUP-S5.1)
```

Per the hermes-loop-in-sidecar ADR the loop is the sidecar process, which core already supervises
with backoff, `/health` and SIGTERM then SIGKILL. Splitting the loop out of the sidecar again would
add a hop with no new isolation boundary, so the split is loop versus tool workers.

## What changed

- **citrate-agent-runtime** (`hup/n4-process-split`):
  - New crate `agent-workers`: supervises one child per worker over a line-delimited JSON stdio
    wire (`ping`, `call`, `shutdown`). Startup ping, a health ping every 5 s (2 s timeout, killed
    after 2 misses), restart with backoff (250 ms doubling to 10 s), give-up after 5 restarts in
    60 s (`failed`), clean shutdown (request, close stdin, 3 s grace, kill). A worker exits when
    its stdin closes, so it never outlives its sidecar. Calls in flight when a child ends fail as
    `Crashed("<how it ended>")`.
  - `agent-sidecar`: the toolchain tools run in `citrate-agent-sidecar --worker toolchain`,
    reached through `RemoteToolHost`. A crash returns a tool error that names the signal, says
    the worker is being restarted, and says the run was not retried; the session finishes its
    turn. `GET /workers` (bearer) reports both kinds; the browser entry is `not_built`. SIGTERM
    stops the workers first, then drains. The worker does not inherit the control-plane bearer
    file path.
- **citrate-core** (`hup/n4-process-split`):
  - `hermes_workers` (async, off the main thread, in the main-window ACL; pop-out capabilities
    unchanged) reads `GET /workers`. A sidecar that is not running reports no workers.
  - The Activity monitor shows a "Worker processes" section: each worker's state, restarts and
    last exit ("running, restarted 1 time (last exit: killed by signal 9)"). The main-window host
    polls every 5 s while the monitor is open and republishes on change. An unread report says
    "could not be read", never "no workers".

## Proof

- runtime: `agent-workers` 4 unit + 14 integration (the test binary re-executed as the child);
  `agent-sidecar` +5 unit, +11 integration in `tests/process_split_tests.rs`, including the real
  binary end to end: kill -9 the worker, `/workers` shows it running again with `restarts: 1` and
  `last_exit: killed by signal 9`, the control plane never stopped, SIGTERM takes the worker down.
  Five supervisor mutants killed (restart bound, crash delivery, health kill, restart count, env
  scrub). Workspace 1248 passed, 0 failed (baseline 1214).
- core: cargo +4 (`hermes::workers`, one mutant killed), tripwire and ACL tests green, workspace
  913 passed, 0 failed (baseline 909); vitest 1011 passed (baseline 1003), tsc clean.
- Parity suite: unchanged and passing on both sides (`parity_wire_tests`, `harness.test.ts`).

## Not done / honest scope

- Process isolation only, no OS sandbox (separate work item, US-2.2 AC1).
- A program a killed worker had started (a forge run) is in its own process group and can
  outlive the worker until its own wall-clock timeout.
- The browser worker slot is reserved; the browser tools are HUP-S5.1.
- `harness.ts` is **kept**. Retiring it waits on the owner's turn-cap decision (6 in `harness.ts`
  vs 8 as the sidecar session default), pending owner sign-off.
