//! The GitHub part of L6-WP13: a 0.5.x GitHub connection (vault slot `connection-github`, a REST
//! token from the native connector) moves to the GitHub MCP server.
//!
//! The member signs in again (the MCP connection is its own grant, bound to the server's resource);
//! the old slot is kept until that sign-in succeeds and cleared only then, so a failed or
//! abandoned sign-in never loses a working token. Hugging Face, Google Sheets and Calendar are
//! not migrated: they stay native (Hugging Face is model-download auth for L5).

use super::flow::Connected;
use super::{store, McpAuthError};
use crate::connections::{slot, Service};
use crate::custody::CustodyVault;

/// The connector id of the GitHub MCP server.
pub(crate) const GITHUB_CONNECTOR: &str = "github";

/// GitHub's remote MCP server, read-only by default (matrix: pin `/readonly` as the default; the
/// write URL stays behind review, L6-WP07). Its PRM resource is `https://api.githubcopilot.com/mcp`,
/// which covers this path.
pub(crate) const GITHUB_MCP_READONLY: &str = "https://api.githubcopilot.com/mcp/readonly";

/// Whether the member should be asked to sign in again: a 0.5.x GitHub token is sealed and no
/// GitHub MCP connection exists yet. `false` when the vault is locked (never forces an unlock).
pub(crate) fn github_needs_migration(vault: &CustodyVault) -> bool {
    if !vault.is_unlocked() {
        return false;
    }
    let old = vault.custody_get(&slot(Service::GitHub)).is_ok();
    let new = store::read(vault, GITHUB_CONNECTOR).is_ok();
    old && !new
}

/// After a GitHub MCP connect attempt: clear the 0.5.x slot only if the connect succeeded and
/// the new record is readable. Returns whether the old slot was cleared.
pub(crate) fn finish_github_migration(
    vault: &CustodyVault,
    outcome: &Result<Connected, McpAuthError>,
) -> Result<bool, McpAuthError> {
    match outcome {
        Ok(c) if c.connector_id == GITHUB_CONNECTOR => {}
        _ => return Ok(false),
    }
    store::read(vault, GITHUB_CONNECTOR)?;
    vault
        .clear_slot(&slot(Service::GitHub))
        .map_err(|_| McpAuthError::Vault)?;
    Ok(true)
}
