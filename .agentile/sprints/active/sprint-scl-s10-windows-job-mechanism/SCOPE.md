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
companions: ../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md, EVIDENCE.md
---

# Sprint SCL-S10.0: Windows Job mechanism proof

This bounded sprint records the stable-Rust Windows spawn mechanism required by S10.0 and its
local proof. The authoritative acceptance row is
[S10.0](../../../planset/2026-10-05-sidecar-lifecycle/05_SPRINTS_AND_WPS.md#red-team-additions-2026-10-05),
added for red-team finding RT-05.

## Selected mechanism

Use `std::process::Command` with `CREATE_SUSPENDED`; create an owner-held non-inheritable Job
with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; assign the suspended child; enumerate its threads with
ToolHelp (`CreateToolhelp32Snapshot`, `Thread32First`, `Thread32Next`); then `ResumeThread` every
matching thread only after assignment. No breakaway flags are enabled. The owner keeps the final
Job handle, so owner death closes it and kills the captured tree.

`NtResumeProcess` is undocumented and rejected. A raw `CreateProcessW` backend is unnecessary for
this implementation. Assignment failure fails closed: terminate the Job and child while suspended,
reap the leader, and never resume an uncontained child.

## Bounded scope

- Record the ADR addendum and local acceptance evidence from runtime branch
  `spike/252-s10-windows-job-proof`, base
  `05243c72bd521a55bc38333eaf6ae6efacee3816`.
- Keep this core documentation branch at base
  `2c812854eade1a8e4b95687d88de477fc7190e96` on
  `spike/252-windows-job-mechanism`.
- Prove the mechanism with real Win32 processes and Jobs, bounded waits, and guarded helper
  cleanup. Production behavior is unchanged except for a minimal `#[cfg(test)]` Job-handle
  inspection seam.

## Out of scope

S10.1 through S10.4, real managed-browser and `npx` fixtures, complete process accounting, core
adoption, hosted CI, review, merge, release, packaged-app validation, and issue closure are out of
scope. Hosted Windows CI evidence remains pending until a branch or PR runs. No commit or push
exists yet.

## Exit record

Local results and the Rule-2 test-count record are in [EVIDENCE.md](EVIDENCE.md). They are proof
of this mechanism only, not evidence of hosted CI or broader Windows containment.
