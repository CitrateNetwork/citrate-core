---
created: 2026-10-01T18:00:00Z
branch: hup/n4-components (updated on hup/n6-web-browse, 2026-10-04)
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S5.5, HUP-S6.1
---

# citrate-components

The signed component updater (HUP-S5.5) and the toolchain bundle definitions (HUP-S6.1).
Policy, verification steps, the publishing runbook and the CVE SLA are in
[`docs/COMPONENT_UPDATER.md`](../docs/COMPONENT_UPDATER.md); this file only maps the directory.

| Path | What it is |
|---|---|
| `src/key.rs` | The pinned component key. The production slot is **empty** until the key ceremony, so every update is refused |
| `src/manifest.rs` | Manifest types, signature checks, field rules, anti-rollback |
| `src/install.rs` | Staging, verify, unpack, health check, swap, rollback, recovery |
| `src/extract.rs` | raw / tar.gz / tar.xz unpacking with path and link checks (zip is refused for now) |
| `src/policy.rs` | Client side of the CVE SLA (stale and expired manifests) |
| `src/bundle.rs` | Bundle checks and `manifest_from_bundle` for the release step |
| `src/main.rs` | CLI: `check-bundle`, `manifest-from-bundle`, `verify-manifest`, `unpack` |
| `toolchain-bundle.json` | solc, foundry, python, slither, aderyn, medusa, node per OS; the managed browser (`chromium`, Chrome for Testing, HUP-S5.1) and local search (`searxng`, AGPL-3.0-or-later, HUP-S5.2); Solidity library archives |
| `locks/slither-0.11.6-macos-arm64-cp312.txt` | The hash-locked wheel closure for slither on macOS arm64 |
| `locks/searxng-2026.10.4-macos-arm64-cp312.txt` | The hash-locked wheel closure for SearXNG on macOS arm64, plus the build-only wheels |
| `licenses/` | Licence texts of the vendored Solidity libraries at their pinned commits |

The library commits are not copied here: `toolchain-bundle.json` points at
[`templates/deps.lock.json`](../templates/deps.lock.json), and `check-bundle` fails if an
archive URL does not name the pinned commit.

No binary is committed. Measured means downloaded and hashed on macOS arm64 (2026-10-01 for the
toolchain, 2026-10-04 for Chrome for Testing), with each tool run once from its archive. The
Chrome for Testing zip cannot be installed by this version (zip is refused, see `src/extract.rs`),
and whether its archives may be re-hosted is pending owner and legal sign-off; both are in the
bundle entry's note.

```sh
cargo test -p citrate-components
cargo run -p citrate-components -- check-bundle --repo .
```
