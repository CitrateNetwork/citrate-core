//! citrate-core-kit — the shared safety-critical spine.
//!
//! Extracted verbatim from `citrate-core/src-tauri/src` (QRM-S1 / WP-S1.2) so
//! citrate-core and citrate-quorum depend on **one** implementation of the
//! code that must never fork:
//!
//! - [`ceremony`] — the SignatureCeremony, the single human-in-the-loop signing
//!   path. The gated signer is reachable ONLY from `SignatureCeremony::approve`.
//! - [`custody`] — the OS-keyring custody vault (`Zeroizing`, no secret bytes
//!   cross an invoke boundary).
//! - [`oidc`] — the loopback-PKCE OIDC relying party.
//! - [`supervisor`] — the sidecar process supervisor (bounded-backoff restart,
//!   loopback bind, 0600 token files).
//! - [`wallet`] — BIP39/BIP44 keystore + the `pub(crate)` gated signer.
//! - [`rpc`] — the JSON-RPC client over an injectable transport.
//! - [`txdecode`] — human-readable decoding of calldata for the ceremony.
//! - [`config`] — persisted app config + keyring status.
//!
//! ## The gated-signer property, now stronger
//! `wallet::sign_message` / `sign_transaction` are `pub(crate)` — crate-private
//! to *this* crate. After the extraction they are unreachable from citrate-core
//! or citrate-quorum except through `ceremony::approve`. A source-scan test in
//! each consuming crate asserts no sibling signing site exists.
// Rule 8 (zero `.unwrap()` in shipped code): CI's clippy step (`-D warnings`) fails on any
// `.unwrap()` outside test code. Tests are compiled with `cfg(test)`, where this does not apply.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
// K6: no `unsafe` in this crate except the reviewed blocks that carry their own
// `#[allow(unsafe_code)]` and a SAFETY note (today: the two libc calls in `supervisor`, plus one
// in its tests). Privacy-based capabilities such as `wallet::WalletKeyAccess` rely on it: only
// `unsafe` can forge a value with a private field. `wallet_tests` pins every `unsafe` and every
// lint override per file (`k6_kit_unsafe_is_only_the_reviewed_blocks`). Not `forbid`: the
// reviewed libc calls need their `allow`.
#![deny(unsafe_code)]

pub mod abi_book;
pub mod blocking;
pub mod ceremony;
pub mod config;
pub mod custody;
pub mod eip712;
pub mod fsutil;
pub mod join;
pub mod oidc;
pub mod rpc;
pub mod secret;
pub mod siwe;
pub mod submitted_log;
pub mod supervisor;
pub mod txdecode;
pub mod wallet;
pub mod wallet_link;
pub mod web_budget;

#[cfg(test)]
mod upgrade_v051_tests;
