//! L6-WP07: the Marketplace MCP catalog view (`mcp_catalog_list`).
//!
//! Data source (Rule 7): the verified brand servers compiled into the app ([`brand`]), the
//! catalog key slot (`citrate_components::catalog::PRODUCTION_CATALOG_PUBKEY`, shared with the
//! signed subregistry), and the member's saved MCP servers (the `mcp-servers.json` registry,
//! read only by [`saved_endpoint_urls`]) for the installed flag.
//!
//! The key slot is empty until the owner's key ceremony, so the view is the pinned list with
//! `signed: false` and the refusal "Signed catalog not available yet: signing key not issued".
//! The signed subregistry client that would turn `signed` on is L6-WP08c; until it exists a
//! filled key slot alone reports `not_fetched`, never a signed state.

pub(crate) mod brand;
#[cfg(test)]
mod brand_tests;
#[cfg(test)]
mod catalog_tests;

use std::collections::BTreeSet;
use std::path::Path;

use citrate_components::catalog::{catalog_trust_root, CatalogError};
use citrate_components::key::TrustRoot;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use brand::BRAND_SERVERS;

/// What the signed section says while the catalog key slot is empty.
pub(crate) const NO_KEY_MESSAGE: &str = "Signed catalog not available yet: signing key not issued";

/// The trust badge on an entry. Only the pinned brand list exists today; the curated,
/// registry-search and member-added badges arrive with WP08c, WP09 and WP11.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrustBadge {
    VerifiedBrand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpCatalogEntry {
    pub(crate) entry_id: String,
    pub(crate) name: String,
    pub(crate) summary: String,
    pub(crate) publisher: String,
    pub(crate) trust: TrustBadge,
    pub(crate) remote_url: Option<String>,
    /// `{ registryType, identifier, version }` records; always empty for the remote brand servers
    /// (the subregistry entries that carry packages arrive with L6-WP08c).
    pub(crate) packages: Vec<serde_json::Value>,
    /// `oauth` or `none`.
    pub(crate) auth: String,
    /// A saved user server points at this entry's URL. `None` when the saved servers could not
    /// be read ([`McpCatalogView::installed_unavailable`] says why).
    pub(crate) installed: Option<bool>,
    /// The date a person checked the entry.
    pub(crate) reviewed_at: Option<String>,
    /// The publisher's own page for the server.
    pub(crate) source_url: String,
    /// `cimd`, `cimd_then_dcr`, `dcr` or `none`.
    pub(crate) registration: String,
    pub(crate) write_tools_default: bool,
}

/// Why the signed section is not shown. `code` is `no_key`, `bad_key` or `not_fetched`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogRefusal {
    pub(crate) code: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpCatalogView {
    /// `pinned` (the brand list compiled in) or, from WP08c, `subregistry`.
    pub(crate) source: String,
    pub(crate) signed: bool,
    pub(crate) refused: Option<CatalogRefusal>,
    /// Why the installed flags are unknown (the saved servers could not be read); `None` when
    /// they were read.
    pub(crate) installed_unavailable: Option<String>,
    pub(crate) entries: Vec<McpCatalogEntry>,
}

/// The URL form used to match a saved server against a pinned entry. An `https` URL is parsed:
/// the host is lowercased, the default port (443) and any user info are dropped, one trailing `/`
/// of the path is dropped, and the path (its case kept) and query stay, so a different port, path
/// or query is a different endpoint. Anything else (not `https`, or unparsable) is kept as given,
/// trimmed, and so never equals a pinned entry.
pub(crate) fn normalize_url(raw: &str) -> String {
    let raw = raw.trim();
    let parsed = match url::Url::parse(raw) {
        Ok(u) if u.scheme() == "https" => u,
        _ => return raw.to_string(),
    };
    let Some(host) = parsed.host_str() else {
        return raw.to_string();
    };
    // `Url` already drops a port equal to the scheme's default.
    let port = parsed.port().map(|p| format!(":{p}")).unwrap_or_default();
    let path = parsed.path();
    let path = path.strip_suffix('/').unwrap_or(path);
    let query = parsed.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!("https://{host}{port}{path}{query}")
}

