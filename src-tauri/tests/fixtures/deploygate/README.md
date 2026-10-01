---
created: 2026-10-01
branch: hup/n3-deploy-gate
author: Larry Klosowski + Claude Opus 5.5
status: active
---

# Deploy gate fixtures (HUP-S6.4)

Inputs for `src-tauri/src/deploy_gate_tests.rs`. They come from a two-contract project
(`HelloMint`, a minimal owner-mint token, and `Drain`, which forwards its whole balance to
any caller-chosen address so Slither reports a High finding). Absolute paths in the outputs
were rewritten to `/work/hello-mint`.

| File | Source |
|---|---|
| `forge-pass.json` | real: `forge test --json`, forge 1.5.1, solc 0.8.28, 2 tests pass |
| `forge-fail.json` | real: the same with one deliberately wrong test (1 failure) |
| `slither-high.json`, `slither-high.sarif` | real: slither 0.11.6 on both contracts (`arbitrary-send-eth`, High) |
| `slither-clean.json`, `slither-clean.sarif` | real: slither 0.11.6 on `HelloMint` only (one Low) |
| `anvil-receipt.json`, `anvil-tx.json` | real: anvil 1.5.1, `cast send --create <initcode> --json` and `cast tx <hash> --json` |
| `initcode.hex` | real: `HelloMint` creation bytecode followed by one ABI-encoded address argument |
| `aderyn-*.handwritten.json` | hand-written: Aderyn is not installed on the build machine; the shape follows Aderyn's JSON report (`issue_count`, `high_issues.issues`, `low_issues.issues`) |
| `medusa-*.handwritten.log` | hand-written: Medusa is not installed on the build machine; the lines follow Medusa's console output (`fuzz: elapsed: …, calls: …`, `Test summary: N test(s) passed, M test(s) failed`) |

The hand-written fixtures must be replaced by real captures once Aderyn and Medusa ship
in the toolchain bundle (HUP-S6.1). Until then the parsers for those two tools are tested
only against these shapes.
