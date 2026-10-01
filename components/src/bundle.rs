//! HUP-S6.1 toolchain bundle definitions (`components/toolchain-bundle.json`).
//!
//! The bundle is the unsigned, reviewed source list: upstream URL, format, and for each
//! platform either a measured SHA-256 and size or an honest status (`to_be_measured`,
//! `to_be_built`, `upstream_unavailable`). [`check_bundle`] enforces that honesty and that the
//! library pins come from `templates/deps.lock.json` (one source of truth). The release step
//! calls [`manifest_from_bundle`] to produce the manifest the component key then signs.
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ComponentError;
use crate::extract::safe_relative_path;
use crate::manifest::{
    is_https_url, validate, ArchiveFormat, Artifact, Component, ComponentKind, Manifest,
    MANIFEST_SCHEMA, MAX_ARTIFACT_BYTES,
};
use crate::platform::Platform;

pub const BUNDLE_SCHEMA: u32 = 1;
/// The lock the library archives must agree with, relative to the repo root.
pub const DEPS_LOCK_PATH: &str = "templates/deps.lock.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    /// Downloaded and hashed; `sha256` and `size` are the measured values.
    Measured,
    /// URL known, not downloaded yet: no hash is recorded.
    ToBeMeasured,
    /// Citrate packs this artifact (for example a wheelhouse); not built yet.
    ToBeBuilt,
    /// Upstream publishes nothing for this platform.
    UpstreamUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleArtifact {
    pub url: Option<String>,
    pub format: ArchiveFormat,
    pub status: ArtifactStatus,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_checksums: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoints: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub name: String,
    pub version: String,
    pub kind: ComponentKind,
    pub license: String,
    pub homepage: String,
    pub entrypoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<Upstream>,
    pub artifacts: BTreeMap<String, BundleArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryArchive {
    pub url: String,
    pub format: ArchiveFormat,
    pub status: ArtifactStatus,
    pub sha256: String,
    pub size: u64,
    pub top_dir: String,
    pub license_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Libraries {
    pub lock: String,
    pub note: String,
    pub archives: BTreeMap<String, LibraryArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub schema: u32,
    pub wp: String,
    pub note: String,
    pub measured_on: String,
    pub platforms: Vec<String>,
    pub tools: Vec<Tool>,
    pub libraries: Libraries,
}

impl Bundle {
    pub fn parse(json: &str) -> Result<Self, ComponentError> {
        serde_json::from_str(json)
            .map_err(|e| ComponentError::ManifestMalformed(format!("bundle: {e}")))
    }
}

/// The parts of `templates/deps.lock.json` the bundle depends on (other fields are ignored,
/// so the lock can grow without breaking the bundle).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DepsLock {
    pub solc: String,
    pub deps: BTreeMap<String, Dep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Dep {
    pub url: String,
    pub tag: String,
    pub commit: String,
    pub license: String,
}

impl DepsLock {
    pub fn parse(json: &str) -> Result<Self, ComponentError> {
        serde_json::from_str(json)
            .map_err(|e| ComponentError::ManifestMalformed(format!("deps lock: {e}")))
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

const PLATFORMS: [Platform; 5] = [
    Platform::MacosArm64,
    Platform::MacosX64,
    Platform::LinuxX64,
    Platform::LinuxArm64,
    Platform::WindowsX64,
];

/// Every problem found, as text. Empty means the bundle is consistent.
pub fn check_bundle(b: &Bundle, d: &DepsLock, repo_root: &Path) -> Vec<String> {
    let mut p = Vec::new();
    if b.schema != BUNDLE_SCHEMA {
        p.push(format!("schema {}", b.schema));
    }
    let want: Vec<&str> = PLATFORMS.iter().map(|x| x.as_str()).collect();
    if b.platforms.iter().map(String::as_str).collect::<Vec<_>>() != want {
        p.push(format!("platforms must be exactly {want:?}"));
    }
    let mut names = std::collections::BTreeSet::new();
    for t in &b.tools {
        let n = &t.name;
        if !names.insert(n.as_str()) {
            p.push(format!("{n}: listed twice"));
        }
        if t.version.is_empty() || t.version.starts_with('.') || t.version.contains('/') {
            p.push(format!("{n}: version"));
        }
        if t.license.trim().is_empty() {
            p.push(format!("{n}: no licence"));
        }
        if !is_https_url(&t.homepage) {
            p.push(format!("{n}: homepage must be https"));
        }
        for e in &t.entrypoints {
            if safe_relative_path(e).is_err() {
                p.push(format!("{n}: entrypoint {e}"));
            }
        }
        if let Some(u) = &t.upstream {
            if !is_https_url(&u.url) || !is_hex64(&u.sha256) || u.size == 0 {
                p.push(format!("{n}: upstream entry"));
            }
        }
        for plat in &want {
            if !t.artifacts.contains_key(*plat) {
                p.push(format!("{n}: no entry for {plat}"));
            }
        }
        for (plat, a) in &t.artifacts {
            let w = format!("{n} {plat}");
            if !want.contains(&plat.as_str()) {
                p.push(format!("{w}: unknown platform"));
            }
            check_artifact(&w, a, repo_root, &mut p);
            let eps = a.entrypoints.as_ref().unwrap_or(&t.entrypoints);
            if a.status == ArtifactStatus::Measured && eps.is_empty() {
                p.push(format!("{w}: a measured artifact needs entrypoints"));
            }
            for e in a.entrypoints.iter().flatten() {
                if safe_relative_path(e).is_err() {
                    p.push(format!("{w}: entrypoint {e}"));
                }
            }
        }
    }
    match b.tools.iter().find(|t| t.name == "solc") {
        Some(s) if s.version == d.solc => {}
        Some(s) => p.push(format!(
            "solc {} differs from {DEPS_LOCK_PATH} ({})",
            s.version, d.solc
        )),
        None => p.push("solc is missing".into()),
    }
    if b.libraries.lock != DEPS_LOCK_PATH {
        p.push(format!("libraries.lock must be {DEPS_LOCK_PATH}"));
    }
    for name in d.deps.keys() {
        if !b.libraries.archives.contains_key(name) {
            p.push(format!("library {name}: no archive"));
        }
    }
    for (name, a) in &b.libraries.archives {
        let w = format!("library {name}");
        let Some(dep) = d.deps.get(name) else {
            p.push(format!("{w}: not in {DEPS_LOCK_PATH}"));
            continue;
        };
        if !is_https_url(&a.url) || !a.url.contains(&dep.commit) {
            p.push(format!(
                "{w}: url must be https and name commit {}",
                dep.commit
            ));
        }
        if a.status != ArtifactStatus::Measured
            || !is_hex64(&a.sha256)
            || a.size == 0
            || a.size > MAX_ARTIFACT_BYTES
        {
            p.push(format!("{w}: must be measured with sha256 and size"));
        }
        if a.format != ArchiveFormat::TarGz {
            p.push(format!("{w}: format must be tar.gz"));
        }
        if safe_relative_path(&a.top_dir).is_err() {
            p.push(format!("{w}: top_dir"));
        }
        if a.license_files.is_empty() {
            p.push(format!("{w}: no licence file"));
        }
        for f in &a.license_files {
            let ok = safe_relative_path(f).is_ok()
                && std::fs::metadata(repo_root.join(f))
                    .map(|m| m.is_file() && m.len() > 0)
                    .unwrap_or(false);
            if !ok {
                p.push(format!("{w}: licence file {f} is missing"));
            }
        }
    }
    p
}

fn check_artifact(w: &str, a: &BundleArtifact, repo_root: &Path, p: &mut Vec<String>) {
    let https = a.url.as_deref().map(is_https_url);
    match a.status {
        ArtifactStatus::Measured => {
            if https != Some(true) {
                p.push(format!("{w}: measured needs an https url"));
            }
            if !a.sha256.as_deref().is_some_and(is_hex64) {
                p.push(format!("{w}: measured needs a 64-hex sha256"));
            }
            if !a.size.is_some_and(|s| s > 0 && s <= MAX_ARTIFACT_BYTES) {
                p.push(format!("{w}: measured needs a size"));
            }
        }
        ArtifactStatus::ToBeMeasured => {
            if https != Some(true) {
                p.push(format!("{w}: to_be_measured needs an https url"));
            }
            if a.sha256.is_some() || a.size.is_some() {
                p.push(format!(
                    "{w}: an unmeasured entry must not carry a hash or size"
                ));
            }
        }
        ArtifactStatus::ToBeBuilt => {
            if a.sha256.is_some() || a.size.is_some() {
                p.push(format!(
                    "{w}: an unbuilt entry must not carry a hash or size"
                ));
            }
            if https == Some(false) {
                p.push(format!("{w}: url must be https"));
            }
        }
        ArtifactStatus::UpstreamUnavailable => {
            if a.url.is_some() || a.sha256.is_some() || a.size.is_some() {
                p.push(format!(
                    "{w}: an unavailable entry has no url, hash or size"
                ));
            }
            if a.note.as_deref().is_none_or(str::is_empty) {
                p.push(format!("{w}: say why it is unavailable"));
            }
        }
    }
    if let Some(f) = &a.build_from {
        let ok = safe_relative_path(f).is_ok() && repo_root.join(f).is_file();
        if !ok {
            p.push(format!("{w}: build_from {f} is missing"));
        }
    }
}

/// The unsigned manifest for every measured artifact. `signatures` maps an artifact's SHA-256
/// (hex) to its minisign signature text (made at the release ceremony); a measured artifact without one is
/// an error, so a manifest never ships an unsigned artifact.
pub fn manifest_from_bundle(
    b: &Bundle,
    d: &DepsLock,
    signatures: &BTreeMap<String, String>,
    sequence: u64,
    issued_at: u64,
    expires_at: u64,
) -> Result<Manifest, ComponentError> {
    let sig = |sha256: &str, url: &str| {
        signatures
            .get(sha256)
            .cloned()
            .ok_or_else(|| ComponentError::ManifestMalformed(format!("no signature for {url}")))
    };
    let mut components = Vec::new();
    for t in &b.tools {
        let mut artifacts = BTreeMap::new();
        for (plat, a) in &t.artifacts {
            if a.status != ArtifactStatus::Measured {
                continue;
            }
            let (Some(url), Some(sha256), Some(size)) = (&a.url, &a.sha256, a.size) else {
                return Err(ComponentError::ManifestMalformed(format!(
                    "{} {plat}: incomplete",
                    t.name
                )));
            };
            artifacts.insert(
                plat.clone(),
                Artifact {
                    url: url.clone(),
                    sha256: sha256.clone(),
                    size,
                    format: a.format,
                    signature: sig(sha256, url)?,
                    entrypoints: a.entrypoints.clone(),
                },
            );
        }
        if artifacts.is_empty() {
            continue;
        }
        components.push(Component {
            name: t.name.clone(),
            version: t.version.clone(),
            kind: t.kind,
            license: t.license.clone(),
            entrypoints: t.entrypoints.clone(),
            artifacts,
        });
    }
    for (name, a) in &b.libraries.archives {
        let dep = d.deps.get(name).ok_or_else(|| {
            ComponentError::ManifestMalformed(format!("{name}: not in the deps lock"))
        })?;
        let mut artifacts = BTreeMap::new();
        artifacts.insert(
            Platform::Any.as_str().to_string(),
            Artifact {
                url: a.url.clone(),
                sha256: a.sha256.clone(),
                size: a.size,
                format: a.format,
                signature: sig(&a.sha256, &a.url)?,
                entrypoints: None,
            },
        );
        components.push(Component {
            name: name.clone(),
            version: dep.tag.trim_start_matches('v').to_string(),
            kind: ComponentKind::Library,
            license: dep.license.clone(),
            entrypoints: vec![a.top_dir.clone()],
            artifacts,
        });
    }
    let m = Manifest {
        schema: MANIFEST_SCHEMA,
        channel: "stable".into(),
        sequence,
        issued_at,
        expires_at,
        components,
    };
    validate(&m)?;
    Ok(m)
}
