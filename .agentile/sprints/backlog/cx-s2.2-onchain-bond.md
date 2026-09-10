---
created: 2026-08-27
updated: 2026-08-27
branch: chore/close-cx-s1-s2
author: Claude Opus 4.8, directed by @SaulBuilds
status: DONE — shipped in citrate-core PR #158 (2026-08-27)
sprint: CX-S2.2 (carved out of CX-S2, Lane B)
tier: T1 (money-path, @rule8)
blocked_on: IPFSIncentivesV3 redeploy on 40204 with the #170 bytecode/ABI
companions:
  - docs/FINDING_PIN_COMMD_BOND_2026-08-26.md
  - citrate-chain#170 (fix MERGED in PRs #171-#178)
---

# Backlog — CX-S2.2: ceremony-gated IPFSIncentivesV3 bond client

> ✅ **DONE** — shipped in citrate-core **PR #158** (2026-08-27), after the chain redeploy
> (IPFSIncentivesV3 = `0xa1a37f79…` on 40204). `storage_pin` computes the canonical
> commitments via `citrate-commd` (byte-exact vs the frozen vectors) and submits
> `registerModel` as a PENDING SignatureCeremony. gateA-storage MET; Lane B closed.
> @rule8: end-to-end broadcast vs the live contract still wants security sign-off.
> Kept for the record; the spec below is the as-built reference.

## Status update 2026-08-27 — the chain FIX landed; the DEPLOY has not

citrate-chain#170 is fixed and merged (#171–#178): a canonical CommD + a SOUND recursive-fold
wrong-CommD challenge (Poseidon-BN254 / Nova, precompile 0x0130). My original finding is fully
addressed — an honestly-registered bond is no longer grief-slashable.

**BUT the fixed contract is NOT deployed.** The IPFSIncentivesV3 at `0xb024ad0b…2df4` on 40204 is
the STALE pre-#170 contract (14-arg constructor, `registerModel` WITHOUT `dataCommit`). The #170 fix
changed the constructor (added `foldVerifier`) and the ABI, so the deployed bytecode no longer
matches source and its CREATE2 address will MOVE on redeploy. **S2.2 is therefore gated on a chain
redeploy, not on more design.**

## The client recipe (now fully specified — from the #170 chain mapping)

Registration is LIGHTWEIGHT: compute three 32-byte values from the file bytes — **no proof, no
precompile, no GPU** (only a *challenger* needs the heavy fold proof).

```
commD      = citrate_commd::compute_comm_d(&file_bytes)      // Poseidon-BN254 Merkle over 31-byte-packed leaves
dataCommit = citrate_commd::compute_data_commit(&file_bytes) // domain+length-bound Poseidon sponge
dataHash   = keccak256(&file_bytes)
registerModel{value: >= MIN_MODEL_BOND}(cid, commD, dataHash, dataCommit, dataUri)
```

- **Crate:** `citrate-commd` (citrate-chain workspace member) — lean by design (ark-bn254 + Poseidon,
  NO halo2/Nova), so `src-tauri` can depend on it without the proving stack (the module doc names the
  CX-S2.2 desktop client as an intended consumer). `compute_comm_d`/`compute_data_commit` are pure
  library fns. Frozen test vectors: `crates/citrate-commd/tests/commd_frozen_v1.rs` (verify against
  these in a unit test).
- The registration path has NO staticcall/verify — the three values are trusted at registration and
  only tested if challenged. So the client never touches 0x0130.
- Intended params after redeploy: `MIN_MODEL_BOND = 55 ether`, `COMMD_CHALLENGE_WINDOW = 302400`,
  `COMMD_CHALLENGER_BPS = 5000`, `foldVerifier = 0x0130`.

## Reopen criteria (one-WP finish once the contract is redeployed)

1. **Chain (owner/chain-team):** redeploy IPFSIncentivesV3 on 40204 with the #170 15-arg constructor
   (`foldVerifier`); update `contracts/addresses/40204.json` + the federation manifest with the NEW
   address. (0x0130 + the trusted-setup ceremony are needed only for the CHALLENGE/slash side, not
   for registration — registration works the moment the new contract is live.)
2. **citrate-core (this repo, ready to build now — can be built deploy-ready + unit-tested):**
   - Add `citrate-commd` as a src-tauri dep; a unit test asserting `compute_comm_d`/`compute_data_commit`
     match `commd_frozen_v1.rs` vectors.
   - Wire `storage_pin(cid, bondSalt)` → retrieve the bytes for `cid` (kubo `cat`, S2.1 seam) →
     compute (commD, dataCommit, dataHash) → build `registerModel(...)` calldata → submit as a
     `SignatureIntent` (kind Transaction, value = bond) through the SignatureCeremony (mirror
     `node_register_validator`). Thread `pinning`/`challenged` into `PinRow`.
   - Read the IPFSIncentivesV3 address from the pinned address book; until it points at the redeployed
     contract, `storage_pin` returns an honest "network bond contract not yet deployed" (no fake tx).
   - Update `StorageFiles.tsx` from the "forthcoming" copy to the live network-bond flow.
   - Est: M–L (client-only; heavy correctness is the calldata + the citrate-commd parity test).
