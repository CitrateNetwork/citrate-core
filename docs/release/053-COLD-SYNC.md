---
created: 2026-11-08
branch: rel/053-network
author: Larry Klosowski (MAC lane, CORE-053; Claude Opus 5.5)
status: prepared (cannot run until the r1007 fleet exists)
gate: planset 05 G6 (CORE-N6), reusing RC52 G1 (#558), K1-L (#384) and G4 (cold-sync.yml)
---

# 0.5.3 clean-machine cold sync to the r1007 tip (CORE-N6)

**Goal.** A machine that has never run Citrate syncs r1007 from block 0 to the tip, as a member
would, on macOS arm64 and on Linux x86_64 and aarch64, and ends on the network's chain, not wedged,
running the node build the cut pinned.

**Why two layers.** The node-only harness (`scripts/e2e/cold-sync.py`, G4) proves the node and
its config. The packaged harnesses (K1-L on Linux, G1 on macOS) prove the installed app starts
that node the same way from a clean home. Both exist from RC52. This plan points them at r1007 and
adds the r1007 checks. Nothing here runs against production hosts: the harnesses only read the
public RPC and join the P2P network as a follower (no `--mine`).

## What changed in the harness for r1007 (in this PR, tested with the stand-in node)

| Rule | What it proves |
|---|---|
| config | a profile with `memberPruneRetain` gets the pruned chain store the app writes (`pruning = true`, `keep_blocks = 10000`); a retention under 10,000 or a retired profile is refused before anything starts |
| `fingerprint` | with `--expect-fingerprint <cut consensusFingerprint>`: the node reports exactly that fingerprint in `citrate_nodeIdentity` (the bytes are the cut's build, with the r1007 config); a node that reports none fails |
| `not_wedged` | the node's own `syncState` is not `wedged` at the end of the run; skipped (never a pass) for a node without `syncState` |

The existing rules stay: `endpoint_identity`, `local_genesis` (block 0 = the profile genesis),
`node_chain_id`, `reaches_target`, `agrees` (selected-parent ancestry agrees with the network RPC
down to block 0), `peers`, `at_rest`, `node_alive`, `cleanup`. Tests:
`scripts/e2e/cold-sync.test.mjs` "r1007 acceptance (CORE-N6)".

## Runs

Each run takes the cut values from `release/network-cut.json` after `apply` (the r1007 profile is
then the active one) and records its verdict as release evidence.

### 1. Linux x86_64, node only (GitHub-hosted runner)

`.github/workflows/cold-sync.yml`, manual dispatch on the release branch:

- `profile_name` empty (the active profile, r1007), `blocks` empty (to tip), `timeout` 6h,
  `negative` true (a wrong genesis must be refused);
- the workflow passes `--expect-fingerprint` itself when the tested profile is the cut's and the
  cut file has a `consensusFingerprint` (read from `release/network-cut.json`, never typed).

Pass: verdict PASS with `fingerprint ok`, `not_wedged ok`, `agrees ok`, `reaches_target ok`.

### 2. Linux, packaged, clean namespace (K1-L)

`scripts/ci/linux-native-acceptance.sh` in its sealed namespace (no host processes, files, display;
network allowed only for this run) with the 0.5.3 RC AppImage and `.deb`, x86_64 on
`citrate-core-linux-build` or the CI runner, aarch64 on an aarch64 builder (the DGX is gone;
owner to name one). **Not built yet:** a cold-sync phase for K1-L that starts the app headless, wait for the node's RPC,
then run `cold-sync.py`'s agreement and fingerprint reads against the app's own node RPC
(read-only) until it is within 10 blocks of the public head, with a 6 h limit.

Pass: the app's node reaches the tip; block 0 is the r1007 genesis; the ancestry agrees; the
fingerprint matches; `node.toml` has `pruning = true`, `keep_blocks = 10000`; the Node page word
is "Synced" (read from `node_status` through the harness, not a screenshot).

### 3. macOS arm64, packaged (GitHub-hosted runner, G1 harness)

`scripts/ci/macos-acceptance` (#558) on a `macos-14` (arm64) GitHub-hosted runner, which writes
its own test-host marker; never on the release Mac and never on a member's machine (the harness
refuses a machine with a Developer ID identity or an installed app). Mode `full` against the RC
DMG, plus the same cold-sync phase as run 2 (not built yet either). The runner's 6 h job limit bounds the run;
if r1007 is longer than the runner can sync in that time, run 1 (node only) carries the full
sync and run 3 is bounded at a height (`--blocks`), recorded as such.

### 4. Upgrade from 0.5.2 (G2 on r1007)

On the same clean machines: install 0.5.2 (its published artifacts), let it sync r1006 for a few
minutes (or stop at the cutover, when the public RPC serves r1007 and 0.5.2 reads "Update
required"), then update through the real feed ([053-UPDATER.md](053-UPDATER.md)). Pass:
`node/chain.r1006.bak/` holds the r1006 database, the wallet opens with the same address, and
the node then cold-syncs r1007 as in runs 2 and 3. The fixture version of this check already runs
in CI (`upgrade_r1007_tests`).

## What cannot be checked before r1007 exists

- every run above (no r1007 fleet, no genesis, no node bytes);
- the sync time on each platform (it decides whether run 3 is to tip or bounded);
- whether pruned members sync r1007 without the DAG-store prune (memory over a long sync is the
  open question from the r1005 history). The harness does not read the node's memory today; add a
  peak-RSS read to runs 2 and 3 when they are built.

## Still to build (before the cut, no r1007 needed)

- the K1-L and G1 cold-sync phase (runs 2 and 3), against a stand-in node first, as
  `cold-sync.test.mjs` does.

## Owner and lead decisions this needs

1. The aarch64 Linux machine for runs 2 and 4.
2. Whether run 3 must reach the tip, or a bounded height is acceptable on macOS.
3. Confirmation of the member retention default (N5, [053-NETWORK.md](053-NETWORK.md)).
