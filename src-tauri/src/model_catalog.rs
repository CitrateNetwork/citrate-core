//! CX-S1 (lane s1) — model catalog resolver & switcher host commands (C-16).
//!
//! S1.4 wires the **resolver**: search Hugging Face Hub / GitHub Releases and turn the results
//! into verifiable [`ModelDescriptor`]s (pinned `size_bytes` + `sha256`) the download path can
//! trust. The resolver functions are PURE over an injected [`HttpClient`] so fixture tests drive
//! the parsers offline; the `#[tauri::command]` surface builds the production `ureq` client.
//!
//! Verifiability (Rule 1): a descriptor is only emitted when the source yields a real sha256 —
//! HF via the LFS `oid`, GitHub via a `<asset>.sha256` sidecar in the same release. Files without
//! one are SKIPPED, never surfaced as an unverifiable "download". `download`/`select`/`local`
//! stay honest `not wired` stubs until S1.5/S1.6 (download+runtime `-m` switch, local registry).
//!
//! Command NAMES are frozen in `lib.rs`; bodies + signatures are filled in HERE (never `lib.rs`).

use crate::model::{ModelDescriptor, ModelKind, ModelSource};
use crate::oidc::HttpClient;
use serde::Deserialize;

/// Hugging Face Hub API root.
const HF_API: &str = "https://huggingface.co/api";
/// GitHub REST API root.
const GH_API: &str = "https://api.github.com";
/// How many search hits (repos / releases) to resolve. Bounds the fan-out per query.
const SEARCH_LIMIT: usize = 15;

// ---------------------------------------------------------------------------
// Resolver — pure over an injected HttpClient (fixture-tested; no network in tests).
// ---------------------------------------------------------------------------

/// Search a connected source for downloadable GGUF models matching `query`. `token` is an
/// optional read token (HF read scope / GitHub token) for gated/private repos; public repos
/// resolve without one. `Bundled` is not a searchable source (the app's built-in default).
pub fn search(
    http: &dyn HttpClient,
    source: ModelSource,
    query: &str,
    token: Option<&str>,
) -> Result<Vec<ModelDescriptor>, String> {
    match source {
        ModelSource::Hf => hf_search(http, query, token),
        // A GitHub query is a `owner/name` repo slug (its releases carry the GGUF assets).
        ModelSource::Github => github_releases(http, query.trim(), token),
        ModelSource::Bundled => Ok(Vec::new()),
    }
}

// ---- Hugging Face Hub ----

#[derive(Deserialize)]
struct HfModel {
    /// The repo slug, e.g. "ggml-org/gemma-4-E4B-it-GGUF".
    id: String,
}

#[derive(Deserialize)]
struct HfTreeEntry {
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<HfLfs>,
}

#[derive(Deserialize)]
struct HfLfs {
    /// The git-lfs object id — the file's sha256 (sometimes "sha256:"-prefixed).
    oid: String,
    /// The real (large) file size; the entry's top-level `size` is only the pointer size.
    size: u64,
}

/// Search HF Hub for GGUF repos matching `query`, then resolve each repo's GGUF files into
/// verifiable descriptors. A repo whose file tree fails to resolve is skipped (best-effort),
/// not fatal — one bad repo never sinks the whole search.
pub fn hf_search(
    http: &dyn HttpClient,
    query: &str,
    token: Option<&str>,
) -> Result<Vec<ModelDescriptor>, String> {
    let url = format!(
        "{HF_API}/models?search={}&filter=gguf&limit={SEARCH_LIMIT}",
        pct(query)
    );
    let body = http.get(&url, token).map_err(|e| e.to_string())?;
    let repos: Vec<HfModel> =
        serde_json::from_str(&body).map_err(|e| format!("HF search parse error: {e}"))?;
    let mut out = Vec::new();
    for r in repos {
        if let Ok(mut files) = hf_files(http, &r.id, "main", token) {
            out.append(&mut files);
        }
    }
    Ok(out)
}

/// Resolve a single HF repo's GGUF files at `revision` into descriptors. A `.gguf` entry with no
/// LFS oid carries no sha256 and is SKIPPED (unverifiable — Rule 1).
pub fn hf_files(
    http: &dyn HttpClient,
    repo: &str,
    revision: &str,
    token: Option<&str>,
) -> Result<Vec<ModelDescriptor>, String> {
    let url = format!("{HF_API}/models/{repo}/tree/{revision}");
    let body = http.get(&url, token).map_err(|e| e.to_string())?;
    let entries: Vec<HfTreeEntry> =
        serde_json::from_str(&body).map_err(|e| format!("HF tree parse error: {e}"))?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            if !e.path.ends_with(".gguf") {
                return None;
            }
            let lfs = e.lfs?; // no LFS oid → no verifiable sha256 → skip.
            let sha = normalize_sha256(&lfs.oid)?;
            // Prefer the LFS size (the real bytes); fall back to the entry size if 0.
            let size = if lfs.size > 0 { lfs.size } else { e.size };
            Some(ModelDescriptor {
                id: format!("hf:{repo}/{}", e.path),
                source: ModelSource::Hf,
                repo: repo.to_string(),
                file: e.path,
                revision: Some(revision.to_string()),
                size_bytes: size,
                sha256: sha,
                kind: ModelKind::Gguf,
            })
        })
        .collect())
}

