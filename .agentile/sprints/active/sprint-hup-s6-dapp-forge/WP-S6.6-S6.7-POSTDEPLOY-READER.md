---
created: 2026-10-01
branch: hup/n4-postdeploy-reader
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: HUP-S6.6, HUP-S6.7
issue: CitrateNetwork/citrate-federation#283
---

# HUP-S6.6 + S6.7: after the deploy, and the Contract reader

Planset: `.agentile/planset/2026-09-30-hermes-upskill/` (02_ARCHITECTURE section 7,
04_FEATURES_BDD US-6.1 tail and US-6.3, 05_SPRINTS_AND_WPS S6.6 and S6.7).

## What shipped

### S6.6: after the deploy (`src-tauri/src/postdeploy.rs`, `src/shell/PostDeployPanel.tsx`)

The After deploy panel (Agent, Contracts tab) takes a rendered hello-mint project folder and
the deploy transaction hash (prefilled from the last "Deploy contract" activity entry):

| Step | Command | Data source |
|---|---|---|
| Find the contract | `postdeploy_receipt` | `eth_getTransactionReceipt` on 40204 (`rpc.citrate.ai`). A reverted creation or a non-creation reports no contract |
| Verify | `postdeploy_verify` | `forge verify-contract --show-standard-json-input` in `contracts/`, then `POST https://explorer.citrate.ai/api/verify` (citrate-explorer `src/app/api/verify/route.ts`). Verified, partial, failed and unavailable (429, 503, transport) are reported as such |
| Switch the site | `postdeploy_switch_site` | Refused unless `eth_getCode` shows code at the address on 40204. Rewrites `app/.env.local` to `VITE_TARGET=citrate` and the EIP-55 address, keeping other lines |
| Pin to IPFS | `postdeploy_pin_site` | The app's bundled kubo API (`storage.rs` `CITRATE_KUBO_API` / `127.0.0.1:5001`): one multipart `add` of `app/dist` as a folder, pinned. IPFS not running: nothing is pinned and the member is told to start it under Storage |
| Export for Vercel | `postdeploy_vercel_export` | Writes `vercel-export/` (app sources without `node_modules`, `dist`, local env files or links; `vercel.json`; `.env.production` for 40204). Returns `npx vercel deploy --prod`; Citrate takes no account action |

`postdeploy_status` reports the contract name, the 40204 contract the page targets, whether
the page is built, and the export folder.

The template's `vite.config.ts` now sets `base: "./"` so the built page also loads under an
IPFS gateway path (`/ipfs/<cid>/`).

### S6.7: the Contract reader pop-out

- `popout.rs`: the `contract` kind now has a view (`available()`); still opened only by the
  main window. The pop-out capability is unchanged (bridge events only, no app commands).
- `src/popout/ContractReader.tsx`: open an address on chain 40204 or on a loopback fork URL,
  see CitrateScan's verified source and ABI (`GET /api/contract/{addr}`), or paste an ABI.
  Functions are listed reads first, each with what can be said for certain from the ABI
  (`contractReader/explain.ts`), Read (view calls), Propose in ceremony (writes, 40204 only),
  and Explain (Hermes).
- `src/popout/contractChannel.ts`: the reader's requests to the main window (`initial`,
  `source`, `codeSize`, `view`, `write`, `explain`). The main window re-checks every argument
  before running an op; malformed requests are refused without running anything.
- `src-tauri/src/contract_reader.rs`: `contract_source`, `contract_code_size`,
  `contract_view_call` (read target is 40204 or an `http://` loopback URL with no
  credentials; anything else is refused), `contract_write_propose` (40204 transaction
  intent, origin `local-user:contract-reader`, gas from `eth_estimateGas` plus 20 % headroom,
  capped at 15M; a failed estimate refuses the write with the node's reason).
- Writes: `store.proposeContractCall` opens the WalletReviewModal (`contract-call` review) in
  the main window. Calldata the ceremony decoder does not recognize needs the raw-mode
  acknowledgement, as for every other unrecognized call. Nothing signs outside the ceremony.
- Explain: `store.explainContract` runs one turn on the active provider with the function and
  the verified source near it fenced as untrusted data. Every tool call in that turn is
  answered "not available", so an explanation cannot act. It refuses with the demo provider
  and while a chat turn is running.

## Tests

- Rust (`cargo test --lib`): 660 passed + 1 failed at baseline (a socket test that passed on
  rerun) → 702 passed, 0 failed. New: `contract_reader_tests.rs` (18), `postdeploy_tests.rs`
  (22, including the anvil end-to-end test that skips without its environment), popout (+2,
  one test renamed for the new view).
- Vitest: 1003 passed, 9 skipped → 1050 passed, 10 skipped (the new skip is the anvil
  end-to-end test without its environment). New: `contractReader/abi`, `explain`,
  `popout/contractChannel`, `contractHost`, `ContractReader.render`, `PopoutRoot` (+2),
  `shell/contractReader.store`, `postDeployPanel.render`, bridge contract seams (+3).
- End to end: `scripts/e2e-postdeploy-reader.sh --work DIR --deps-cache DIR` renders
  hello-mint, builds it with the pinned dependencies, deploys it to a throwaway anvil (chain
  id 40204, anvil's unlocked dev account, no key in the repo), starts a throwaway offline kubo
  when `ipfs` is installed, then runs the Rust test (receipt → address, code size, `name()`,
  `MAX_SUPPLY()`, `totalMinted()`, a `mint` gas estimate, forge standard JSON, site switch,
  Vercel export, IPFS pin with the folder listing checked) and the TypeScript test (forge ABI →
  `parseAbi` → `encodeCall` → `eth_call` → `decodeResult`). Run on 2026-10-01: all green, CID
  `bafybeihqjqrdaxfnwzyyv2umds45waderh5j7iyvvgbrmqmao6zrv7dudm` for the stand-in site.
- Mutation checks (13 mutants, all killed after one added test): loopback-only read target,
  full-match-only verified state (needed the added test), reverted creation has no contract,
  export never replaces a folder Citrate did not write, links never followed when pinning,
  gas clamp, fork never proposes writes, demo provider cannot explain, explain refuses tools,
  channel wei check, untrusted fence in the prompt, negative uint refused.

## Not done here (honest state)

- Nothing was run against chain 40204 or the live explorer verifier: no hello-mint contract is
  deployed on 40204 yet. A read-only `GET /api/contract/{addr}` against the live explorer was
  checked to match the parser's shape.
- The page build (`npm run build`) is the member's (or a future workflow's) step; the panel
  says to rebuild after switching. The e2e uses a two-file stand-in for `app/dist`.
- Verification needs `forge` on the machine (the bundled toolchain, HUP-S6.1, will provide
  it). Without it the step says so.
- No Hermes tool exposes these steps yet; the hello-mint workflow (S6.5) can call the same
  commands. The planset's `getVerifiedSource` tool for the citratescan MCP is not added here;
  the reader uses the explorer's existing public contract endpoint.
- The browser pop-out preview of the switched site is S5.1's.
