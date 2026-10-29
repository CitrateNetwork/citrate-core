---
created: 2026-10-28T00:00:00Z
branch: spike/252-windows-job-mechanism
author: Sisyphus, directed by Kurt
status: active
sprint: SCL-S10.0
planset: 2026-10-05-sidecar-lifecycle
code: SCL
repo: citrate-core + citrate-agent-runtime
issue: CitrateNetwork/citrate-core#252
---

# SCL-S10.0 evidence

## Pinned state

- Date: 2026-10-28
- Host: Windows, `x86_64-pc-windows-msvc`
- Runtime base SHA: `05243c72bd521a55bc38333eaf6ae6efacee3816`
- Runtime branch: `spike/252-s10-windows-job-proof`
- Core base SHA: `2c812854eade1a8e4b95687d88de477fc7190e96`
- Core branch: `spike/252-windows-job-mechanism`
- Issue: `CitrateNetwork/citrate-core#252`

## Acceptance proof

The proof uses real Win32 processes and Jobs, not mocks. Every acceptance test has bounded waits;
helper cleanup is guarded. The tests are in
`citrate-agent-runtime/agent-shell/src/windows_job_acceptance_tests.rs`.

| Test | Exact S10.0 criterion | Local result |
|---|---|---|
| `windows_s10_acceptance_suspended_helper_runs_only_after_adoption` | Suspended child is assigned to the Job before it runs | pass |
| `windows_s10_acceptance_assignment_failure_reaps_suspended_helper` | Assignment failure terminates the suspended child and never resumes it uncontained | pass |
| `windows_s10_acceptance_job_handle_is_not_inherited_or_usable` | Job handle is non-inheritable and no child has a usable duplicate | pass |
| `windows_s10_acceptance_owner_death_closes_job_and_terminates_tree` | Kill-on-close fires when owner death closes the final Job handle | pass |

The selected mechanism is stable `std::process::Command` plus `CREATE_SUSPENDED`, ToolHelp thread
enumeration (`CreateToolhelp32Snapshot`, `Thread32First`, `Thread32Next`), and `ResumeThread` only
after Job assignment. The owner-held Job is non-inheritable, configured with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and uses no breakaway flags. Raw `CreateProcessW` is
unnecessary; undocumented `NtResumeProcess` is rejected.

## Red-phase status

No failing-test-first artifact exists for this proof: the production stable-Rust Job mechanism and
fail-closed cleanup were already present at runtime base
`05243c72bd521a55bc38333eaf6ae6efacee3816` from prior runtime PRs #72 and #73 before these
acceptance tests were added. This is not an exception: issue #252's red-green requirement remains
unmet unless the authorized owner accepts this mechanism-characterization case or requests a
legitimate alternative artifact.

## Local verification

```text
cargo fmt -p citrate-agent-shell -- --check
PASS

cargo test -p citrate-agent-shell --locked windows_s10_acceptance -- --nocapture --test-threads=1
4 passed; 0 failed

cargo test -p citrate-agent-shell --locked
unit: 30 passed; 0 failed; 5 ignored
Windows integration: 12 passed; 0 failed
total: 42 passed; 0 failed; 5 ignored

cargo clippy -p citrate-agent-shell --all-targets --locked -- -D warnings
PASS
```

`cargo fmt --all -- --check` encounters unrelated pre-existing formatting drift outside
`citrate-agent-shell`, including `agent-legacy/src/adapters/memory.rs`. The package-scoped format
check above is the relevant S10.0 check; this record does not claim the workspace-wide check passed.

## Hosted verification

[Runtime PR #104](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/104) at
`8a2296ab2cd5359dad6831fbabb728c34925449f` passed its full check set, including aggregate `ci`.
The hosted [Windows (agent-shell tests + agent-sidecar release build)](https://github.com/CitrateNetwork/citrate-agent-runtime/actions/runs/40827023392/job/123292293808)
job result: `PASS` (17m15s). It runs `cargo test -p citrate-agent-shell --locked`, which includes the four
non-ignored acceptance tests. No separate focused hosted command or hosted test count is recorded.

## Rule-2 record

The runtime base has 38 passing package tests, 26 unit and 12 Windows integration. The proof tree
has 42 passing tests, an increase of four, plus five ignored helper entry points. Those five
ignored tests are process helper entry points, not disabled acceptance tests.

## Limits and pending evidence

Production behavior is unchanged by this proof except for a minimal `#[cfg(test)]` Job-handle
inspection seam. Hosted Windows CI evidence is recorded above; the local evidence remains local
only. Core documentation is published in [PR #595](https://github.com/CitrateNetwork/citrate-core/pull/595);
review and merge remain pending. This does not claim release,
packaged-app validation, complete Windows containment, core adoption, issue closure, S10.1 through
S10.4 completion, or real managed-browser and `npx` fixture coverage.
