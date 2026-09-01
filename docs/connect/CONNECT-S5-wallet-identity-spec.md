---
created: 2026-08-31
branch: feat/connect-s5-wallet-identity
author: Claude (Opus 4.8), directed by @SaulBuilds
status: spec (implemented)
planset: connect-realign
code: CONNECT-S5
repo: citrate-core (+ kit)
companions:
  - docs/CONNECT_REALIGN_PLANSET.md
  - src-tauri/src/comms.rs
  - kit/src/wallet.rs
---

# CONNECT-S5 — wallet-derived portable comms identity

## The user & the outcome

A member signs in with their wallet on ANY Mac and is automatically the same person in all their
groups — connectivity as easy as a social page. Reinstalling, or moving to a second machine, no longer
drops them out of the groups they were invited to.

## Why this step exists (the bug it fixes)

The node's comms identity — the address group rosters, the relay key-package directory, and mailboxes
all key on — was a **random secp256k1 key minted per device** and sealed in that Mac's keychain with,
by design, no backup and no derivation from the wallet (`comms.rs` "Option A", 2026-08-27). So a
reinstall on another Mac minted a *different* comms address; the seat an invited member earned was
keyed to their first device's address, and the new install was, to the group, a stranger. Confirmed
root cause of "I reinstalled on my other Mac and couldn't find the user in the group they were invited
to." Owner decision (2026-08-31): make identity follow the wallet ("wallet = your account").

## What changed

Identity is now **derived deterministically from the custody wallet**, so the same wallet yields the
same comms address on every device.

- **`kit/src/wallet.rs` — new `pub fn derive_scoped_secret(vault, info) -> Zeroizing<[u8;32]>`**:
  HKDF-SHA256 over the sealed BIP39 entropy (salt `SCOPED_SECRET_SALT`, domain = `info`), guaranteed
  a valid secp256k1 scalar (deterministic counter re-expansion on the ~2^-128 bad-draw). It is **not a
  signature** (one-way KDF), so it carries no signing authority and is deliberately OUTSIDE the Rule-3
  ceremony mandate that gates `sign_message`/`sign_transaction`/`sign_personal` — a background module
  (the lazy comms/cluster daemon start) can call it without an interactive human ceremony, which the
  signature path could not provide without deadlocking startup. The wallet key never leaves the vault;
  only this scoped, one-way-derived key does. The signer-reachability guard
  (`ceremony_tests::adv1_adv7_signer_only_reachable_via_approve`) is unaffected (still passes).
- **`src-tauri/src/comms.rs` — `provision_comms_seed` loads-or-derives**: on first use it calls
  `vault.ensure_auto_unlocked()` (device-bound, no passphrase in the beta model) then
  `wallet::derive_scoped_secret(vault, COMMS_IDENTITY_INFO)`, and caches the result in the OS keyring
  under a NEW account `comms-member-key-v2`. `load_or_derive_comms_seed` takes the derivation as an
  injected closure so it stays a pure keyring unit under test.
- Everything downstream is unchanged: `address_from_secret_hex`, the daemon seed file, the cluster
  Noise/peer id, `groups_self_address`, and the invite-claim address all already key on
  `address_from_secret_hex(seed)`, so the same wallet → same address flows through automatically.

## Data-source trace (Rule 7)

| Value | Source |
|---|---|
| comms seed (first use) | `wallet::derive_scoped_secret` = HKDF-SHA256(sealed BIP39 entropy, `COMMS_IDENTITY_INFO`) — deterministic in the wallet |
| comms seed (later) | cached in OS keyring account `comms-member-key-v2` |
| comms address (roster key) | `address_from_secret_hex(seed)` = `keccak256(uncompressed_pubkey[1..])[12..]` (unchanged) |
| wallet entropy | custody vault slot `wallet-entropy-0`, device-bound auto-unlock (`custody::ensure_auto_unlocked`) |

## Acceptance criteria

- [x] Same wallet → same comms address on any device (unit test: two vaults with the same mnemonic
  derive the same secret). `derive_scoped_secret_is_deterministic_domain_separated_and_valid`.
- [x] Different wallet → different identity; different domain → different secret; output is a valid
  secp256k1 scalar. (same test)
- [x] Fails CLOSED: locked/absent wallet → `WalletNotReady` (never a random throwaway identity);
  unreachable keyring → hard fault; corrupt cached key → hard fault. Tests:
  `derive_scoped_secret_fails_closed_when_no_wallet_or_locked`,
  `comms_seed_fails_closed_when_wallet_not_ready`, `comms_seed_rejects_a_corrupt_stored_key`.
- [x] Cached after first derive; not re-derived on restart. `comms_seed_is_derived_sealed_and_stable_across_restarts`.
- [x] Rule 3 intact — derivation is not a signer; the ceremony signer-reachability guard still passes.
- [x] Suites green: kit 200 (+2), app 371 (+1 net on comms seed tests).

## Migration & operational note

- Versioned migration: v2 keyring account; the legacy random `comms-member-key` is no longer read
  (kept as a name for observability/rollback). Existing installs derive a NEW (now stable) address
  once → **each existing member needs a single re-invite**, after which identity is portable forever.
  Acceptable because the comms path is reroll-insensitive, nothing on-chain anchors the address, and
  invite sealing is independent of the comms key (ephemeral per-invite keypair).
- Ordering: the wallet must exist + be unlockable before comms derives. Onboarding calls
  `wallet_ensure_ready` before any Groups/People surface; comms starts lazily on first Groups
  interaction, so the wallet is present by then. If not, the honest `WalletNotReady` error surfaces.

## Out of scope (v1)

- The on-chain / `wallet_link` attestation anchor (separate deferred follow-on).
- Automatic re-invite of pre-migration members (owner re-invites once).
- UI polish for the `WalletNotReady` state (covered by the separate stability/pinwheel pass).
