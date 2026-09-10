---
created: 2026-07-16
branch: feat/core-wallet-activity
author: Claude Opus 4.8, directed by @SaulBuilds
status: building — real wallet activity (tx history) from the CitrateScan indexer
program: CORE finish-list item 4 — wallet_activity
rule8: NO — a PUBLIC, no-auth, read-only tx-history fetch (no key/money/signing/identity custody)
grounded_in:
  - citrate-explorer (CitrateScan) public API — VERIFY the exact route + response shape against source before encoding
  - the RPC has NO address-history method (no citrate_getAddressTransactions/ots_/trace_) — the indexer API is the only source
  - src/shell/state.ts Activity {id,kind,amount,hash,ts}; Wallet.tsx activity tab (amount leads "−" for outgoing)
---

# CORE — real wallet activity (finish-list item 4)

Today `wallet.activity` is an honest `unavailable` stub in tauri; the Activity tab
shows sim seed + local optimistic entries (store.addActivity on Send/Stake/etc.).
CitrateScan indexes 40204 and exposes a PUBLIC address-tx API → wire real history.

## STEP 0 (do FIRST): verify the explorer API contract against source
Read citrate-explorer source and CONFIRM (do not trust this doc's shape):
- The Etherscan-compatible route `src/app/api/v1/route.ts` (GET `?module=account&
  action=txlist&address=0x..&limit=`) AND/OR `src/app/api/address/[addr]/route.ts`.
- The EXACT response JSON: the Etherscan envelope `{status:"1"|"0", message, result:[...]}`
  and each tx's fields (hash, from, to, value(wei), timestamp, status(1/0/null),
  methodId, blockHeight, createdContract). Confirm the `provisioned:false` / "no
  records" honest fallback shape. Confirm it is PUBLIC (no auth) + the rate limit.
Pin the base URL to the canonical host: `https://explorer.citrate.ai`.

## Build

### Rust — new `activity.rs` (NOT @rule8; a public read). Rust-origin (ureq) to avoid webview CORS + match rpc/oidc/ai pattern.
- `EXPLORER_BASE = "https://explorer.citrate.ai"`. `wallet_activity(address)` command:
  validate the 0x-20-byte address; `ureq` GET the txlist endpoint; parse the envelope.
  Map each tx → `ActivityEntry { id(hash), kind, amount, hash, ts, status, direction }`:
  - direction: from==self → "out", to==self → "in", both → "self".
  - kind: "Sent"/"Received"/"Self"; if methodId present & non-"0x" & to is a contract-ish
    call → "Contract call" (keep simple + honest; don't over-claim a method name unless
    trivially known). Do NOT fabricate a decoded method.
  - amount: value wei → SALT string, prefixed "−" (U+2212) if outgoing, "+" if incoming
    (matches the Wallet tab's sign convention). Zero-value → no sign.
  - ts: the tx timestamp (unix seconds → ms for the JS `rel()` helper, or return seconds
    and let JS scale — match what the Activity `ts` field expects: epoch ms).
  - status: pass through (1/0/null) so the UI can mark failed txs.
  Honest handling: indexer-not-provisioned or `status:"0" / "No records"` → return an
  EMPTY list (not an error, not fabricated) with a flag the UI can show ("indexer syncing /
  no transactions"); a transport/HTTP failure → Err (the store keeps last-honest + shows
  an honest error). NEVER fabricate a tx. Coarse errors. Zero prod `.unwrap()`.
- Injectable HTTP seam (mirror rpc.rs RpcTransport / ai.rs http seam) so tests script
  responses without a live socket. Register `activity::wallet_activity` in lib.rs.

### Frontend
- bridge WalletDomain.activity (tauri) → invoke `wallet_activity(address)` → map to Activity[];
  sim keeps `s().activity`. Honest empty/error (no fabricated rows).
- store: `refreshActivity()` folds the real list into `state.activity` (call on Wallet mount +
  after a settled ceremony, alongside refreshWallet/refreshPendingWithdrawals). Keep
  `addActivity` as an OPTIMISTIC prepend for a just-sent tx, DEDUPED by hash against the
  fetched list so a real indexed entry doesn't duplicate the optimistic one.
- Wallet Activity tab already renders `s.activity` — no UI rewrite; just ensure failed txs
  (status 0) render distinctly if easy, and the empty state is honest ("no transactions yet"
  vs "indexer unavailable").

## Acceptance
- In a Tauri build, the Activity tab shows the member's REAL indexed 40204 tx history from
  CitrateScan (hash/direction/amount/time/status), not a sim list. A fresh address shows an
  honest empty state; the indexer being down shows an honest message; nothing is fabricated.
- Just-sent txs (optimistic) don't duplicate once indexed (dedupe by hash).

## Non-negotiables
Rule 1 (name the data source — the CitrateScan txlist endpoint — in comments; no fabricated
txs; honest empty/failure). Zero prod `.unwrap()`. Test count monotone. cargo `--no-default-features`.
Build-and-stop; NO self-review; NO merge.
