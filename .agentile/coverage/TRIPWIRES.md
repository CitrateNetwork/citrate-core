---
created: 2026-11-27
branch: feat/v60-lq-wp03-tripwire-inventory
author: Claude Opus 5.5 (LQ-WP03 worker) for Larry Klosowski
status: active
purpose: The honesty and tripwire registry rule (LQ-WP03, 0.6.0 non-regression guard). What is registered, how the check works, and the only way a registered test may go.
---

# Honesty and tripwire registry

Owner direction for 0.6.0: existing honesty and tripwire tests are kept, or replaced by stricter
ones. This file is the rule; [`tripwires.json`](tripwires.json) is the registry; the check is
[`scripts/ci/lq/tripwire-inventory.mjs`](../../scripts/ci/lq/tripwire-inventory.mjs), run by
[`.github/workflows/lq-tripwires.yml`](../../.github/workflows/lq-tripwires.yml) as
`lq / tripwires` on every PR, merge group and push to main.

## What is registered

Every test the runners list whose name matches the tripwire patterns:

| Runner | Listing (data source) | Pattern |
|---|---|---|
| cargo | `cargo test --workspace --locked --offline -- --list`, and the same with `--ignored`, on ubuntu-latest, toolchain as `ci.yml` | test fn matches `tripwire\|honest\|never_\|adv[0-9]\|dark_refuses\|payouts_dark`, or the module is `removed_commands_tests` / `invoke_secret_scan_tests` |
| vitest | `vitest list --json` (vitest leaves skipped, todo and only-filtered tests out of it) | file name matches `tripwire\|honesty`, or the full title matches `tripwire\|honest\|fabricat` |

Ids are the runner's own: `<test binary>::<module path>::<fn>` for Rust (the binary is the crate's
lib, `<name>@bin` for a bin target, or the integration test file), and
`vitest:<file>::<suite > title>` for vitest. Doc-tests are not registered.

Only tests that run count. A Rust test marked `#[ignore]` stays in `--list` but `ci.yml` does not
run it, so the `--list --ignored` listing is subtracted; a vitest `.skip`, `.todo` or a sibling of
`.only` is already absent from `vitest list`. (A test that skips itself at run time is outside
what a listing can see; reviewers watch for it.)

Each entry: `{ id, class, since, replaced_by }`, plus `replacement_evidence` once replaced.
`class` (custody, ceremony, money, honesty, sidecar, updater, network, ipc, privacy, ui-truth)
is an advisory tag from the id; the review owner may retag it.

## What the check refuses

1. A registered test that is not in the lists (deleted, renamed, or moved behind a cfg), or that
   is listed but marked `#[ignore]`.
2. An entry removed from the registry. Entries are never removed, even when their test is replaced.
3. A replacement (`replaced_by` set) unless all of these hold:
   - the replacement has its own registry entry, and the chain of replacements (A -> B -> C) ends
     at a registered test that is listed and runs; a middle link need not exist any more;
   - `replacement_evidence` links the run or commit (`github.com/CitrateNetwork/...`) where the
     replacement failed against a deliberately weakened copy of the guarded code;
   - the PR has the `tripwire-replace` label, matched exactly from the JSON label list GitHub
     gives (checked on the PR; the merge group re-checks everything else).
4. Re-pointing an earlier replacement, a replacement cycle, a duplicate id, or a malformed entry.

A tripwire-named test that is not registered is a warning today, and becomes a failure
(`--strict-unregistered`) at LQ-WP03 close. Register it in the same PR that adds it.

## Replacing a tripwire (the only way one goes)

1. Write the stricter test first. Weaken the guarded code on a throwaway branch (the mutant the old
   test killed) and record the run where the new test fails. This is the mutation-testing step.
2. In the PR: keep the old entry, set `replaced_by` to the new id and `replacement_evidence` to
   that run or commit link; add the new test's own entry; add the `tripwire-replace` label. To
   retire a replacement later, set `replaced_by` on the replacement's own entry (B -> C); the
   earlier edge (A -> B) is never changed.
3. The owner reviews: `tripwires.json` is code-owned by @SaulBuilds (CODEOWNERS), so no weakening
   merges without the owner.

## Refreshing the registry

Every run uploads `lq-tripwires-candidate` (existing entries untouched, new matches added with
`since` = the head commit). The LQ lead reviews it into `tripwires.json`. CI never commits it.

```sh
node scripts/ci/lq/tripwire-inventory.mjs generate --registry .agentile/coverage/tripwires.json \
  --cargo-list cargo-list.txt --cargo-ignored cargo-list-ignored.txt \
  --vitest-list vitest-list.json --root "$PWD" --since <rev>
node scripts/ci/lq/tripwire-inventory.mjs count --registry .agentile/coverage/tripwires.json
```

`count` prints `{registered, active, replaced, by_class}` (plus `present`, the active entries
that run, when given the three listings); it is
the tripwire axis command in [`BASELINE.md`](BASELINE.md).
