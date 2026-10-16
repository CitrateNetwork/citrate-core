---
created: 2026-10-01T00:00:00Z
branch: hup/n3-templates
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: HUP-S6.2, HUP-S6.9
---

# HUP-S6.2 + S6.9: contract and dApp templates, Medusa budgets

Planset `2026-09-30-hermes-upskill`: 05_SPRINTS_AND_WPS (S6.2, S6.9),
02_ARCHITECTURE section 7, 00_OVERVIEW red-team correction #10. Sprint issue:
CitrateNetwork/citrate-federation#283. Canonical description of the tree:
[`templates/README.md`](../../../../templates/README.md).

## What landed

| Item | Where | State |
|---|---|---|
| ERC-20 (permit, fixed supply), ERC-721 paid mint, ERC-1155 paid mint, Governor + votes token, on OpenZeppelin v5.7.0 | `templates/{erc20,erc721,erc1155,governor}/` | implemented, forge-tested |
| Solady ERC-721 variant (same interface as `erc721`) | `templates/erc721-solady/` | implemented, forge-tested |
| hello-mint: `erc721` under `contracts/` + vite/React/wagmi/viem mint page under `app/`; fork vs 40204 by `VITE_TARGET`, address from `VITE_CONTRACT_ADDRESS` | `templates/hello-mint/` | implemented, type-checked, `vite build` passes; not run in a browser |
| Medusa harness per contract template (`test/Properties.sol`, `property_*`) + the same harness as Foundry invariants (`test/Invariants.t.sol`) + `medusa.json` | each template, ERC-721 harness shared via `_common/erc721-properties` | invariants run under forge; **Medusa itself not run (not installed here)** |
| Renderer `citrate-templates` (Rust lib + CLI): strict parameter validation, `{{ct:key}}` substitution, all-or-nothing write into an empty dir, `citrate-template.lock.json` provenance | `templates/renderer/` | implemented; **not wired into the app or the sidecar** (S6.3/S6.4) |
| S6.9 per-tier Medusa budgets as data | `templates/medusa-budgets.json`, `renderer/src/budget.rs` | implemented; call budget flows into `medusa.json`; coverage plateau / minimum coverage recorded for S6.3/S6.4, nobody enforces them yet |
| Dependency pins (OZ v5.7.0, Solady v0.1.26, forge-std v1.17.0, full commits) | `templates/deps.lock.json` | pinned; the gate fetches and checks commits; nothing vendored |
| Template gate script | `templates/scripts/verify-templates.sh` | run for real (below) |

### Why the renderer is Rust

Its callers are the Tauri backend and the toolchain sidecar (both Rust); it
writes files on the user's disk, so it sits where the no-`unwrap` rule and typed
errors apply; and it needs no Node runtime. It is a standalone workspace crate
with no Tauri dependency so the sidecar can take it later.

### Injection safety

Each parameter has a validator with an alphabet that cannot close or escape a
string in Solidity, TypeScript, JSON or HTML (`name`: letters, digits, single
space or hyphen; `symbol`: A-Z0-9; numbers: canonical decimals; `owner`:
EIP-55). A name that would make the contract identifier shadow an imported or
declared symbol is refused, and two tripwire tests scan every template for
`import {..}` symbols and fixed `contract`/`interface` declarations and require
each to be on the reserved list. Substitution re-checks every value as
defense in depth. Unknown keys, unknown or unterminated placeholders, symlinks
in a template and a non-empty output directory are all errors, and a failed
render writes nothing.

## Evidence

Red first: the three Rust test files were written against an empty crate and
failed to compile (`unresolved import citrate_templates::budget`, `::params`,
`RenderError`, `TemplateSet`), then went green.

| Check | Command | Result |
|---|---|---|
| Renderer tests | `cargo test -p citrate-templates` | 32 passed (params 9, budgets 5, render 18) |
| Clippy | `rustup run 1.98.1 cargo clippy --no-deps -p citrate-templates --all-targets -- -D warnings` | clean |
| Template gate | `templates/scripts/verify-templates.sh --work <dir> --deps-cache <dir> --node-modules citrate-core/node_modules` (T1) | all 6 templates: forge build + forge test green; hello-mint app `tsc --noEmit` + `vite build` green |
| forge tests per template | (inside the gate) | erc20 5 + 3 invariants; erc721 9 + 7; erc721-solady 9 + 7; erc1155 7 + 4; governor 4 + 4 |
| `forge lint` on every rendered project | `forge lint` | 0 findings |
| Slither 0.11.6 on rendered `src/` (T1 sample params) | `slither src --compile-force-framework foundry --foundry-compile-all --exclude-dependencies` | 0 High, 0 Medium; Informational only (low-level call in withdraw, long price literal, EIP-6372 `CLOCK_MODE` name). One Low (constructor parameter shadowing in the Governor) fixed |
| Mutation check, rendered erc721 | 5 mutants: drop `nonReentrant` on mint, accept overpayment, drop the cap check, drop `onlyOwner` on withdraw, miscount supply | all 5 killed |
| Mutation check, app config | loopback check removed; `citrate` target ignored | killed (1 and 2 failing tests) |
| hello-mint config (vitest) | `npx vitest run src/templates` | 5 passed |
| tsc (core) | `npx tsc --noEmit` | clean |
| vitest (core) | `npx vitest run` | 801 passed, 3 skipped (89 files) |
| `cargo test --workspace` (core) | `cargo test --workspace` | 777 passed, 0 failed, 6 ignored |

