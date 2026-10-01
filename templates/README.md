---
created: 2026-10-01T00:00:00Z
branch: hup/n3-templates
author: Larry Klosowski + Claude Opus 5.5
status: active
wp: HUP-S6.2, HUP-S6.9
---

# Contract and dApp templates

The starting points for the dApp forge (planset `2026-09-30-hermes-upskill`,
02_ARCHITECTURE section 7). Hermes fills one of these with the interview answers
and hands the result to the toolchain (forge, slither, aderyn, medusa) and the
deploy gate. This directory is source data plus a small renderer. It will be
bundled with the app as resources later; nothing in the app calls it yet.

| Template | What it is | Library |
|---|---|---|
| `erc20` | Fixed-supply ERC-20 with permit, whole supply minted to the owner address | OpenZeppelin v5.7.0 |
| `erc721` | Paid ERC-721 mint: supply cap, fixed price, 10 per transaction, base URI, withdraw. The hello-mint contract | OpenZeppelin v5.7.0 |
| `erc721-solady` | The same contract and interface on Solady | Solady v0.1.26 |
| `erc1155` | Paid ERC-1155 mint, per-id cap, fixed price per unit | OpenZeppelin v5.7.0 |
| `governor` | ERC-20 votes token and a Governor (4% quorum, 1 day delay, 1 week period, timestamp clock, no timelock) | OpenZeppelin v5.7.0 |
| `hello-mint` | `erc721` under `contracts/` plus a vite + React + wagmi + viem mint page under `app/` | |

Every contract template ships:

- `src/`: the contract;
- `test/*.t.sol`: forge unit and fuzz tests;
- `test/Properties.sol`: the Medusa property harness (`property_*` functions);
- `test/Invariants.t.sol`: the same harness run as Foundry invariants, so the
  template invariants are checked even where Medusa is not installed;
- `medusa.json`: Medusa configuration with the tier's call budget;
- `foundry.toml`, `remappings.txt`: solc 0.8.36, evm cancun (as citrate-chain).

## Parameters

| Key | Rule |
|---|---|
| `name` | 1 to 48 ASCII letters, digits, single spaces or hyphens, starting with a letter. The contract is named after it in PascalCase (`Lemon Drops` becomes `LemonDrops`); names that would shadow an imported symbol are refused |
| `symbol` | 1 to 11 characters A-Z or 0-9, starting with a letter |
| `supply` | Whole number in the template's range (see each `template.json`) |
| `price` | Wei, whole number, at most 10^30. `0` is a free mint |
| `owner` | `0x` + 40 hex digits, not zero; mixed case must be a valid EIP-55 checksum. Written checksummed |

None of the accepted alphabets can close or escape a string literal in Solidity,
TypeScript, JSON or HTML, so values are substituted without context-specific
escaping. Unknown keys, unknown placeholders and unterminated placeholders are
errors, and a failed render writes nothing.

## Renderer

`renderer/` is the `citrate-templates` crate (library and CLI, no Tauri
dependency). It is in Rust because its callers are the Tauri backend and the
toolchain sidecar, and because it writes files on the user's disk.

```sh
cargo run -p citrate-templates -- list --root templates
cargo run -p citrate-templates -- render --root templates --template hello-mint \
  --tier T1 --out /path/to/empty/dir \
  --param "name=Lemon Drops" --param symbol=LEMON --param supply=500 \
  --param price=5000000000000000000 --param owner=0x...
```

Placeholders are `{{ct:key}}`; other `{{` text passes through (JSX uses
`style={{ ... }}`). Layers under `_common/` hold files shared by several
templates. Each render writes `citrate-template.lock.json`: template id, a sha256
digest of the template sources, the tier, the normalized parameters, the Medusa
budget and the dependency pins.

## Dependencies

Pinned in `deps.lock.json` (OpenZeppelin v5.7.0, Solady v0.1.26, forge-std
v1.17.0, each by full commit). The renderer does not fetch them: the toolchain
bundle (HUP-S6.1) is meant to vendor these commits, and the gate script fetches
them.

## Medusa budgets (HUP-S6.9)

`medusa-budgets.json` holds one budget per hardware tier, counted in fuzzer calls:

| Tier | Calls (`testLimit`) | Workers | Sequence length | Coverage plateau | Minimum coverage |
|---|---|---|---|---|---|
| T0 | 10,000 | 2 | 50 | 2,500 calls | 60% |
| T1 | 50,000 | 4 | 50 | 10,000 calls | 75% |
| T2 | 200,000 | 8 | 100 | 40,000 calls | 80% |

Calls, workers, sequence length and the wall-clock ceiling (`timeout_secs`) go
into `medusa.json` and Medusa enforces them. The coverage plateau and minimum
coverage are not Medusa options: they are recorded in the lock file for the
toolchain runner (HUP-S6.3) and the deploy gate (HUP-S6.4), which do not exist
yet. The numbers are starting values to re-measure on real T0, T1 and T2 hardware.

## Gate

```sh
templates/scripts/verify-templates.sh --work <scratch dir> --deps-cache <cache dir> \
  [--node-modules <dir with the app's packages>] [--medusa] [template ids...]
```

Renders every template, fetches the pinned dependencies (checking each commit),
runs `forge build` and `forge test` on every contract project, and with
`--node-modules` runs `tsc --noEmit` and `vite build` on the hello-mint app.
`--medusa` also runs `medusa fuzz` when Medusa is installed and says so when it
is not.

## Not here

No deploy logic, no signing and no network calls from the renderer. Deploy goes
through the deploy gate (HUP-S6.4) and the SignatureCeremony; switching a live
site to chain 40204 after deploy is HUP-S6.6. The hello-mint page sends mint
transactions through the visitor's own wallet and holds no keys.
