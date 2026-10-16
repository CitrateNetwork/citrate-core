//! citrate-components: the signed component updater (HUP-S5.5) and the toolchain bundle
//! definitions (HUP-S6.1).
//!
//! Large or fast-moving pieces of the app (the managed browser, the dApp toolchain, private
//! search, skills, the docs graph) are installed on first run and kept current as signed
//! components instead of growing the installer (planset red-team corrections 15 and RT-8/RT-9).
//!
//! - [`key`]: the pinned trust root. The production slot is EMPTY until the @rule8 key
//!   ceremony, so nothing installs until then.
//! - [`manifest`]: the signed manifest (minisign, prehashed Ed25519), with anti-rollback
//!   sequence numbers, expiry and strict field checks.
//! - [`install`]: download to staging, verify size + SHA-256 + artifact signature, unpack,
//!   health-check, then swap by committing one state file; rollback to the previous version.
//! - [`extract`]: unpacking with path, link and size checks.
//! - [`policy`]: the client side of the CVE SLA (stale and expired manifests).
//! - [`bundle`]: the toolchain bundle definitions and the manifest the release step signs.
//!
//! Rule 3: nothing here holds a private key or signs. Signing happens offline at the release
//! ceremony with the minisign tool.
pub mod bundle;
pub mod error;
pub mod extract;
pub mod fetch;
pub mod install;
pub mod key;
pub mod manifest;
pub mod platform;
pub mod policy;
