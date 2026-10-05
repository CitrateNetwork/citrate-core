---
created: 2026-10-04
branch: hup/n7-sizeup-library
author: Larry Klosowski + Claude Opus 5.5
status: evidence for review; core switch staged as a patch; the sizeup repo now exists, the library branch is not pushed yet
sprint: HUP-S8 Fleet (fan-out 7, lane L07)
wps: S8.2 (sizeup probe source), S1.6 follow-up; federation item F-10
repos: citrate-sizeup (local branch hup/n7-sizeup-library, not pushed), citrate-core hup/n7-sizeup-library
---

# F-10: core's tier probe on the citrate-sizeup library

## Where this stands

**Update after review (2026-10-04):** the private `CitrateNetwork/citrate-sizeup` repository now
exists (created 2026-10-04, Pacific time) and holds `main` at `09b3e11`. The
library branch `hup/n7-sizeup-library` is still local only: `1658eba`, `d593b8b`, and the review
commit `f29c3e0`, which pins that the library probe never asks the collector for the hostname or
a site key and that a zero RAM reading stays unknown (sizeup tests 296 to 298). The text below was
written before the repo existed; step 1 of the finish list is now only "push the branch and merge
it".

The work is done and proven on this machine. The core side is **not** on this branch as code
yet, because the private `CitrateNetwork/citrate-sizeup` repository does not exist. Creating it
is the owner's call, and a Cargo dependency on a remote that does not exist would break every
core build. A sibling `path` dependency would also break CI, which checks out core alone (Cargo
resolves path dependencies even when they are optional, so a feature flag does not help).

So this branch carries:

1. **`n7-sizeup-library.patch`** (next to this file): the complete core switch, tested here
   against a sibling checkout of sizeup. Apply it once the repo exists (steps below).
2. **Five vitest render cases** in `src/fleet/fleetWizard.render.test.tsx`: the fleet wizard's
   machine step renders every outcome the sizeup rule returns (T0 light by RAM, T1 worker, T2
   heavy by the dedicated-GPU lift, T0 for unknown memory) and the no-tier case as unknown. These
   hold today and after the patch, because the `TierReport` shape does not change.

## What the sizeup side gained (local commits, branch `hup/n7-sizeup-library`)

In `/Users/learnlikelarry/Projects/citrate-labs/citrate-sizeup`, on top of `09b3e11`:

- `1658eba` **feat(library)**: `sizeup_core::library`, the stable embedding API
  (`citrate.sizeup.library/1`). `probe(&ProbeOptions) -> Result<Receipt, ProbeError>` uses the
  same collector as the `sizeup-probe` binary, never collects the hostname, applies no site key,
  writes nothing and sends nothing. `Receipt::from_bytes` validates untrusted bytes (size,
  schema, sections, digest) and reads a `HostSummary`. `role_recommend(&Receipt)` and
  `recommend(&HostSummary)` return the tier, the role, the reserve, usable RAM and which rule
  decided (`Basis`). `NODE_RESERVE_BYTES` is pinned by test to `daemon_set_cost(DAEMON_SET).ram`.
  No new dependency (dep-budget holds), no new receipt field.
- `d593b8b` **fix(hw/macos)**: Apple Silicon is detected through Rosetta (`hw.optional.arm64`),
  the rule core's old reader used, so an x86_64 app build on an M-series Mac still gets the
  unified-memory flag.

Proof: `cargo test --workspace --locked` 276 to 296, all passing; clippy 1.98.1
`--workspace --all-targets -D warnings` clean on macOS, plus `-p sizeup-core` for
`x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`; `cargo fmt --check` clean;
`scripts/dep-budget.sh` passes.

## What the patch changes in core

- `src-tauri/Cargo.toml`: adds `sizeup-core` (as a sibling `path` in the patch; swap to the git
  pin below).
