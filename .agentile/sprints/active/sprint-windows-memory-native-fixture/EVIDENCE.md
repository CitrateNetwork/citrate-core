---
created: 2026-10-04T00:00:00-07:00
branch: test/windows-memory-native-fixture
author: Sisyphus, directed by Kurt
status: active
sprint: WINDOWS-MEMORY-NATIVE-FIXTURE
---

# Verification evidence

## Pinned state

- Base branch: `main`
- Base SHA: `fc1d0b916524bfa5ea9f8058d6157184c64c9860`
- Host: Windows, `x86_64-pc-windows-msvc`
- Issue: `CitrateNetwork/citrate-core#215`
- Parent Windows tracking issue: `CitrateNetwork/citrate-core#51`

## Red evidence

```text
cargo test --lib memory::tests:: -- --nocapture
18 passed; 4 failed; 1 ignored

spawn failed: %1 is not a valid Win32 application. (os error 193)
```

Failing tests:

- `memory::tests::ingest_skips_when_tenant_is_nonempty_but_the_sidecar_seen_set_is_missing`
- `memory::tests::seed_context_authors_network_node_stake_facts_then_is_idempotent`
- `memory::tests::seed_context_skips_personal_when_no_grant`
- `memory::tests::store_on_disk_is_ciphertext_not_plaintext`

## Green evidence

```text
cargo test --lib memory::tests:: -- --nocapture
22 passed; 0 failed; 1 ignored

cargo fmt --all -- --check
PASS

cargo clippy --lib --tests -- -D warnings
PASS
```

Direct fixture checks also fail closed as required:

```text
stub_mem_mcp: CITRATE_MEM_STORE_KEY env required
stub_mem_mcp: CITRATE_MEM_STORE_KEY contains non-hexadecimal characters
```

Rust language-server diagnostics are clean for `memory_tests.rs`. The standalone
`stub_mem_mcp.rs` fixture reports only rust-analyzer's expected `unlinked-file` hint
because it is compiled directly with `rustc`; successful compilation is exercised by
every Memory fixture test.

## Full-suite observation

`cargo test --lib` completed with `526 passed`, `5 failed`, and `5 ignored`. Every
Memory test passed. The remaining failures are the two node-fixture tests addressed by
PR `#213`, the node-agent fixture test addressed by PR `#214`, and the two Comms timeout
tests addressed by PR `#180`. No failing test was deleted, disabled, or weakened.

## Current claim

The Memory test module is locally Windows-clean. Its 22 non-ignored tests pass with the
native fixture while production Memory and supervisor code remain unchanged. The work is
not yet committed, published, reviewed, merged, released, or packaged-app tested.
