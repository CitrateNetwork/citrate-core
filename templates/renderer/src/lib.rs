//! citrate-templates: the HUP-S6.2 template renderer and the HUP-S6.9 Medusa budgets.
//!
//! Planset `2026-09-30-hermes-upskill`, 02_ARCHITECTURE section 7 (the dApp forge):
//! the hello-mint flow starts from a template filled with the interview answers
//! (name, symbol, supply, price, owner). This crate is that step and nothing else:
//!
//! - [`params`] validates each value against a small inert alphabet, so a name can
//!   never inject code into a Solidity, TypeScript, JSON or HTML file;
//! - [`render::TemplateSet`] loads `citrate-core/templates/`, substitutes
//!   `{{ct:key}}` placeholders, and writes into an empty directory with a
//!   `citrate-template.lock.json` provenance record;
//! - [`budget`] loads the per-tier Medusa call/coverage budgets.
//!
//! It does not fetch dependencies, compile, fuzz, deploy or sign. Dependency pins
//! live in `templates/deps.lock.json`; the toolchain bundle (HUP-S6.1) supplies
//! them, and `templates/scripts/verify-templates.sh` fetches them for the gate.
//!
//! Why Rust and not TypeScript: the consumers are the Tauri backend and the
//! toolchain sidecar (both Rust), the renderer writes files on the user's disk and
//! so sits on a trust boundary where the workspace's no-`unwrap` rule and typed
//! errors apply, and it needs no Node runtime to run.

pub mod budget;
pub mod params;
pub mod render;

pub use budget::{MedusaBudget, MedusaBudgets, Tier};
pub use render::{RenderError, RenderReport, TemplateKind, TemplateManifest, TemplateSet};