/// Only the field the installed flag needs; every other field of a saved entry (including the
/// plain-text `env` an older version stored) is skipped, not kept in memory.
#[derive(Deserialize)]
struct SavedUrls {
    #[serde(default)]
    servers: Vec<SavedUrl>,
}

#[derive(Deserialize)]
struct SavedUrl {
    #[serde(default)]
    url: Option<String>,
}

/// The normalized URLs of the member's saved servers, read from `<dir>/mcp-servers.json` without
/// changing anything: no migration, no keychain, no rewrite, no removal of the old allowlist
/// (those stay with `McpStore::load`). A missing file is an empty set; an unreadable or damaged
/// file is an error, never silently empty.
pub(crate) fn saved_endpoint_urls(dir: &Path) -> Result<BTreeSet<String>, String> {
    let text = match std::fs::read_to_string(dir.join(crate::mcp_servers::REGISTRY_FILE)) {
        Ok(t) => Zeroizing::new(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(e) => {
            return Err(format!(
                "Installed status unknown: the MCP server list could not be read ({})",
                e.kind()
            ))
        }
    };
    let saved: SavedUrls = serde_json::from_str(&text).map_err(|_| {
        "Installed status unknown: the MCP server list could not be read (file is damaged)"
            .to_string()
    })?;
    Ok(saved
        .servers
        .iter()
        .filter_map(|s| s.url.as_deref())
        .map(normalize_url)
        .collect())
}

fn refusal(key: &Result<TrustRoot, CatalogError>) -> CatalogRefusal {
    let (code, message) = match key {
        Err(CatalogError::KeyNotIssued) => ("no_key", NO_KEY_MESSAGE.to_string()),
        Err(e) => ("bad_key", format!("Signed catalog not available: {e}")),
        Ok(_) => (
            "not_fetched",
            "Signed catalog not fetched: this version lists the verified brand servers only"
                .to_string(),
        ),
    };
    CatalogRefusal {
        code: code.to_string(),
        message,
    }
}

/// The catalog view over the given key slot result and the saved servers' normalized URLs. A
/// failed saved-server lookup leaves the pinned entries and the key status as they are and marks
/// every installed flag unknown.
pub(crate) fn catalog_view(
    key: &Result<TrustRoot, CatalogError>,
    installed_urls: &Result<BTreeSet<String>, String>,
) -> McpCatalogView {
    let entries = BRAND_SERVERS
        .iter()
        .map(|s| McpCatalogEntry {
            entry_id: format!("brand:{}", s.id),
            name: s.name.to_string(),
            summary: s.summary.to_string(),
            publisher: s.publisher.to_string(),
            trust: TrustBadge::VerifiedBrand,
            remote_url: Some(s.url.to_string()),
            packages: Vec::new(),
            auth: s.auth.as_str().to_string(),
            installed: installed_urls
                .as_ref()
                .ok()
                .map(|urls| urls.contains(&normalize_url(s.url))),
            reviewed_at: Some(s.checked.to_string()),
            source_url: s.source.to_string(),
            registration: s.client.as_str().to_string(),
            write_tools_default: s.write_tools_default,
        })
        .collect();
    McpCatalogView {
        source: "pinned".to_string(),
        signed: false,
        refused: Some(refusal(key)),
        installed_unavailable: installed_urls.as_ref().err().cloned(),
        entries,
    }
}

/// **mcp_catalog_list**: the Marketplace MCP catalog. Reads no network and writes nothing; with
/// the key slot empty it is the pinned brand list and the "signing key not issued" refusal. It
/// always returns the pinned list: a saved-server lookup that fails only makes `installed` unknown.
#[tauri::command]
pub(crate) async fn mcp_catalog_list(app: tauri::AppHandle) -> Result<McpCatalogView, String> {
    let installed = match crate::mcp_servers::hermes_dir(&app) {
        Ok(dir) => match crate::blocking::off_main(move || Ok(saved_endpoint_urls(&dir))).await {
            Ok(lookup) => lookup,
            Err(e) => Err(format!("Installed status unknown: {e}")),
        },
        Err(e) => Err(format!("Installed status unknown: {e}")),
    };
    Ok(catalog_view(&catalog_trust_root(), &installed))
}