Two test bugs found on the way, both the same Foundry footgun: an external call
inside the argument list of the call under `vm.expectRevert` (`token.PRICE()`,
`token.MAX_SUPPLY()`) is the call the cheatcode checks. Values are now read
into locals first, with a comment.

## Not done (honest)

- **Medusa was not run.** It is not installed on this machine (by instruction).
  The harnesses compile and run as Foundry invariants; `medusa.json` is checked
  as JSON with the tier budget. First real `medusa fuzz` belongs to S6.1/S6.3.
- **Coverage budgets are recorded, not enforced.** `coverage_plateau_calls` and
  `min_coverage_pct` have no consumer until the S6.3 runner and the S6.4 gate.
- **Budget numbers are starting values.** T1 = 50k calls follows the planset;
  T0 10k and T2 200k, workers and coverage bars need measuring on real hardware.
- **No dependency vendoring.** The S6.1 bundle must ship the pinned commits; the
  renderer only records them.
- **The renderer is not called from the app or the sidecar**, and the templates
  are not yet listed as Tauri bundle resources.
- **The hello-mint page was not run against an anvil fork in a browser.** It
  type-checks and builds; the end-to-end run is S11.1.
- No deploy logic (S6.4/S6.6), by design.

## Owner decisions taken conservatively

1. Governor template has no timelock and uses a timestamp clock (block-time
   independent on a BlockDAG). Adding `GovernorTimelockControl` is a template
   variant away.
2. ERC-20 template has no owner/admin and no mint function: the `owner`
   parameter is the initial holder only.
3. Medusa budgets T0/T1/T2 = 10k/50k/200k calls; minimum coverage 60/75/80%.
4. forge-std pinned to v1.17.0 (v1.9.7 produces solc 0.8.36 deprecation noise).

## Test counts

| When | cargo test --workspace | vitest | Commit |
|---|---|---|---|
| Base (`origin/release/0.5.0-hermes-upskill`) | 745 passed, 6 ignored (777 minus the 32 new) | 796 passed, 3 skipped (801 minus the 5 new) | 525b9ca |
| This branch | 777 passed, 0 failed, 6 ignored | 801 passed, 3 skipped | this branch |

## Journal

The interesting part of this WP was not the contracts, which are standard, but
the seam between a model-chosen string and a compiler. The tempting design is
escaping per output language. The design here is narrower and easier to trust:
accept only an alphabet that is inert everywhere, then make the tests prove the
templates never import or declare a name a user could collide with. The other
lesson repeated the fan-out retro: a cap is only tested if the fuzzer can reach
one past it, so each harness has a "past the cap" handler, not just a clamped
mint. And the same Foundry footgun bit twice in one hour; the fix is a habit
(read values before `expectRevert`), and it is now a comment in the template the
agent will copy from.

## Adversarial review (2026-10-01)

Reran on this branch: citrate-templates 33 tests (32 plus 1 new), clippy 1.98.1
clean, `verify-templates.sh` over all 6 templates green (forge build and test, and
the hello-mint app `tsc --noEmit` plus `vite build`), core `tsc --noEmit` clean,
vitest 801 passed / 3 skipped. Ten renderer guard mutations were each caught except
the symlink check, which is backed by the "not a regular file" refusal (equivalent
mutant). Two contract mutations on a rendered erc1155 (no per-id cap, withdraw not
owner-only) were each caught by forge tests.

Fixes made in review:

- The renderer claimed a failed render writes nothing, but an I/O failure partway
  through the write phase left a partial tree. `render` now records every file and
  directory it creates (including missing parents of the output directory) and
  removes them on failure. Test: `a_write_failure_partway_leaves_nothing_behind`
  (seen failing first; both "no undo" and "no directory undo" mutants caught).
- hello-mint fork mode: the fork keeps chain id 40204, so the wallet sends the mint
  to whatever its own 40204 network points at. The page now says so in fork mode,
  and the project README explains it. A real guard (for example a distinct fork
  chain id, or checking the wallet's RPC against the fork) is left to S6.4/S6.6.

Not run in review: core `cargo test --workspace` (free disk was 7 GB, near the
6 GB stop line; the change touches only the standalone citrate-templates crate).
