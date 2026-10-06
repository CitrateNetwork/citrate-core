---
created: 2026-10-06T00:00:00Z
branch: release/v0.5.0-prep
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: SCL-S0
release: v0.5.0
companions: ../../../planset/2026-09-30-hermes-upskill/10_RELEASE_PLAN.md, ../../../planset/2026-09-30-hermes-upskill/gates.yaml, ../../../planset/2026-10-05-sidecar-lifecycle/gates.yaml, ../../../../docs/RELEASE.md, ../../../../docs/releases/v0.5.0.md
---

# v0.5.0 release checklist

Every step from the prep PR to a published v0.5.0, with owner and status. This file is the
tracking surface for the cut (Rule 4); the release plan
([10_RELEASE_PLAN.md](../../../planset/2026-09-30-hermes-upskill/10_RELEASE_PLAN.md)) and the
ceremony ([docs/RELEASE.md](../../../../docs/RELEASE.md), [RELEASE_LINUX.md](../../../../docs/RELEASE_LINUX.md),
[RELEASE_WINDOWS.md](../../../../docs/RELEASE_WINDOWS.md)) stay the source for how each step is
done; this file links to them and does not repeat them.

Scope (owner decisions 2026-10-05, third set, recorded on #254): v0.5.0 is the minimum viable
release, the HUP work already merged on `release/0.5.0-hermes-upskill` plus SCL-S0 (S0.1, S0.3 to
S0.7, S1.6a, S8.5a) plus the 40204 reroll pin. S7.5a and the supervisor contract ship in v0.5.1;
the rest of SCL in v0.5.2. *Owner decision (2026-10-06): S7.5a is back in v0.5.0 (closes the open
High RT-12 in the 0.5.0 red-team slice); `g3-provider-routing-v050` is release v0.5.0.* Private items are named here only generically (S0.1, S0.3, S0.4: process
cleanup safety improvements; specifics on federation #298).

Owners: **Owner** = Larry Klosowski (@SaulBuilds). **Mac lane** = the owner's Mac with agents.
**DGX** = the DGX team (chain operator, Linux builds, mirror). **Windows team** = @kurtatwork and
@RDCTart69. **Reviewers** = @BerryManifold, @Matr0xshka (code owners on `main`).

Status words: **done**, **in PR** (open, named), **blocked on X**, **pending**.

## A. Before G (now)

| # | Step | Owner | Status |
|---|---|---|---|
| A1 | Version 0.5.0 in `package.json`, `package-lock.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `Cargo.lock` (`citrate-core` entry); release notes, this checklist and the 0.4.x upgrade notice | Mac lane | in PR (this PR, `release/v0.5.0-prep`) |
| A2 | Record the third-set owner decisions (S7.5a to v0.5.1, v0.5.x renamed v0.5.2) in the plansets | Owner merges | in PR #254 |
| A3 | SCL-S0.5, S0.7, S1.6a (shutdown coverage, Windows installer stops own sidecars, Windows CI job) | Mac lane | done (#255 merged, 835bef6) |
| A4 | SCL-S0.6 + S8.5a (genesis-change startup barrier and chain database lock check) | Mac lane; Reviewers | in PR #256 |
| A5 | Process cleanup safety improvements in core | Mac lane; Owner merges | in PR #257, #258 |
| A6 | Process cleanup safety improvements in the Hermes runtime, through the private remediation route (#298) | Mac lane; Owner merges | pending |
| A7 | Rebuild the Hermes sidecar (`scripts/build-hermes.sh`) from citrate-agent-runtime `main` after A6 lands; record the runtime commit in [EVIDENCE.md](EVIDENCE.md) and here: runtime commit `c2f394c973a617c8144fd0bca7821c13e882d365` (includes runtime #71 and #70) | Mac lane | done (2026-10-06, v0.5.0-rc.1; EVIDENCE.md "v0.5.0-rc.1") |
| A8 | Rule-2 record on the release head after A3 to A6: `cargo test --workspace --locked` and vitest counts in [EVIDENCE.md](EVIDENCE.md) | Mac lane | done (2026-10-06, v0.5.0-rc.1; EVIDENCE.md) |
| A9 | Upload the knowledge corpus tarball and the import-corpus `mem-mcp` to `runtime-deps`, pinned in `src-tauri/runtime-deps.sha256` (the release notes claim the bundled corpus) | DGX | pending |
| A10 | Book the Windows team's S0.7 native run (#249): needs a 0.5.0 Windows installer and a machine running 0.4.2 | Owner, Windows team | pending |
| A11 | Licence check: `node scripts/licence-inventory.mjs --corpus src-tauri/knowledge-corpus --require-sign-off` and the third-party notices steps (docs/RELEASE.md steps 6 and 7); see gate g3-licence below | Owner (sign-off), Mac lane | pending owner decision |

## B. When G and the new book are posted (federation #289)

Never hand-edit an address or a pin; every value below comes from the posted book and the chain
commit.

| # | Step | Owner | Status |
|---|---|---|---|
| B1 | DGX posts the 40204 genesis commit G, the genesis hash and the new `contracts/addresses/40204.json` on federation #289 | DGX | pending (expected 2026-10-06) |
| B2 | Pin G: the `citrate-chain` `rev` in `src-tauri/Cargo.toml` (`citrate-wallet-core`, `citrate-commd`) and `kit/Cargo.toml` (`citrate-wallet-core`), `Cargo.lock`, and `MIN_CHAIN_REV` in `scripts/build-sidecar.sh` (today `80c1781c`); manifest pin through `citrate-federation/manifest.toml` + `pin-bump.sh` (Rule 11) | Mac lane | blocked on B1 |
| B3 | Book sync with the generator only: `python3 scripts/sync-addresses.py --book ../citrate-chain/contracts/addresses/40204.json --genesis <G genesis hash> --rpc https://rpc.citrate.ai`, then `cargo test -p citrate-core --lib addresses` (`--with-inference-router` stays an owner call) | Mac lane | blocked on B1 |
| B4 | Rebuild the node sidecar from G (`scripts/build-sidecar.sh`); `citrate consensus` fingerprint of the bundled node equals the fingerprint DGX posts for G; record both: fingerprint `[ ]` | Mac lane, DGX | blocked on B1 |
| B5 | Rebuild every sidecar from current mains (runbook: 09-13 binaries predated R2) and refresh `runtime-deps` with sha256 pins; `scripts/ci/check-release-pins.sh` green | Mac lane, DGX | Mac part done (2026-10-06, v0.5.0-rc.1: all 8 macOS sidecars rebuilt or verified; citrate, mem-mcp, node-agent, llama-server uploaded and pinned; tripwire green); llama runtime tarball, Gemma GGUF, BGE tarball and knowledge corpus still unpinned (A9) |
| B6 | Federation consumers after G: citrate-docs #33, citrate-landing #86, citrate-sdk-js #21 regenerated from the same book and merged | Owner merges | in PR (each), blocked on B1 |
| B7 | Gates on the pinned head: `cargo fmt --all -- --check`, `cargo +1.98.1 clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, `npm run typecheck`, vitest, `scripts/ci/check-release-pins.sh` | Mac lane | done for v0.5.0-rc.1 (2026-10-06; EVIDENCE.md); re-run on the final head if anything lands after the tag |
| B8 | Rule-6 benchmark on the chain core for G (citrate-chain `tests/load` benchmark-suite) | DGX | blocked on B1 |

## C. Prove it

| # | Step | Owner | Status |
|---|---|---|---|
| C1 | 2,000-block soak on the rerolled 40204 with the 0.5.0 node build (B4) | DGX | blocked on B1 |
| C2 | macOS native run: in-app update 0.4.2 to 0.5.0 on the rerolled chain (S0.6); old chain data cleared, keys kept, the one-time notice shown; startup barrier and lock check observed (S8.5a); process list after quit (S0.5) | Owner (Mac) | blocked on B7, internal signed build |
| C3 | Linux native run on the DGX: manual package update 0.4.2 to 0.5.0 (S0.6); process list after quit (S0.5) | DGX | blocked on D3 |
| C4 | Windows native run: manual NSIS install of 0.5.0 over 0.4.2 with the old app and Hermes running; task list before and after (S0.7, #249); process list after quit (S0.5) | Windows team | blocked on D4 |
| C5 | Chain-compatibility checks on the soak chain (see the gate table: g4-identity, the g4-precompiles slice): AgentSBT minted by a member through the ceremony; one call per agent precompile answers | Owner (packaged app) | blocked on C1 |
| C6 | Hands-on QA of the signed, notarized Mac release candidate (the 0.4.2 gate pattern) | Owner | blocked on D1 |
| C7 | Record every native run in [EVIDENCE.md](EVIDENCE.md); flip `g1-precut`, `g3-recorded-cleanup-v050`, `g4-native-v050`, `g5-release-coupling` (SCL) and `g5-scl` (HUP) with evidence paths | Mac lane | blocked on C2 to C4 |

## D. Build, sign, publish

| # | Step | Owner | Status |
|---|---|---|---|
| D1 | macOS build per the runbook: stage the full BGE model, `TAURI_SIGNING_PRIVATE_KEY` (v2 key, 8EC382DD107F08C5) + `LZMA_API_STATIC=1` + `npx tauri build --config src-tauri/tauri.bundle-lite.conf.json`; probe the key first | Mac lane, Owner (key) | blocked on B7 |
| D2 | Notarize the DMG (`notarytool --keychain-profile citrate`), staple DMG and app, `spctl` says Notarized Developer ID; size gate `node scripts/size-budget.mjs`; record DMG and tar.gz sha256 | Mac lane, Owner | blocked on D1 |
| D3 | Linux x86-64 and ARM64 AppImage and .deb on the DGX (RELEASE_LINUX.md); Mac signs the updater artifacts with `tauri signer sign` and hands back `.sig` | DGX, Mac lane | blocked on B7 |
| D4 | Windows x64 NSIS installer (RELEASE_WINDOWS.md), Authenticode signed; no updater artifacts | Windows team | blocked on B7 |
| D5 | Release PR `release/0.5.0-hermes-upskill` to `main` (squash only; 2 code-owner approvals, `ci` and `secret-scan` green). `main` is at 0.4.3 (Windows fixes #149, never published); resolve the version to 0.5.0 | Mac lane; Reviewers; Owner merges | blocked on C2 to C7 |
| D6 | Release body from [docs/releases/v0.5.0.md](../../../../docs/releases/v0.5.0.md) with the bracketed values filled from B3 and B4, headed by the [upgrade notice](../../../../docs/releases/v0.5.0-upgrade-notice.md). Owner decides whether to mark it `[critical]` (as 0.4.0 was for the last network restart), which makes the update download itself | Owner | pending owner decision |
| D7 | `gh release create v0.5.0 --target <main sha>` with the DMG, `Citrate-Core.app.tar.gz` + `.sig`, Linux assets, Windows installer, `SHA256SUMS`, `PROVENANCE.txt`, `latest.json` | Mac lane | blocked on D5, D6 |
| D8 | Update feed: `latest.json` lists only platforms whose artifacts are 0.5.0 (darwin-aarch64, linux-x86_64, linux-aarch64; no Windows entry) | Mac lane | blocked on D7 |
| D9 | DO mirror (`aws --profile citrate-spaces`): DMG to `downloads/Citrate-Core-macos-arm64.dmg`, updater files and `latest.json` (Space URLs) to `downloads/updater/`, Linux and Windows downloads; CDN flush; parity gate (download sha256 = asset, `latest.json` version = 0.5.0, mirror tar.gz = asset) | DGX | blocked on D7 |
| D10 | Order with the RPC cutover: 0.4.x apps show the retired-network gate as soon as rpc.citrate.ai serves the new genesis, so the 0.5.0 feed and downloads must be live by then | Owner, DGX | pending |
| D11 | Member notice: the [upgrade notice](../../../../docs/releases/v0.5.0-upgrade-notice.md) on the release page and in member comms (DGX channels) | Owner, DGX | blocked on D9 |
| D12 | Post-release: an installed 0.4.2 shows Update available and restarts into 0.5.0; a fresh download launches without a Gatekeeper warning; merge `main` back into the integration branch; journal and retro per the release plan | Owner, Mac lane | blocked on D9 |

## HUP gate criteria not met today (2026-10-06)

Read from [HUP gates.yaml](../../../planset/2026-09-30-hermes-upskill/gates.yaml) on
`release/0.5.0-hermes-upskill` @ 835bef6. Default rule (owner brief): carry anything that is not a
safety or chain-compatibility item. **Every classification below is pending owner confirmation.**
gate0 and gate2 are met in full.

| Criterion | Classification | One-line reason |
|---|---|---|
| g1-approval-audit | **needed for 0.5.0** | Safety: every effectful chat tool must route through approval; the enumeration test passed on #191, but re-run it on the release head, which adds terminal commands (#236), then flip |
| g1-injection | carried to 0.5.1 | Measured (T0 12/12, T1 11/12 on #194); thresholds are an owner call, and every effectful action still needs a HIC approval. Owner may want this one in 0.5.0 since terminal access is on by default |
| g1-no-block | carried to 0.5.1 | Responsiveness, not safety; evidence on #191, only the flip is missing |
| g1-render | carried to 0.5.1 | Markdown golden test and streaming definition (owner call); presentation only |
| g1-sidebar | carried to 0.5.1 | Screenshots only; nav tests pass |
| g1-loop | carried to 0.5.1 | One remaining gap (an idle view does not drive core-hosted tools for a CLI or MCP turn); not safety |
| g1-wallet-unlink | carried to 0.5.1 | Depends on the identity deploy (identity#31); the feature is not in 0.5.0 (#117, #232 held) |
| g1-eval | carried to 0.5.1 | Measurement thresholds are owner calls; T2 run is external |
| g3-updater | carried to 0.5.1 | The signed component channel serves the managed browser and tools, which ship off; with no valid manifest the browser stays off the open web (S5.5). The app updater is separate and live |
| g3-licence | **needed for 0.5.0** | Distribution: docs/RELEASE.md requires the owner's licence sign-off (`--require-sign-off`) before the first public 0.5.0; not safety or chain, so the owner may waive it |
| g3-browser | carried to 0.5.1 | Browser ships behind a switch, off by default |
| g3-e2e | carried to 0.5.1 | Needs a member-approved deploy on the live new chain, after launch |
| g4-identity | **needed for 0.5.0** | Chain compatibility: the notes claim member-minted AgentSBT; prove one member mint on the rerolled chain during the soak (C5) |
| g4-anchor | carried to 0.5.1 | O-5 (anchor gas) is an owner decision; first live anchor after launch |
| g4-precompiles | **needed for 0.5.0 (slice)** | Chain compatibility: one call per agent precompile answers against the 0.5.0 node on the soak chain (C5); the full model and LoRA end-to-end run is carried to 0.5.1 |
| g4-fleet | carried to 0.5.1 | Two-machine run and CL-S4 sign-off are external; mesh stays off |
| g4-fl | carried to 0.5.2 | Live learning round needs GPU devices and owner round rules; off |
| g5-everyday | carried to 0.5.1 | Video backend, OAuth client and packaged evidence; not safety |
| g5-size | carried to 0.5.1 | Budgets await owner sign-off; record the measured 0.5.0 sizes at D2 |
| g5-os | carried to 0.5.1 | Clean-install hello-mint on three OSes; 0.5.0's native runs are C2 to C4 |
| g5-redteam | **needed for 0.5.0 (slice)** | Safety: the release plan never ships with an open High in the red-team register; confirm none is open for anything 0.5.0 ships. The full final multi-repo pass carries to 0.5.1 |
| g5-scl | **needed for 0.5.0** | This is the v0.5.0 SCL content: SCL criteria with `release: v0.5.0` (g1-precut, g3-recorded-cleanup-v050, g4-native-v050, g5-release-coupling); g3-provider-routing-v050 (S7.5a) moved to v0.5.1 (#254); *Owner decision (2026-10-06): back in v0.5.0, so g3-provider-routing-v050 is also needed* |
| g5-docs | carried to 0.5.1 | Retro "at close" and the essay follow the release; the release notes are in this PR |

Release hygiene items from the release plan that still apply to 0.5.0: test counts recorded (A8,
B7), clippy, fmt, the tripwire and the zero-unwrap check green (B7), and a recorded security
sign-off for the @rule8 items 0.5.0 ships (updater key use for this release).

## Blocking right now

1. G and the new address book (B1, federation #289).
2. SCL-S0 merges: #256, #257, #258, and the private runtime items (A6), then the Hermes rebuild (A7).
3. Owner decisions: the gate classifications above, g3-licence sign-off or waiver, `[critical]`.
