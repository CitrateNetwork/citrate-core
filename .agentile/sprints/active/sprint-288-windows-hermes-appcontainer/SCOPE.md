---
created: 2026-10-29T23:04:16Z
branch: docs/288-windows-appcontainer-plan
author: Sisyphus, directed by Kurt McKelvey
status: active
phase: design-only
issue: CitrateNetwork/citrate-core#288
base_branch: release/0.5.1
base_sha: f177382d221f4755fd3d75984c78cb1e6f0b8de4
runtime_sha: 75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63
tier: T1
security_review: pending
---

# Sprint 288: Windows Hermes AppContainer write boundary

## Authority and tracking

This active, design-only sprint tracks [CitrateNetwork/citrate-core#288](https://github.com/CitrateNetwork/citrate-core/issues/288).
Under Rule 4, this SCOPE is the authoritative status record for this design slice. Architecture,
decision, rationale, alternatives, compliance, claims, security review, and references are owned
by the [LPAC write-boundary ADR](../../../../docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md).

## Pinned workspace/source map

| Item | Pin or exact anchor |
| --- | --- |
| Core workspace | `release/0.5.1@f177382d221f4755fd3d75984c78cb1e6f0b8de4` |
| Runtime pin | `CitrateNetwork/citrate-agent-runtime@75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63` |
| Pin source | `eval/sidecar-runtime.rev:1-9` |
| Runtime availability | `agent-shell/src/sandbox.rs:24-26`; `agent-shell/src/lib.rs:70-71` |
| Real-process test pattern | `agent-shell/tests/sandbox.rs:656-754` |

## Scope

Plan WIN-FS-1 only: filesystem write confinement on supported local NTFS for the production
Required-mode ShellRunner path, using the ADR's externally verified unique per-run LPAC, grant
root, and disposable profile scratch design.

## Explicit exclusions

No implementation, executable test, CI, acceptance, enforcement, release-readiness, S10.1 to
S10.4, network-denial, read-confinement, Job-proof, all-path, or all-filesystem claim is in scope.
No runtime or CI change, stable SID, full security-descriptor restore, private blocker detail, or
broad network/Job matrix is permitted.

## Deliverables

The deliverable is this scope plus the linked ADR: a reviewable future launch/teardown contract,
red-first native-test procedure, gate dependencies, and pending T1 review criteria. It is not an
implementation or an evidence record.

## Red-first procedure

The planned future test is
`windows::writes_are_limited_to_grant_and_profile_scratch`, through the production ShellRunner
Required-mode path and a real native helper, never a mock or alternate launcher:

```text
cargo test -p citrate-agent-shell --test sandbox windows::writes_are_limited_to_grant_and_profile_scratch -- --exact --nocapture
```

On local NTFS, create sibling `grant` and `control` with equivalent parent-user DACLs and mandatory
labels. The parent writes before and after. Add the unique run-SID ACE only to `grant`; writes to
`grant` and profile scratch must succeed. Opening the pre-existing closed `control\denied.txt` as
`CreateFileW(GENERIC_WRITE, OPEN_EXISTING, ...)` must return exactly `ERROR_ACCESS_DENIED (5)` and
leave it unchanged. A junction in `grant` to `control` must also return 5. Confirm an unrelated
inheritable handle and environment canary are absent, the second run receives a different SID, and
teardown leaves no run-SID ACE, profile, or scratch. The future contract also requires retained
handle effective-access verification, mandatory-integrity label/policy checks, denial of protected
rights regardless of additive ACE source, and reciprocal cross-run denial between runs A and B.
This is planned future validation only.

## Document validation commands

Run these commands after drafting; they are instructions, not evidence that any command has run:

```text
python -c "from pathlib import Path; ps=[Path('docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md'),Path('.agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md')]; required=['created: 2026-10-29T23:04:16Z','branch: docs/288-windows-appcontainer-plan','issue: CitrateNetwork/citrate-core#288','base_sha: f177382d221f4755fd3d75984c78cb1e6f0b8de4','runtime_sha: 75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63','tier: T1','security_review: pending']; assert all(all(x in p.read_text(encoding='utf-8') for x in required) for p in ps)"
python -c "from pathlib import Path; a=Path('docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md'); s=Path('.agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md'); assert (a.parent / '../../.agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md').resolve() == s.resolve(); assert (s.parent / '../../../../docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md').resolve() == a.resolve()"
git diff --check -- docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md .agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md
git diff --name-only
git diff -- docs/adr/ADR-2026-10-29-windows-hermes-appcontainer-write-boundary.md .agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md
```

Confirm the frontmatter command returns every required pin for both files, the link-resolution
command exits successfully, and `git diff --name-only` lists exactly the two approved paths above.

## Future-only gates

| Gate | Purpose and minimal closure | Disposition |
| --- | --- | --- |
| WIN-FS-1 | Narrow write-boundary contract, including effective-access checks | Planned, release-blocking |
| WIN-READ-1 | Restricted-read closure; no detailed matrix here | Unproven, release-blocking |
| WIN-NET-1 | Network closure; no detailed matrix here | Unproven, release-blocking |
| WIN-JOB-1 | Job closure; no detailed matrix here | Unproven, release-blocking |
| Protected-file parity | Decide and test `.git/hooks`, `.git/config`, and `.env*` treatment | Required before closure |
| HIC/folder integration | Prove unchanged core folder-grant and HIC approval semantics integrate | Required before closure |
| Windows 10/11 native evidence | Native evidence on both Windows 10 and Windows 11 | Required before closure |
| T1 review | Pending SID/ACL, launch, and claim review | Required before closure |
| CI | Reviewed CI evidence | Required before closure |

## Dependencies

Future work depends on supported local NTFS with persistent ACLs, a native Windows runner, the
pinned core/runtime revisions, every ADR launch and teardown condition, protected-file parity,
unchanged folder-grant/HIC integration, Windows 10 and 11 native evidence, T1 review, and CI.
Cross-repo implementation requires federation sprint and ownership routing before runtime changes.
#250, #252, core PR #595, and runtime PR #104 remain unmerged and provisional; no private blocker
disposition is inferred.

## T1 review gate

All checklist items are **Pending**: SID/ACE lifetime; ACL partial failure and recovery; reparse
and path substitution; inherited handles and environment; token verification; LPAC/Job composition;
effective descriptor/access and mandatory-integrity policy; cross-run isolation; and claim honesty.
No item is approved. Required mode remains unavailable until every future-only gate passes.

## Stop conditions

Stop before resume on any planned identity, ACL, path, handle, environment, capability, or Job
check mismatch; terminate and reap the suspended child before rollback. Stop Required-mode
availability for unsupported volume, UNC path, nonpersistent-ACL volume, or reparse point in the
grant root, cwd, or executable chain. Fail closed rather than broaden candidate ACE rights.

## Current status

**PLANNED-RED / NOT EXECUTED.** At the pinned runtime, no Windows OS sandbox backend exists and
Required mode refuses; no executable WIN-FS-1 test exists. This is not runtime FAIL evidence.
`SandboxUnavailable` is not the final passing oracle, and a compilation failure is only a
portability blocker. No implementation, tests, CI, acceptance, enforcement, release readiness, or
S10.1 to S10.4 claim is made.