- `src-tauri/src/tier.rs`: `probe()` takes a sizeup receipt and maps its host summary onto
  `HardwareFacts`; `recommend_tier()` takes the tier from `library::recommend` and keeps only the
  profile table, the rationale text and the override. Deleted: the sysctl, `/proc/meminfo`,
  `nvidia-smi` and PowerShell readers and parsers, the process runner and the threshold
  constants. `NODE_RESERVE_BYTES` now is `sizeup_core::library::NODE_RESERVE_BYTES`, and its
  7 GiB pin test is kept. Free disk for the app data folder is still read in core, because sizeup
  reads the primary volume and models land in the data folder.
- `src-tauri/src/fleet.rs`: `role_for` maps through `library::Tier::parse` and
  `Role::for_tier`. `fleet_probe` reaches sizeup through `tier_recommend_sync`.
- Tests: the six parser tests went with the parsers. Eight replace them: seven in
  `tier_tests.rs` (summary mapping for Apple Silicon including Rosetta, CUDA, unknown; the
  reserve constant; a RAM by VRAM grid where core's tier equals sizeup's; tier id agreement) and
  one in `fleet_tests.rs` (roles are sizeup's and parse strictly). Net +2.

Proof with the patch applied (sizeup as a sibling checkout, 2026-10-04, macOS arm64):
`cargo test --lib` in `src-tauri` 1718 passed, 0 failed, 15 ignored; clippy 1.98.1
`--all-targets -D warnings` clean; `cargo fmt --all -- --check` clean. Front end on this branch:
`tsc --noEmit` clean; vitest 2190 passed, with three `scripts/*.test.mjs` CLI cases timing out
under parallel load and passing when rerun alone (46 of 46), unrelated to this change; release
pin tripwire OK.

## To finish after the repo exists (owner first, then one short lane)

1. **Owner**: the private `CitrateNetwork/citrate-sizeup` exists. Push branch
   `hup/n7-sizeup-library` from the local checkout (open it as a PR; merge it so the pin is on
   `main`).
2. **Owner**: extend the `CITRATE_CHAIN_READ_TOKEN` secret (core CI and release workflows) to
   Contents:read on `citrate-sizeup`, or core CI cannot fetch it.
3. **citrate-federation** `manifest.toml` (Rule 12, before the Cargo change): add
   `[repos.citrate-sizeup]` (tier 1, private, `rev` = the merged sha, `consumed_by =
   ["citrate-core"]`), add `"citrate-sizeup"` to core's `consumes_repos`, and add:

   ```toml
   # HUP F-10: citrate-core reads hardware and takes the tier rule from sizeup's stable
   # library API (sizeup_core::library, citrate.sizeup.library/1). Zero third-party crates.
   [[drift]]
   consumer = "citrate-core"
   dep_repo = "citrate-sizeup"
   file = "src-tauri/Cargo.toml"
   pin_field = "sizeup-core"
   pin = "<merged sizeup sha>"
   ```

   Not added on this pass: drift-check fails a row whose consumer file has no `rev` pin, and a
   `[repos]` entry for a repo that does not exist breaks `bootstrap.sh`.
4. **citrate-core**: `git apply n7-sizeup-library.patch`, then replace the `path` line with
   `sizeup-core = { git = "https://github.com/CitrateNetwork/citrate-sizeup", rev = "<merged
   sizeup sha>" }`, run `cargo update -p sizeup-core`, and run the core gates (fmt, clippy 1.98.1,
   `cargo test --lib`, tsc, vitest, tripwire, `cargo audit`). Delete the patch file in the same
   commit.
5. **📡 DGX team**: run `cargo test -p sizeup-core --test library_api` in citrate-sizeup on a
   Linux box with an NVIDIA GPU and on Windows 11, and the core tier step on both, and attach
   the outputs to the S8 sprint issue (planset proof: three reference machines).

## Owner decisions (defaults built, pending sign-off)

- Role names and mapping (T0 light, T1 worker, T2 heavy) now live in sizeup's `library::Role`;
  still a default pending owner sign-off, as the wizard says.
- F-10 recommended default: create the private repo and push (not vendoring). Nothing in this
  lane creates the repo.
