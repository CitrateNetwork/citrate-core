//! Verify-then-swap installs with rollback.
//!
//! Layout under the store root (owner-only):
//!
//! ```text
//! state.json                      the commit point: current + previous version per component
//! .staging/<name>-<nonce>/        download.bin, then tree/ (removed after every attempt)
//! <name>/<version>-<sha12>/       an unpacked, verified version
//! ```
//!
//! An install downloads into staging, checks size, SHA-256 and the artifact signature in one
//! pass over the file, unpacks, runs the health check on the unpacked tree, renames the tree
//! into place and only then rewrites `state.json` (write to a temporary file, fsync, rename).
//! Until that rename the old version is current and nothing it uses has changed, so any failure
//! is a rollback by construction. The previous version stays on disk for [`Store::rollback`];
//! older ones are removed. [`Store::recover`] clears what a crash left behind.
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{io, ComponentError};
use crate::extract::extract;
use crate::fetch::Fetcher;
use crate::key::TrustRoot;
use crate::manifest::{
    check_sequence, verify_stream, Component, SeenManifest, VerifiedManifest, ARTIFACT_DOMAIN,
};
use crate::platform::Platform;

const STATE_FILE: &str = "state.json";
const STAGING: &str = ".staging";
const STATE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledVersion {
    pub version: String,
    pub sha256: String,
    /// Directory name under `<root>/<name>/`.
    pub dir: String,
    pub platform: Platform,
    pub installed_at: u64,
    pub manifest_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledComponent {
    pub current: InstalledVersion,
    pub previous: Option<InstalledVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreState {
    pub schema: u32,
    pub last_manifest: Option<SeenManifest>,
    pub components: BTreeMap<String, InstalledComponent>,
}

impl Default for StoreState {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA,
            last_manifest: None,
            components: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed {
        version: String,
        previous: Option<String>,
    },
    AlreadyCurrent {
        version: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub removed_staging: u64,
    pub removed_versions: u64,
}

/// Runs on the unpacked tree before the swap. An error aborts the install.
pub trait HealthCheck {
    fn check(&self, tree: &Path, component: &Component) -> Result<(), String>;
}

/// Every entrypoint the manifest names exists inside the tree (following links that stay
/// inside it).
pub struct EntrypointsPresent;

impl HealthCheck for EntrypointsPresent {
    fn check(&self, tree: &Path, component: &Component) -> Result<(), String> {
        let root = fs::canonicalize(tree).map_err(|e| e.to_string())?;
        let eps: Vec<&String> = component
            .artifacts
            .values()
            .flat_map(|a| component.entrypoints_for(a).iter())
            .collect();
        // Only the artifact actually installed matters; the caller passes a component trimmed
        // to that artifact (see `Store::install`).
        for ep in eps {
            let rel = crate::extract::safe_relative_path(ep).map_err(|e| e.to_string())?;
            let p = fs::canonicalize(tree.join(&rel)).map_err(|_| format!("{ep} is missing"))?;
            if !p.starts_with(&root) {
                return Err(format!("{ep} resolves outside the component"));
            }
        }
        Ok(())
    }
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens (creating if needed, owner-only) the store at `root`.
    pub fn open(root: &Path) -> Result<Self, ComponentError> {
        fs::create_dir_all(root).map_err(io)?;
        restrict(root)?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The recorded state; a missing file is the empty state, a corrupt one is an error.
    pub fn state(&self) -> Result<StoreState, ComponentError> {
        let p = self.root.join(STATE_FILE);
        let bytes = match fs::read(&p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(StoreState::default()),
            Err(e) => return Err(io(e)),
        };
        let st: StoreState =
            serde_json::from_slice(&bytes).map_err(|e| ComponentError::State(e.to_string()))?;
        if st.schema != STATE_SCHEMA {
            return Err(ComponentError::State(format!("schema {}", st.schema)));
        }
        Ok(st)
    }

    fn write_state(&self, st: &StoreState) -> Result<(), ComponentError> {
        let bytes =
            serde_json::to_vec_pretty(st).map_err(|e| ComponentError::State(e.to_string()))?;
        let tmp = self.root.join(format!("{STATE_FILE}.tmp"));
        {
            let mut f = fs::File::create(&tmp).map_err(io)?;
            f.write_all(&bytes).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        fs::rename(&tmp, self.root.join(STATE_FILE)).map_err(io)?;
        sync_dir(&self.root);
        Ok(())
    }

    /// Records `vm` as the newest manifest seen (refusing an older one).
    pub fn record_manifest(&self, vm: &VerifiedManifest) -> Result<(), ComponentError> {
        let mut st = self.state()?;
        check_sequence(
            st.last_manifest.as_ref(),
            vm.manifest().sequence,
            vm.digest_hex(),
        )?;
        st.last_manifest = Some(vm.seen());
        self.write_state(&st)
    }

    /// The directory of the current version of `name`, if installed.
    pub fn current_dir(&self, name: &str) -> Result<Option<PathBuf>, ComponentError> {
        let st = self.state()?;
        Ok(st
            .components
            .get(name)
            .map(|c| self.root.join(name).join(&c.current.dir)))
    }

    /// Installs `name` from `vm` for `platform`. See the module docs for the sequence.
    #[allow(clippy::too_many_arguments)]
    pub fn install(
        &self,
        vm: &VerifiedManifest,
        name: &str,
        platform: Platform,
        root_key: &TrustRoot,
        fetcher: &dyn Fetcher,
        health: &dyn HealthCheck,
        now: u64,
    ) -> Result<InstallOutcome, ComponentError> {
        let mut st = self.state()?;
        check_sequence(
            st.last_manifest.as_ref(),
            vm.manifest().sequence,
            vm.digest_hex(),
        )?;
        let comp = vm
            .manifest()
            .component(name)
            .ok_or_else(|| ComponentError::UnknownComponent(name.to_string()))?;
        let (used_platform, art) =
            comp.artifact_for(platform)
                .ok_or_else(|| ComponentError::NoArtifactForPlatform {
                    component: name.to_string(),
                    platform: platform.to_string(),
                })?;
        // Record the manifest first: even an install that fails must not let an older manifest
        // in afterwards.
        st.last_manifest = Some(vm.seen());
        if let Some(cur) = st.components.get(name) {
            if cur.current.version == comp.version && cur.current.sha256 == art.sha256 {
                self.write_state(&st)?;
                return Ok(InstallOutcome::AlreadyCurrent {
                    version: comp.version.clone(),
                });
            }
        }
        self.write_state(&st)?;

        let staging = self.new_staging(name)?;
        let r = self.stage_and_swap(
            &mut st,
            comp,
            art,
            used_platform,
            root_key,
            fetcher,
            health,
            &staging,
            vm,
            now,
        );
        let _ = fs::remove_dir_all(&staging);
        r
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_and_swap(
        &self,
        st: &mut StoreState,
        comp: &Component,
        art: &crate::manifest::Artifact,
        used_platform: Platform,
        root_key: &TrustRoot,
        fetcher: &dyn Fetcher,
        health: &dyn HealthCheck,
        staging: &Path,
        vm: &VerifiedManifest,
        now: u64,
    ) -> Result<InstallOutcome, ComponentError> {
        let download = staging.join("download.bin");
        {
            let mut f = fs::File::create(&download).map_err(io)?;
            fetcher.fetch(&art.url, art.size, &mut f)?;
            f.sync_all().map_err(io)?;
        }
        // One pass over the file on disk: size, SHA-256 and the artifact signature.
        let (n, digest) = verify_stream(
            root_key,
            &art.signature,
            fs::File::open(&download).map_err(io)?,
            ARTIFACT_DOMAIN,
        )
        .or_else(|e| match e {
            // Report a size or hash problem ahead of the signature: it is the more precise cause.
            ComponentError::SignatureInvalid(_) | ComponentError::WrongTrustedComment => {
                let (n, d) = hash_file(&download)?;
                if n != art.size {
                    Err(ComponentError::SizeMismatch {
                        expected: art.size,
                        got: n,
                    })
                } else if d != art.sha256 {
                    Err(ComponentError::HashMismatch)
                } else {
                    Err(e)
                }
            }
            other => Err(other),
        })?;
        if n != art.size {
            return Err(ComponentError::SizeMismatch {
                expected: art.size,
                got: n,
            });
        }
        if digest != art.sha256 {
            return Err(ComponentError::HashMismatch);
        }

        let tree = staging.join("tree");
        extract(art.format, &download, &tree, &comp.name)?;
        // The health check sees only the artifact being installed.
        let mut only = comp.clone();
        only.artifacts.retain(|k, _| k == used_platform.as_str());
        health
            .check(&tree, &only)
            .map_err(ComponentError::HealthCheck)?;

        let comp_dir = self.root.join(&comp.name);
        fs::create_dir_all(&comp_dir).map_err(io)?;
        // The manifest's sha256 is checked as 64 hex digits; never slice past what is there.
        let short = art
            .sha256
            .get(..12)
            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| {
                ComponentError::ManifestMalformed("an artifact hash is not hex".into())
            })?;
        let dir_name = format!("{}-{short}", comp.version);
        let target = comp_dir.join(&dir_name);
        let old = st.components.get(&comp.name).cloned();
        let referenced = |d: &str| {
            old.as_ref().is_some_and(|o| {
                o.current.dir == d || o.previous.as_ref().is_some_and(|p| p.dir == d)
            })
        };
        if target.exists() {
            if old.as_ref().is_some_and(|o| o.current.dir == dir_name) {
                // Same bytes already current under another version string: nothing to swap.
                return Err(ComponentError::State(format!(
                    "{dir_name} is already current"
                )));
            }
            // An unreferenced leftover or the previous version: replaced by the fresh copy.
            fs::remove_dir_all(&target).map_err(io)?;
        }
        fs::rename(&tree, &target).map_err(io)?;
        sync_dir(&comp_dir);

        let new_version = InstalledVersion {
            version: comp.version.clone(),
            sha256: art.sha256.clone(),
            dir: dir_name.clone(),
            platform: used_platform,
            installed_at: now,
            manifest_sequence: vm.manifest().sequence,
        };
        let previous = old.as_ref().map(|o| o.current.clone());
        st.components.insert(
            comp.name.clone(),
            InstalledComponent {
                current: new_version,
                previous: previous.clone(),
            },
        );
        if let Err(e) = self.write_state(st) {
            // Not committed: the old version is still current. Remove the new tree unless an
            // older state still points at it.
            if !referenced(&dir_name) {
                let _ = fs::remove_dir_all(&target);
            }
            return Err(e);
        }
        // Committed. Remove the version that fell off (the old previous), if any.
        if let Some(o) = old {
            if let Some(p) = o.previous {
                if p.dir != dir_name && p.dir != o.current.dir {
                    let _ = fs::remove_dir_all(comp_dir.join(&p.dir));
                }
            }
        }
        Ok(InstallOutcome::Installed {
            version: comp.version.clone(),
            previous: previous.map(|p| p.version),
        })
    }

    /// Makes the previous version current (and the current one previous).
    pub fn rollback(&self, name: &str) -> Result<InstalledVersion, ComponentError> {
        let mut st = self.state()?;
        let c = st
            .components
            .get_mut(name)
            .ok_or_else(|| ComponentError::NoPrevious(name.to_string()))?;
        let prev = c
            .previous
            .clone()
            .ok_or_else(|| ComponentError::NoPrevious(name.to_string()))?;
        if !self.root.join(name).join(&prev.dir).is_dir() {
            return Err(ComponentError::NoPrevious(format!(
                "{name} (its directory is gone)"
            )));
        }
        c.previous = Some(c.current.clone());
        c.current = prev.clone();
        self.write_state(&st)?;
        Ok(prev)
    }

    /// Removes leftovers of an interrupted install: every staging directory, and every version
    /// directory the state does not reference. Run once at startup, before any install.
    pub fn recover(&self) -> Result<RecoveryReport, ComponentError> {
        let st = self.state()?;
        let mut rep = RecoveryReport::default();
        let staging = self.root.join(STAGING);
        if staging.is_dir() {
            for e in fs::read_dir(&staging).map_err(io)? {
                let e = e.map_err(io)?;
                fs::remove_dir_all(e.path()).map_err(io)?;
                rep.removed_staging += 1;
            }
        }
        for e in fs::read_dir(&self.root).map_err(io)? {
            let e = e.map_err(io)?;
            let name = e.file_name().to_string_lossy().to_string();
            if name == STAGING || !e.file_type().map_err(io)?.is_dir() {
                continue;
            }
            let keep: Vec<&str> = st
                .components
                .get(&name)
                .map(|c| {
                    let mut v = vec![c.current.dir.as_str()];
                    if let Some(p) = &c.previous {
                        v.push(p.dir.as_str());
                    }
                    v
                })
                .unwrap_or_default();
            for v in fs::read_dir(e.path()).map_err(io)? {
                let v = v.map_err(io)?;
                let vname = v.file_name().to_string_lossy().to_string();
                if !keep.contains(&vname.as_str()) {
                    if v.file_type().map_err(io)?.is_dir() {
                        fs::remove_dir_all(v.path()).map_err(io)?;
                    } else {
                        fs::remove_file(v.path()).map_err(io)?;
                    }
                    rep.removed_versions += 1;
                }
            }
        }
        Ok(rep)
    }

    fn new_staging(&self, name: &str) -> Result<PathBuf, ComponentError> {
        let base = self.root.join(STAGING);
        fs::create_dir_all(&base).map_err(io)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = base.join(format!("{name}-{nanos:x}-{:x}", std::process::id()));
        fs::create_dir(&p).map_err(io)?;
        Ok(p)
    }
}

fn hash_file(p: &Path) -> Result<(u64, String), ComponentError> {
    use sha2::Digest as _;
    let mut f = fs::File::open(p).map_err(io)?;
    let mut h = sha2::Sha256::new();
    let n = std::io::copy(&mut f, &mut h).map_err(io)?;
    Ok((n, hex::encode(h.finalize())))
}

#[cfg(unix)]
fn restrict(p: &Path) -> Result<(), ComponentError> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(p, fs::Permissions::from_mode(0o700)).map_err(io)
}

#[cfg(not(unix))]
fn restrict(_p: &Path) -> Result<(), ComponentError> {
    Ok(())
}

/// Best effort: persist a rename in `dir` (a no-op where directories cannot be opened).
fn sync_dir(dir: &Path) {
    if let Ok(f) = fs::File::open(dir) {
        let _ = f.sync_all();
    }
}
