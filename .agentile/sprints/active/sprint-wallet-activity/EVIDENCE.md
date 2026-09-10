---
created: 2026-07-16T00:00:00Z
branch: feat/core-wallet-activity
author: Claude Opus 4.8, directed by @SaulBuilds
sprint: sprint-wallet-activity
status: active
---

# CORE item 4 — real wallet activity (tx history) · evidence

Build-and-stop. Wired `wallet.activity` to REAL indexed 40204 tx history from the
CitrateScan `txlist` endpoint. NOT @rule8 (a public, no-auth, read-only fetch);
honest per Rule 1 (no fabricated txs; empty/error handled honestly; data source
named in code).

## STEP 0 — VERIFIED explorer API contract (against citrate-explorer source)

Endpoint coded against (pinned base `https://explorer.citrate.ai`):

    GET /api/v1?module=account&action=txlist&address=0x..&limit=25

- Source: `citrate-explorer/src/app/api/v1/route.ts` (`module=account` /
  `action=txlist` → `searchTransactions(address, 100)` over the Neon index in
  `src/lib/indexer/repository.ts`).
- **PUBLIC**: `/api/v1` allows ANONYMOUS access (`validateApiKey(null,…)` →
  `{anonymous:true, perSec:2}`) — no key required. We send no key.
- **Envelope** (Etherscan): `{ status:"1"|"0", message, result:[...] }`.
  - `status:"1"` + `result:[...]` → tx rows (newest-first, `desc(timestamp)`).
  - `status:"0"` + `result:[]` → "No transactions found" OR "No transactions found
    (indexer not provisioned)" — HONEST EMPTY (both). We return an empty list;
    the not-provisioned message sets an `indexer_unavailable` flag.
  - `status:"0"` + `result:null` → hard failure (e.g. "invalid address") → error.
- **Per-tx fields** consumed (raw drizzle `transactions` row, `src/lib/db/schema.ts`):
  `hash`, `from`, `to` (null ⇒ creation), `value` (wei string), `status`
  (int|null: 1/0/null), `methodId` (string|null), `createdContract` (string|null),
  `timestamp`.
- **Timestamp units — VERIFIED SECONDS.** `transactions.timestamp` = the block
  timestamp, `hexToNum(raw.timestamp)` (`src/lib/citrate/rpc.ts:101`) → UNIX
  **seconds**. The core JS `rel()` helper does `Date.now() - ts`
  (`src/surfaces/Wallet.tsx:20`), expecting epoch **ms**. Rust therefore emits
  `timestamp_seconds * 1000`.

### Deviation from the spec doc
- The spec listed `/api/v1` as key-gated in one place; SOURCE shows it allows
  anonymous access — so a public no-key read is correct. No `provisioned:false`
  JSON is exposed by this route; the not-provisioned case surfaces as the honest
  `status:"0"` "No transactions found (indexer not provisioned)" envelope, which we
  map to an empty list + `indexer_unavailable=true`. `blockHeight`/`createdContract`
  are present in the row but not needed for the Activity shape.

## Files changed
- NEW `src-tauri/src/activity.rs` — `EXPLORER_BASE`, `wallet_activity` command,
  `read_activity`, envelope parse, wei→SALT signed formatting, injectable
  `ActivityHttpClient` seam (`UreqActivityClient` prod).
- NEW `src-tauri/src/activity_tests.rs` — 18 tests + 1 `#[ignore]` live proof.
- `src-tauri/src/lib.rs` — `mod activity;` + registered `activity::wallet_activity`
  (removed `seam::wallet_activity` from the handler list).
- `src-tauri/src/seam.rs` — dropped the `wallet_activity` seam stub + updated tests.
- `src/bridge/tauri/index.ts` — `wallet.activity` invokes `wallet_activity`, maps
  rows → Activity shape.
- `src/shell/store.ts` — `mergeActivity` (dedupe by hash) + `refreshActivity()`;
  called on launch (tauri), Wallet mount, and after every settled ceremony.
- `src/surfaces/Wallet.tsx` — `refreshActivity` on mount; honest empty state by mode.
- `src/bridge/tauri.test.ts`, `src/bridge/sim.test.ts`, `src/shell/store.test.ts`
  — new tests (none removed).

## Gates (all green)
- `cargo test --no-default-features`: **297 passed** (was 279 → +18), 6 ignored.
- `cargo clippy --no-default-features --all-targets -- -D warnings`: clean.
- `cargo fmt --check`: clean (Cargo.toml untouched).
- prod `.unwrap()` in `src/` (frontend) and non-test Rust: **0** (only test code).
- `npx tsc --noEmit`: clean. `npx vitest run`: **125 passed** (was 119 → +6).
- Dep tree / Cargo.toml / cargo audit: **unchanged** (reused ureq/serde_json/hex/serde).

## Open concerns
- Rate limit: anonymous is 2/sec; we do one GET per refresh (launch/mount/settle) —
  well under. No retry/backoff added (a failed fetch keeps the last honest list).
- Failed-tx display: Rust passes `status` through, but the JS `Activity` type has no
  `status` field, so the row isn't yet visually marked as failed (amount sign/color
  still distinguishes out/in). Left out to avoid broadening the persisted state
  shape; a follow-up could carry `status`/`direction` into the UI.
- Dedupe: keyed on tx hash; an optimistic just-sent row (random hash from
  `addActivity` when no real hash yet) is replaced by the canonical indexed row once
  its real hash appears (settle paths pass the REAL `result.txHash` to addActivity).