// ---- GitHub Releases ----

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    #[serde(default)]
    size: u64,
}

/// Resolve GGUF assets from a GitHub repo's releases into descriptors. GitHub publishes no
/// per-asset sha256, so an asset is catalog-addable ONLY when the same release ships a
/// `<asset>.sha256` sidecar (whose text is the hex digest). Assets without one are SKIPPED —
/// no unverifiable download (Rule 1). `repo` is `owner/name`.
pub fn github_releases(
    http: &dyn HttpClient,
    repo: &str,
    token: Option<&str>,
) -> Result<Vec<ModelDescriptor>, String> {
    if repo.is_empty() || !repo.contains('/') {
        return Err("GitHub source expects an `owner/name` repo".to_string());
    }
    let url = format!("{GH_API}/repos/{repo}/releases?per_page={SEARCH_LIMIT}");
    let body = http.get(&url, token).map_err(|e| e.to_string())?;
    let releases: Vec<GhRelease> =
        serde_json::from_str(&body).map_err(|e| format!("GitHub releases parse error: {e}"))?;
    let mut out = Vec::new();
    for rel in releases {
        let has_sidecar = |name: &str| {
            let sidecar = format!("{name}.sha256");
            rel.assets.iter().any(|a| a.name == sidecar)
        };
        for a in &rel.assets {
            if !a.name.ends_with(".gguf") || !has_sidecar(&a.name) {
                continue;
            }
            let sidecar_url = format!(
                "https://github.com/{repo}/releases/download/{}/{}.sha256",
                rel.tag_name, a.name
            );
            let Ok(text) = http.get(&sidecar_url, token) else {
                continue; // sidecar unreadable → cannot verify → skip.
            };
            let Some(sha) = text.split_whitespace().next().and_then(normalize_sha256) else {
                continue; // sidecar not a hex digest → skip.
            };
            out.push(ModelDescriptor {
                id: format!("github:{repo}/{}@{}", a.name, rel.tag_name),
                source: ModelSource::Github,
                repo: repo.to_string(),
                file: a.name.clone(),
                revision: Some(rel.tag_name.clone()),
                size_bytes: a.size,
                sha256: sha,
                kind: ModelKind::Gguf,
            });
        }
    }
    Ok(out)
}

// ---- helpers ----

/// Percent-encode a query for a URL query value (RFC 3986 unreserved set stays literal).
fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Accept a `sha256:`-prefixed or bare hex oid and return a validated lowercase 64-hex digest,
/// or `None` if it isn't one (so a malformed oid can never become a fake pin).
fn normalize_sha256(raw: &str) -> Option<String> {
    let hex = raw.trim().strip_prefix("sha256:").unwrap_or(raw.trim());
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(hex.to_ascii_lowercase())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Tauri command surface. Frozen NAMES (lib.rs); bodies here.
// ---------------------------------------------------------------------------

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("model_catalog::{cmd} is not wired yet (CX-S1 scaffold)"))
}

/// Locally-present, verified models. S1.6 (local registry) fills this in; until then it is an
/// honest `not wired` stub rather than a half-true guess at what's on disk.
#[tauri::command]
pub fn model_catalog_local() -> Result<Vec<ModelDescriptor>, String> {
    not_wired("local").map(|()| Vec::new())
}

/// Search a connected source (`"hf"` | `"github"`) for downloadable GGUF models. Public repos
/// resolve without a token; gated-repo token threading rides with the S1.5 download path (which
/// already opens the connection vault). Blocking `ureq` — Tauri runs commands off the UI thread.
#[tauri::command]
pub fn model_catalog_search(
    source: String,
    query: String,
) -> Result<Vec<ModelDescriptor>, String> {
    let src = match source.as_str() {
        "hf" => ModelSource::Hf,
        "github" => ModelSource::Github,
        other => return Err(format!("unknown model source '{other}' (expected hf|github)")),
    };
    search(&crate::oidc::UreqClient, src, &query, None)
}

/// Download + verify a descriptor by id. Wired in S1.5 (build the transport from
/// `descriptor.download_url()` → `ModelManager::from_descriptor` → download + verify).
#[tauri::command]
pub fn model_catalog_download(_id: String) -> Result<(), String> {
    not_wired("download")
}

/// Switch the active local model (restart `llama-server -m`). Wired in S1.5.
#[tauri::command]
pub fn model_catalog_select(_id: String) -> Result<(), String> {
    not_wired("select")
}

#[cfg(test)]
mod tests {
    include!("model_catalog_tests.rs");
}
