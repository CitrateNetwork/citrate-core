---
created: 2026-10-01T11:00:00Z
branch: hup/n4-agent-sbt
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S7
wp: HUP-S7.4
---

# HUP-S7.4: AgentSBT mint at onboarding (US-7.1)

Planset `2026-09-30-hermes-upskill`: 04_FEATURES_BDD US-7.1, 05_SPRINTS_AND_WPS S7.4, D-24.

## What shipped

| Piece | Where |
|---|---|
| `mintAgent` calldata builder, DID + fingerprint derivation, read calldata, decoders | `src-tauri/src/agent_sbt.rs` |
| Readiness model (`assess`) and the chain gather (`gather`, generic over the RPC transport) | same |
| Commands `agent_sbt_status` (read) and `agent_sbt_mint` (pending SignatureCeremony), both async via `off_main`, both in the main-window ACL | `agent_sbt.rs`, `lib.rs`, `permissions/main-window.toml` |
| AgentSBT + OrganizationSBT added to the app address book as optional pins (regenerated with `sync-addresses.py --rpc`, code verified live) | `scripts/sync-addresses.py`, `src-tauri/addresses/40204.json`, `addresses.rs` |
| Onboarding step "S6.6 Hermes identity" with a "Give Hermes an identity" button | `src/onboarding/AgentIdentityStep.tsx`, `Onboarding.tsx` |
| Profile status: the Wallet "Registered agents" box shows the member's AgentSBTs read from chain | `Wallet.tsx` (`RegisteredAgents`) |
| Card model + calls | `src/identity/agentSbt.ts` |
| Anvil end-to-end test and its runner | `agent_sbt_tests.rs` (ignored test), `scripts/anvil-agent-sbt.sh` |

## How the step decides

Core gathers, in order: the address-book entry, `eth_getCode`, `balanceOf(member)`, the
parent organization's `isActive` (through the AgentSBT's own `orgContract()`), this install's
identity key, and an `eth_call` preflight of the exact `mintAgent` tx from the member's wallet.
The step is offered only when every one passes. A member who already holds an AgentSBT sees it
and is not offered a second one. Revert data is decoded from both node shapes (anvil's
`error.data`, the Citrate node's hex inside `error.message`).

Tokens are found from the mint `Transfer(0, member, id)` logs and read with `getAgent`, because
the contract has no `tokenOfOwnerByIndex`. If the log scan fails, the balance still shows and the
token list says it could not be read (never an empty list).

## State on 40204 today (read 2026-10-01)

AgentSBT is deployed at `0xd16b1ad6e744F3E92223C65F492c35D36ae07c7b` (owner: the CitAgent 2-of-3
timelock), but `OrganizationSBT.nextTokenId()` is 0, so `isActive(0)` is false. The step
therefore shows "Hermes identity is available after the network upgrade. The organization that
issues Hermes identities is not set up on chain yet." and the button is disabled. Once an
organization exists, the preflight from a member wallet reverts `OwnableUnauthorizedAccount`
(`mintAgent` is `onlyOwner`), and the step says identities are issued by the registrar today.
It turns on by itself when the chain allows a member's own mint.

## Pending owner sign-off (placeholders in code, marked there)

1. **Parent organization**: org `0`, overridable with `CITRATE_AGENT_PARENT_ORG_ID`.
2. **Issuance path**: `mintAgent` is issuer-only. Either a member-callable mint in the contract
   (a chain change plus redeploy) or a registrar service that mints on request.
3. **Identity key**: `pubkey_fingerprint = sha256(ed25519 pubkey)` (the runtime's
   `signer_id_from_pubkey`). Hermes has no key of its own, so the placeholder uses this node's
   ed25519 proposer public key.
4. **DID format**: `did:citrate:agent:<member address, lowercase>`, keccak-256 to `bytes32`.

## Proof

- `cargo test --lib agent_sbt`: 36 passed, 1 ignored (the anvil test).
- `scripts/anvil-agent-sbt.sh`: builds AgentSBT + OrganizationSBT from citrate-chain `0aab474b`,
  deploys them on anvil 1.5.1, and checks OrgNotActive, NotIssuer (member), Ready (issuer), the
  mint through `mint_agent_calldata` + `mint_tx_json`, then Minted with token #0, its DID hash and
  fingerprint: 1 passed.
- Calldata is byte-exact against `cast calldata`; the revert fixtures are real node answers.
- Mutation check: five mutants of `agent_sbt.rs` (availability rule, held threshold, error
  selector, gas margin, word encoding), all killed.
- `addresses.rs`: one new test for the optional pins. Vitest: 20 new tests
  (`agentSbt.test.ts`, `agentIdentityStep.test.tsx`).

## Not done

- No transaction was sent on 40204 (none can succeed today, and none is allowed from here).
- The registrar request route (owner decision 2) is not built.
- Explorer visibility (US-7.1 AC2, explorer half) belongs to citrate-explorer.
