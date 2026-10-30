---
created: 2026-10-29T23:04:16Z
branch: docs/288-windows-appcontainer-plan
author: Sisyphus, directed by Kurt McKelvey
status: proposed
issue: CitrateNetwork/citrate-core#288
base_branch: release/0.5.1
base_sha: f177382d221f4755fd3d75984c78cb1e6f0b8de4
runtime_sha: 75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63
tier: T1
security_review: pending
---

# ADR: Windows Hermes LPAC write boundary

## Status

**Proposed, design only.** This ADR does not authorize implementation, executable tests, CI,
acceptance, enforcement, release readiness, or S10.1 to S10.4 claims. Procedure, current state,
validation, dependencies, and stop conditions belong to the
[sprint scope](../../.agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md).

## Authority and pinned baseline

This is the design record for [CitrateNetwork/citrate-core#288](https://github.com/CitrateNetwork/citrate-core/issues/288), against
`release/0.5.1@f177382d221f4755fd3d75984c78cb1e6f0b8de4`. The runtime reference is
`CitrateNetwork/citrate-agent-runtime@75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63`, pinned by
[`eval/sidecar-runtime.rev`](../../eval/sidecar-runtime.rev). #250, #252, core PR #595, and
runtime PR #104 are unmerged and provisional; this ADR infers no private blocker disposition.
Any cross-repo implementation needs federation sprint and ownership routing before runtime changes;
no federation artifact is needed for this core-only design slice.

## Context

At the pinned runtime, `agent-shell/src/sandbox.rs:24-26` and `agent-shell/src/lib.rs:70-71`
describe no Windows OS sandbox backend and Required-mode refusal. The real-process pattern is
`agent-shell/tests/sandbox.rs:656-754`. These are source facts, not runtime FAIL evidence.
`SandboxUnavailable` is not the final passing oracle; a compilation failure is a portability
blocker only. The pinned/released 0.5.1 behavior remains Required-mode refusal. Linked
[#266](https://github.com/CitrateNetwork/citrate-core/issues/266),
[#287](https://github.com/CitrateNetwork/citrate-core/issues/287), and
[#302](https://github.com/CitrateNetwork/citrate-core/issues/302) place later Windows sandbox
capability work on the 0.5.2 lane. Although #288 originally targeted 0.5.1, this proposal neither
rewrites release history nor claims a new approved schedule. The current state is
**PLANNED-RED / NOT EXECUTED**.

## Decision

WIN-FS-1 is a future filesystem write-confinement design for the production Required-mode
ShellRunner path on supported local NTFS only. Every run uses a newly created LPAC profile and a
unique per-run package SID, never a regular or stable AppContainer SID. The child starts suspended
with LPAC security capabilities and `PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY` opting
out of ambient `ALL APPLICATION PACKAGES` authority.

| Mechanism | WIN-FS-1 assessment |
| --- | --- |
| Regular AppContainer | Rejected: a profile could be unique per run, but it retains broader ambient package authority and lacks the LPAC restriction required by this proposal. |
| Restricted/low-integrity token | Rejected: restriction and integrity do not define a filesystem path allowlist. |
| LPAC | Selected principal: unique per-run package SID enables token and ACL attribution. |
| LPAC + Job | Selected composition: LPAC/ACL provide the filesystem boundary; Job is orthogonal lifecycle containment. |

Per-run LPAC plus explicit ACL and effective-access verification is selected because adding one
candidate ACE alone cannot establish a boundary: Windows permissions are additive.

## WIN-FS-1

The intended future guarantee is: the production Required-mode ShellRunner launches an externally
verified per-run LPAC; the unique run SID may write the one canonical grant root and the disposable
profile scratch; and a parent-writable sibling control root denies a write to its pre-existing,
closed file with exactly `ERROR_ACCESS_DENIED (5)`.

Non-guarantees: this design does not claim read confinement, network denial, Job proof,
all-path coverage, all-filesystem coverage, protected-file parity, implementation, enforcement, or
passing evidence. Recursive grants do not reproduce Unix `.git/hooks`, `.git/config`, or `.env*`
parity. Required mode remains unavailable until that parity is decided and tested.

## Boundary ownership and launch transaction

AppContainer/ACL authority and Job ownership remain orthogonal, composed in one transaction:

1. Persist a durable run ledger before any ACL mutation.
2. Create no profile or other persistent security state before that ledger exists.
3. Preconfigure an independent kill-on-close, no-breakaway Job.
4. Create the suspended LPAC child, preferably with a `JOB_LIST` attribute.
5. Externally verify its token and Job membership before resume.
6. Resume only if every check succeeds; otherwise terminate and reap the suspended child before
   rollback.

## Principal, ACL, path and environment requirements

The entire disposable AppContainer profile and virtualized storage are scratch; this ADR does not
limit scratch to one directory. Outside scratch, grant the unique run SID only on exactly one
canonical member-granted root. The future candidate ACE mask is
`FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD | SYNCHRONIZE`,
with OI/CI inheritance and DACL-protection behavior selected only after T1 review; this draft does
not prove the correct settings. It excludes `WRITE_DAC`, `WRITE_OWNER`,
`ACCESS_SYSTEM_SECURITY`, and Full Control. Final rights remain subject to native testing and T1
review; any uncertainty fails closed rather than broadening the mask.

Adding this ACE is insufficient because Windows permissions are additive. Through the retained
no-follow, identity-checked grant-root handle, implementation must post-verify the effective
descriptor and effective access for the suspended child. The check accounts for `ALL APPLICATION
PACKAGES`, `ALL RESTRICTED APPLICATION PACKAGES`, owner/`OWNER RIGHTS`, inherited ACEs, capability
SIDs, existing package-specific grants, and mandatory-integrity policy. It must prove the child
cannot obtain `WRITE_DAC`, `WRITE_OWNER`, or `ACCESS_SYSTEM_SECURITY`, regardless of which ACE
would grant them. Record and verify DACLs plus mandatory-integrity labels and policy for both grant
and control; Low-IL token verification alone is insufficient. Run B's distinct LPAC token must be
denied run A's grant root and profile, and vice versa.

Version one rejects UNC, nonpersistent-ACL volumes, and reparse points in the grant root, cwd,
and executable chain. It accepts supported local NTFS with persistent ACLs only, opens through
no-follow handles, and binds authorized objects to volume serial and file ID.

Use a Unicode-scrubbed, explicit environment with `HOME`, `TEMP`, and `TMP` redirected to profile
scratch. `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` contains only dedicated standard-I/O handles; it
contains no directory, token, registry, socket, Job, profile, unrelated pipe, or other inherited
handle. While suspended, verify `TokenIsAppContainer=1`, `TokenIsLessPrivilegedAppContainer=1`,
the ledger's exact package SID, Low IL, expected user SID, no elevation, no UIAccess, and the exact
approved no-network capability allowlist. These are future checks, not present network evidence.

## Teardown and recovery

First observe the authoritative Job-empty state, then reconcile pipes and handles. Descriptor
mutations and exact run-SID ACE removal use the retained identity-checked handle, are serialized by
the trusted launcher, and are post-verified. They must not intentionally overwrite unrelated
entries, but this ADR claims no unsupported atomicity or concurrent-change guarantee; descriptor
drift causes quarantine. Never restore a whole security descriptor. Delete scratch and profile
afterward; quarantine any cleanup failure and never reuse that SID.

## Verification contract

The planned native Windows test and exact command are specified by the sprint scope. It must use
the production Required-mode path and a real native helper, establish equivalent parent-user DACLs
and mandatory labels for sibling `grant` and `control` roots, and prove the narrow outcomes stated
in WIN-FS-1. It must also cover a junction from `grant` to `control`, absent unrelated inheritable
handle and environment canary, a distinct second-run SID, and no run-SID ACE/profile/scratch after
teardown. This is a future contract, not an executed result.

## Future gates

WIN-READ-1, WIN-NET-1, and WIN-JOB-1 are unproven, release-blocking future gates for restricted
read, network, and Job evidence respectively; no full matrix is designed here. Future closure also
requires native Windows 10 and Windows 11 evidence, protected-file parity, unchanged core
folder-grant and HIC approval semantics integration-tested, T1 review, and CI. Windows Required
mode stays unavailable until those gates pass.

## Consequences

- **Positive:** a unique SID makes ledger, ACL, token, and cleanup checks attributable to one run.
- **Negative:** supported scope is local NTFS only; setup, verification, and failure cleanup add
  launch complexity and may fail closed.
- **Neutral:** Hermes capability ownership remains as defined by the Hermes-loop ADR; this design
  complements rather than replaces owned-sidecar lifecycle ownership.

## Alternatives considered

- **Regular or stable AppContainer:** rejected because authority and cleanup can cross runs.
- **One writable scratch directory:** rejected because profile and virtualized storage are scratch
  as a whole.
- **Path checks, inherited handles, or post-resume inspection:** rejected due to substitution or
  authority windows.
- **Whole security-descriptor restore:** rejected because it can erase concurrent ACL changes.

## Affected components

Future work would affect the runtime `agent-shell` Required-mode ShellRunner path, Windows launch
and token/ACL helpers, the managed AppContainer profile lifecycle, and native Windows test support.
Existing core folder-grant and HIC approval semantics remain unchanged and require integration
testing before closure. It changes neither current runtime behavior nor core custody boundaries.

## Claims and non-claims

This ADR claims only a proposed architecture and verification contract. It makes no claim of
implementation, tests, CI, acceptance, enforcement, release readiness, network denial, read
confinement, Job proof, or all-path/all-filesystem coverage. It does not disclose private review
details.

## Compliance check

The design preserves Rule 3 custody boundaries and Rule-9 source ownership by linking to the
Hermes-loop and owned-sidecar lifecycle ADRs instead of copying their contracts. Rule 4 status
authority is the active [SCOPE](../../.agentile/sprints/active/sprint-288-windows-hermes-appcontainer/SCOPE.md);
this ADR owns architecture only. It is T1 and remains proposed pending the gates and review below.

## T1 security review/sign-off

| Review role | Required disposition | Current disposition |
| --- | --- | --- |
| T1 security reviewer | SID/ACE lifetime, recovery, and claim review | Pending |
| Windows security reviewer | token, LPAC, path, handle, and Job composition review | Pending |
| Owning maintainers | architecture and native-evidence review | Pending |

No row is approved or accepted.

## References

- [Issue #288](https://github.com/CitrateNetwork/citrate-core/issues/288),
  [#250](https://github.com/CitrateNetwork/citrate-core/issues/250),
  [#252](https://github.com/CitrateNetwork/citrate-core/issues/252),
  [#266](https://github.com/CitrateNetwork/citrate-core/issues/266),
  [#287](https://github.com/CitrateNetwork/citrate-core/issues/287),
  [#302](https://github.com/CitrateNetwork/citrate-core/issues/302),
  [core PR #595](https://github.com/CitrateNetwork/citrate-core/pull/595), and
  [runtime PR #104](https://github.com/CitrateNetwork/citrate-agent-runtime/pull/104).
- [`eval/sidecar-runtime.rev`](../../eval/sidecar-runtime.rev) and runtime source at
  `75e21cd9c33b4feb6b3c75ababfb755ffe0b8f63`, including `agent-shell/src/sandbox.rs`,
  `agent-shell/src/lib.rs`, and `agent-shell/tests/sandbox.rs`.
- [Hermes-loop ADR](ADR-2026-09-30-hermes-loop-in-sidecar.md) and
  [owned-sidecar lifecycle ADR](ADR-2026-10-05-owned-sidecar-lifecycle.md).
- Microsoft: [AppContainer isolation](https://learn.microsoft.com/windows/win32/secauthz/appcontainer-isolation),
  [LPAC](https://learn.microsoft.com/windows/win32/secauthz/low-privilege-app-containers),
  [process-thread attributes](https://learn.microsoft.com/windows/win32/procthread/proc-thread-attribute-list),
  and [Job Objects](https://learn.microsoft.com/windows/win32/procthread/job-objects).

## Revision history

| UTC | Change |
| --- | --- |
| 2026-10-29T23:04:16Z | Initial proposed design record; no runtime execution claimed. |
