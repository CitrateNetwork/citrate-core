//! L6-WP07: the Marketplace MCP catalog view (`mcp_catalog_list`).
//!
//! Data source (Rule 7): the verified brand servers compiled into the app ([`brand`]), the
//! catalog key slot (`citrate_components::catalog::PRODUCTION_CATALOG_PUBKEY`, shared with the
//! signed subregistry), and the member's saved MCP servers (`mcp_servers_list`) for the installed
//! flag.
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

use citrate_components::catalog::{catalog_trust_root, CatalogError};
use citrate_components::key::TrustRoot;
use serde::Serialize;

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

/// A package an entry installs from (empty for every remote brand server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackageView {
    pub(crate) registry_type: String,
    pub(crate) identifier: String,
    pub(crate) version: String,
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
    pub(crate) packages: Vec<PackageView>,
    /// `oauth` or `none`.
    pub(crate) auth: String,
    /// A saved user server points at this entry's URL.
    pub(crate) installed: bool,
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
    pub(crate) entries: Vec<McpCatalogEntry>,
}

/// The URL form used to match a saved server: host lowercased, one trailing `/` dropped.
pub(crate) fn normalize_url(url: &str) -> String {
    let url = url.trim();
    let url = url.strip_suffix('/').unwrap_or(url);
    match url.strip_prefix("https://") {
        Some(rest) => {
            let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
            format!("https://{}{path}", host.to_ascii_lowercase())
        }
        None => url.to_string(),
    }
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

/// The catalog view over the given key slot result and the saved servers' normalized URLs.
pub(crate) fn catalog_view(
    key: &Result<TrustRoot, CatalogError>,
    installed_urls: &BTreeSet<String>,
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
            installed: installed_urls.contains(&normalize_url(s.url)),
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
        entries,
    }
}

/// **mcp_catalog_list**: the Marketplace MCP catalog. Reads no network; with the key slot empty
/// it is the pinned brand list and the "signing key not issued" refusal.
#[tauri::command]
pub(crate) async fn mcp_catalog_list(app: tauri::AppHandle) -> Result<McpCatalogView, String> {
    let saved = crate::mcp_servers::mcp_servers_list(app).await?;
    let installed: BTreeSet<String> = saved
        .iter()
        .filter_map(|s| s.url.as_deref())
        .map(normalize_url)
        .collect();
    Ok(catalog_view(&catalog_trust_root(), &installed))
}
