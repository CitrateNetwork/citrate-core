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

pub mod ceremony;
pub mod config;
pub mod custody;
pub mod oidc;
pub mod rpc;
pub mod supervisor;
pub mod txdecode;
pub mod wallet;
