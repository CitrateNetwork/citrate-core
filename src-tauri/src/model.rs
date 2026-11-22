//! citrate-core — local model download + verify (BC-3.1). @rule8-adjacent ·
//! gated download of a large binary (a supply-chain surface).
//!
//! Owner decision D-BC-1: a SLIM installer + a first-run STREAMED download of the
//! Gemma GGUF with a pinned SHA-256 verify, run via a bundled `llama-server`
//! sidecar (BC-3.2). The remote `infer.citrate.ai` gateway is the honest fallback
//! while the model is downloading / absent.
//!
//! ## What this module does (and what it refuses to do)
//! - **`model_status`** — the honest, file-derived state:
//!   `NotPresent | Downloading{…} | Verifying | Ready | Error{msg}`. `Ready` is
//!   EARNED only by a real `verify()` (a sha256 match written to a status file),
//!   NEVER by mere file presence (Rule 1). A raw-disk presence is at most a
//!   completed-but-unverified download.
//! - **`model_download`** — a STREAMED, **resumable** download. It writes to
//!   `models/<file>.part`, resumes from the current partial size via an HTTP
//!   `Range` request, checks the GGUF magic on the first bytes, and only renames
//!   `.part` → final once the full pinned length arrived. A wrong magic, a short
//!   stream, or a length shortfall ABORTS (no bogus finalize).
//! - **`model_verify`** — streams the final file through SHA-256, compares to the
//!   pinned [`MODEL_SHA256`], and on ANY mismatch (or size mismatch) QUARANTINES
//!   the file and returns an error. It never marks `Ready` on a mismatch.
//!
//! ## Secret discipline
//! No key material anywhere. A public model file + a public URL + a public hash.
//! No `#[tauri::command]` here returns or takes a secret. The download uses the
//! existing `ureq` client (as ai.rs/rpc.rs do) — llama-server is a SPAWNED binary
//! (BC-3.2), NOT a cargo dep, so no llama.cpp bindings / heavy crates enter the
//! tree.
//!
//! ## Testability (injectable transport, like rpc.rs::RpcTransport)
//! The HTTP transport is a trait ([`ModelTransport`]) so tests drive a SMALL
//! fixture stream — the checksum/resume/magic/short-stream guards are proven
//! WITHOUT a 5 GB pull or a live socket. The real 5 GB download + real
//! llama-server inference are a documented manual/integration proof (SCOPE.md).

// BC-3.1: the production `UreqModelTransport` only runs in a real download path;
// the fixture transport drives CI. Some constructor/status surface is reached
// only by BC-3.2 + the tests until the onboarding step wires it, mirroring
// node.rs/memory.rs.
#![allow(dead_code)]

use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pinned, grounded facts (verified 2026-07-19 — do NOT re-derive). Rule 1: the
// size + hash + URL are the single source of truth the verify path enforces.
// ---------------------------------------------------------------------------

/// The Gemma GGUF file name (also the app-data filename + the final rename
/// target). The `.part` in-progress file is `<MODEL_FILE>.part`.
pub const MODEL_FILE: &str = "gemma-4-E4B-it-Q4_0.gguf";

/// The exact byte length of the model (4,590,807,392 bytes ≈ 4.28 GiB / 4.59 GB).
/// The streamed download refuses to finalize until exactly this many bytes have
/// arrived, and `verify` re-checks the on-disk length against it.
pub const MODEL_SIZE_BYTES: u64 = 4_590_807_392;

/// The pinned SHA-256 of the model file. `verify` streams the file through
/// SHA-256 and compares to THIS; any mismatch quarantines the file (never Ready).
/// (Re-pinned 2026-07-23: the repo's Q4_K_M was replaced by Q4_0; this is the live
/// `ggml-org/gemma-4-E4B-it-GGUF` Q4_0 sha256 — also the bundled/seeded model.)
pub const MODEL_SHA256: &str = "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2";

/// The default source URL: a Citrate-controlled 307 vanity redirect to the model blob (today it
/// hops to the HF `ggml-org/gemma-4-E4B-it-GGUF` Q4_0 resolve path; our reqwest downloader follows
/// the redirect chain transparently). The indirection is deliberate — the backing store can move
/// (HF → a DO Spaces mirror) with a one-line redirect flip and ZERO app rebuild (307 = uncached).
/// Safe because the download is integrity-gated regardless of source: the first bytes must be the
/// GGUF magic, the total must equal [`MODEL_SIZE_BYTES`], and the stream must hash to
/// [`MODEL_SHA256`] or the file is quarantined (never Ready). Overridable via `CITRATE_MODEL_URL`.
/// (Verified end-to-end 2026-09-01: /download/model → 307 → HF → 200, content-length == MODEL_SIZE_BYTES.)
pub const DEFAULT_MODEL_URL: &str = "https://citrate.ai/download/model";

/// The env var that overrides [`DEFAULT_MODEL_URL`] (the config seam).
pub const MODEL_URL_ENV: &str = "CITRATE_MODEL_URL";

/// The GGUF header magic ("GGUF", `0x47 0x47 0x55 0x46`). Checked on the first
/// bytes of the download so a wrong/HTML-error body is caught immediately.
pub const GGUF_MAGIC: [u8; 4] = *b"GGUF";

// The status side-file is now DERIVED per model as `<file>.status.json` (ModelManager),
// so multiple models each keep their own verified flag. (Was a single STATUS_FILE const.)

/// Where a model's bytes come from (CX-S1 catalog).
/// Where a model's bytes come from. Serializes to the bridge DTO's `"hf" | "github" |
/// "bundled"` (CX-S1 `ModelDescriptor.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// A Hugging Face Hub repo file (resolve URL).
    Hf,
    /// A GitHub release asset (`revision` is the release tag).
    Github,
    /// Seeded from the app bundle / the app's default first-run download; no catalog fetch.
    Bundled,
}

/// A model's on-disk format. Serializes to the bridge DTO's `"gguf" | "safetensors"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Gguf,
    Safetensors,
}

/// A downloadable/local model — mirrors the bridge `ModelDescriptor` DTO 1:1 (camelCase over
/// the wire). The pinned `sha256`/`size_bytes` are the verify gate; `.part` and `.status.json`
/// derive from `file`, so models coexist in `models/` by filename. The download URL is DERIVED
/// from (source, repo, revision, file) via [`ModelDescriptor::download_url`] — never stored, so
/// a descriptor stays self-describing and can't drift from its own URL (CX-S1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    /// Stable id (e.g. "hf:<repo>/<file>" or "bundled:gemma-4-e4b").
    pub id: String,
    pub source: ModelSource,
    /// The `owner/name` repo slug on the source (empty for a source that has none).
    pub repo: String,
    /// The on-disk GGUF/safetensors filename.
    pub file: String,
    /// The revision — HF git ref or GitHub release tag. Defaults to `main` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Exact byte length (the download refuses to finalize until this many bytes arrive).
    pub size_bytes: u64,
    /// Pinned lowercase-hex SHA-256 (verify quarantines on mismatch — no Ready without it).
    pub sha256: String,
    pub kind: ModelKind,
}

impl ModelDescriptor {
    /// The HTTP URL the bytes are fetched from — DERIVED from the descriptor, never stored.
    /// `None` for a bundled model (seeded from the app, not downloaded from a catalog source).
    /// This derives the DIRECT source for an HF/GitHub catalog descriptor; the app's own default
    /// download instead goes through [`DEFAULT_MODEL_URL`] (a Citrate vanity redirect), which is a
    /// separate config seam — the two are intentionally decoupled so the default's backing store can
    /// move without touching catalog descriptors.
    pub fn download_url(&self) -> Option<String> {
        let rev = self.revision.as_deref().unwrap_or("main");
        match self.source {
            ModelSource::Hf => Some(format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                self.repo, rev, self.file
            )),
            ModelSource::Github => Some(format!(
                "https://github.com/{}/releases/download/{}/{}",
                self.repo, rev, self.file
            )),
            ModelSource::Bundled => None,
        }
    }
}

/// The default Gemma descriptor — the single model the app has always shipped, now expressed
/// as a descriptor so the catalog can add more alongside it. Its values are the existing pins
/// ([`MODEL_FILE`]/[`MODEL_SIZE_BYTES`]/[`MODEL_SHA256`]); its HF `download_url()` derives the direct
/// Hugging Face resolve path (the vanity [`DEFAULT_MODEL_URL`] currently redirects there). `source` is
/// `Bundled` (it is the app's built-in default), while the
/// repo/revision still point at the real HF resolve path so a re-download resolves correctly.
pub fn default_descriptor() -> ModelDescriptor {
    ModelDescriptor {
        id: "bundled:gemma-4-e4b-it-q4_0".to_string(),
        source: ModelSource::Bundled,
        repo: "ggml-org/gemma-4-E4B-it-GGUF".to_string(),
        file: MODEL_FILE.to_string(),
        revision: Some("main".to_string()),
        size_bytes: MODEL_SIZE_BYTES,
        sha256: MODEL_SHA256.to_string(),
        kind: ModelKind::Gguf,
    }
}

/// Resolve the model source URL: the `override` (from `CITRATE_MODEL_URL`) if
/// present, else [`DEFAULT_MODEL_URL`]. Taken as an explicit arg (not read inline)
/// so tests are deterministic and env-race-free.
pub fn resolve_model_url(override_url: Option<String>) -> String {
    override_url.unwrap_or_else(|| DEFAULT_MODEL_URL.to_string())
}

// ---------------------------------------------------------------------------
// The pinned onboarding models (2026-11-22, owner: "the user can access a decent local model on
// the way into the app"). Each one is a fixed file + length + SHA-256, and an ordered list of
// UNSIGNED, stable source URLs. A signed CDN URL (the Hugging Face Xet CDN answers a resolve with a
// URL that expires after about an hour) is never stored anywhere: every attempt starts again from
// one of these URLs and follows the redirects fresh.
// ---------------------------------------------------------------------------

/// One pinned local model the onboarding step can download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedModel {
    /// Stable key over the bridge: `"default"` or `"small"`.
    pub key: &'static str,
    /// Display name.
    pub label: &'static str,
    /// The file name in `models/`.
    pub file: &'static str,
    /// Exact byte length.
    pub size_bytes: u64,
    /// Lowercase-hex SHA-256.
    pub sha256: &'static str,
    /// The Hugging Face repo the file is published in.
    pub hf_repo: &'static str,
    /// The repo COMMIT the file is pinned at (a commit never changes; `main` can be re-uploaded,
    /// as happened when Q4_K_M was replaced by Q4_0).
    pub hf_revision: &'static str,
    /// A Citrate redirect tried before Hugging Face, if any.
    pub vanity_url: Option<&'static str>,
    /// Citrate-controlled mirrors (unsigned, stable public URLs), tried first.
    ///
    /// Empty until the owner approves publishing the file on the Citrate CDN. The proposed object is
    /// `https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/models/<file>` (the same Spaces
    /// CDN that serves the updater feed and the component manifest); adding that URL here is the
    /// whole change. A mirror needs no trust: the bytes pass the same magic, length and SHA-256 gate.
    pub mirror_urls: &'static [&'static str],
}

impl PinnedModel {
    /// The Hugging Face resolve URL at the pinned commit.
    pub fn hf_url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.hf_repo, self.hf_revision, self.file
        )
    }

    /// The sources in order: Citrate mirrors, the Citrate redirect, Hugging Face.
    pub fn source_urls(&self) -> Vec<String> {
        let mut v: Vec<String> = self.mirror_urls.iter().map(|u| u.to_string()).collect();
        if let Some(u) = self.vanity_url {
            v.push(u.to_string());
        }
        v.push(self.hf_url());
        v
    }
}

/// The default onboarding model: Gemma 4 E4B instruct, Q4_0 (4.59 GB). Unchanged pins.
pub const DEFAULT_PIN: PinnedModel = PinnedModel {
    key: "default",
    label: "Gemma 4 E4B",
    file: MODEL_FILE,
    size_bytes: MODEL_SIZE_BYTES,
    sha256: MODEL_SHA256,
    hf_repo: "ggml-org/gemma-4-E4B-it-GGUF",
    hf_revision: "8903cb408c7df39cde2ed15fac397e6d7be92951",
    vanity_url: Some(DEFAULT_MODEL_URL),
    mirror_urls: &[],
};

/// The smaller option for a slow connection or a full disk: Gemma 4 E2B instruct, Q4_0 (2.84 GB),
/// the same family and quantisation as the default, so the bundled llama-server runs it the same
/// way. Offered, never chosen for the member. (Pins read from the Hugging Face API 2026-11-22:
/// `x-linked-size` and `x-linked-etag` of the file at the pinned commit.)
pub const SMALL_PIN: PinnedModel = PinnedModel {
    key: "small",
    label: "Gemma 4 E2B",
    file: "gemma-4-E2B-it-Q4_0.gguf",
    size_bytes: 2_841_481_184,
    sha256: "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
    hf_repo: "ggml-org/gemma-4-E2B-it-GGUF",
    hf_revision: "2a8538c2e2d4687953a86c422c7c623cfd022880",
    vanity_url: None,
    mirror_urls: &[],
};

/// Which pinned model the member's onboarding download is for. Persisted in
/// `models/local-model.json`; absent means [`LocalModel::Default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalModel {
    #[default]
    Default,
    Small,
}

impl LocalModel {
    pub fn pin(self) -> &'static PinnedModel {
        match self {
            LocalModel::Default => &DEFAULT_PIN,
            LocalModel::Small => &SMALL_PIN,
        }
    }

    /// Parse a bridge key (`"default"` / `"small"`).
    pub fn from_key(key: &str) -> Option<LocalModel> {
        match key {
            "default" => Some(LocalModel::Default),
            "small" => Some(LocalModel::Small),
            _ => None,
        }
    }
}

const LOCAL_CHOICE_FILE: &str = "local-model.json";

#[derive(Serialize, Deserialize)]
struct LocalChoiceFile {
    model: LocalModel,
}

/// The persisted onboarding model choice in `dir` (the default when none is recorded).
pub fn read_local_choice(dir: &std::path::Path) -> LocalModel {
    std::fs::read(dir.join(LOCAL_CHOICE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<LocalChoiceFile>(&b).ok())
        .map(|c| c.model)
        .unwrap_or_default()
}

/// Record the onboarding model choice in `dir`.
pub fn write_local_choice(dir: &std::path::Path, model: LocalModel) -> Result<()> {
    std::fs::create_dir_all(dir).map_err(|e| ModelError::Io(e.to_string()))?;
    let blob = serde_json::to_vec(&LocalChoiceFile { model })
        .map_err(|e| ModelError::Io(e.to_string()))?;
    std::fs::write(dir.join(LOCAL_CHOICE_FILE), blob).map_err(|e| ModelError::Io(e.to_string()))
}

/// The sources the default model is downloaded from, in order. With `CITRATE_MODEL_URL` set that
/// URL is the only source. Otherwise [`DEFAULT_PIN`]'s list: Citrate mirrors, the
/// [`DEFAULT_MODEL_URL`] redirect, then Hugging Face at the pinned commit, so the first-run model
/// step still works when one of them stops serving the model. Every source passes the same gate:
/// GGUF magic, pinned length, pinned SHA-256.
pub fn model_source_urls(override_url: Option<String>) -> Vec<String> {
    match override_url {
        Some(u) => vec![u],
        None => DEFAULT_PIN.source_urls(),
    }
}

/// Whether `e` says the SOURCE is not serving the model, so this run stops asking it and moves on:
/// a body that is not a GGUF file (a web page), a resume answered without a matching
/// `206 Partial Content`, a file of another size, or a missing object (404).
fn source_is_unusable(e: &ModelError) -> bool {
    matches!(
        e,
        ModelError::BadMagic
            | ModelError::RangeIgnored
            | ModelError::WrongSize { .. }
            | ModelError::NotServed(_)
    )
}

/// Whether `e` is a network failure that outlasted one source's retries (try the other sources,
/// then come back to this one).
fn is_transient(e: &ModelError) -> bool {
    matches!(e, ModelError::Transport(_) | ModelError::Stalled { .. })
}

/// Passes over all the sources with no byte gained before [`download_from_sources`] (and a caller
/// that asks for a bound) gives up. The app's own download ([`ModelDownloadOptions::for_pin`]) has
/// no bound: it waits for the connection for as long as the app is open. Any byte gained starts
/// the count again.
pub const MAX_SOURCE_PASSES: u32 = 4;

/// Wait before the next pass over the sources: 30 s, 60 s, then 120 s.
fn default_pass_backoff(dry_passes: u32) -> std::time::Duration {
    std::time::Duration::from_secs((30u64 << dry_passes.saturating_sub(1).min(2)).min(120))
}

/// The download settings [`download_with`] runs with.
pub struct ModelDownloadOptions {
    pub file: String,
    pub expected_hash: String,
    pub expected_size: u64,
    pub segment_bytes: u64,
    pub retry_backoff: fn(u32) -> std::time::Duration,
    pub stall_timeout: std::time::Duration,
    pub free_space: fn(&std::path::Path) -> Option<u64>,
    /// Consecutive passes over the sources without progress before giving up.
    pub max_passes: u32,
    /// The wait before pass `n + 1` after `n` passes in a row without progress.
    pub pass_backoff: fn(u32) -> std::time::Duration,
    /// The sources' unsigned URLs, recorded in the `.part` sidecar (never a redirect target).
    pub source_labels: Vec<String>,
}

impl ModelDownloadOptions {
    /// Production settings for a pinned model.
    pub fn for_pin(pin: &PinnedModel, sources: Vec<String>) -> Self {
        ModelDownloadOptions {
            file: pin.file.to_string(),
            expected_hash: pin.sha256.to_string(),
            expected_size: pin.size_bytes,
            segment_bytes: SEGMENT_BYTES,
            retry_backoff: default_retry_backoff,
            stall_timeout: STALL_TIMEOUT,
            free_space: disk_free,
            // While the app is open a lost connection is never final: the download keeps waiting
            // (passes 2 minutes apart at most) and resumes by itself when the network returns.
            // The member can pause, or choose the smaller model, at any time.
            max_passes: u32::MAX,
            pass_backoff: default_pass_backoff,
            source_labels: sources,
        }
    }
}

/// Download the model at `dir/<MODEL_FILE>` from the first of `sources` that serves it (the
/// production settings otherwise). Kept for the callers and tests that predate [`download_with`].
pub fn download_from_sources(
    dir: &std::path::Path,
    sources: Vec<Box<dyn ModelTransport>>,
    expected_hash: &str,
    expected_size: u64,
    retry_backoff: fn(u32) -> std::time::Duration,
) -> Result<()> {
    let opts = ModelDownloadOptions {
        file: MODEL_FILE.to_string(),
        expected_hash: expected_hash.to_string(),
        expected_size,
        segment_bytes: SEGMENT_BYTES,
        retry_backoff,
        stall_timeout: STALL_TIMEOUT,
        free_space: disk_free,
        max_passes: 1,
        pass_backoff: default_pass_backoff,
        source_labels: Vec::new(),
    };
    download_with(dir, sources, &opts)
}

/// Download `dir/<opts.file>` from `sources`, in order.
///
/// - A source that is not serving the model ([`source_is_unusable`]) is dropped for this run.
/// - A network failure that outlasts one source's retries moves on to the next source; after a
///   pass over all of them the download waits and goes round again, until [`ModelDownloadOptions::
///   max_passes`] passes in a row gained no byte.
/// - A partial left by one source is resumed by the next (the same pinned file; the SHA-256 verify
///   covers every byte, and a mixed file that does not match is deleted, never Ready).
/// - Anything else (a gated repo, a full disk, a pause, a second download) is returned at once.
pub fn download_with(
    dir: &std::path::Path,
    sources: Vec<Box<dyn ModelTransport>>,
    opts: &ModelDownloadOptions,
) -> Result<()> {
    let sources: Vec<std::sync::Arc<dyn ModelTransport>> =
        sources.into_iter().map(std::sync::Arc::from).collect();
    let part = dir.join(format!("{}.part", opts.file));
    let _session = BusyMark::new(part.clone());
    let part_len = || std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut unusable = vec![false; sources.len()];
    let mut last = ModelError::Transport("no model source configured".into());
    let mut dry_passes: u32 = 0;
    loop {
        let before = part_len();
        for (i, transport) in sources.iter().enumerate() {
            if unusable[i] {
                continue;
            }
            let mgr = ModelManager::with_shared_transport(
                dir.to_path_buf(),
                transport.clone(),
                opts.expected_hash.clone(),
                opts.expected_size,
            )
            .with_file(opts.file.clone())
            .with_segment_bytes(opts.segment_bytes)
            .with_retry_backoff(opts.retry_backoff)
            .with_stall_timeout(opts.stall_timeout)
            .with_free_space_probe(opts.free_space)
            .with_source_labels(opts.source_labels.clone());
            match mgr.download() {
                Ok(_) => return Ok(()),
                Err(e) if source_is_unusable(&e) => {
                    crate::model_log::event(&format!(
                        "source {} is not serving the model: {e}",
                        source_label(&opts.source_labels, i)
                    ));
                    unusable[i] = true;
                    last = e;
                }
                Err(e) if is_transient(&e) => {
                    crate::model_log::event(&format!(
                        "source {} failed after its retries: {e}",
                        source_label(&opts.source_labels, i)
                    ));
                    last = e;
                }
                Err(e) => return Err(e),
            }
        }
        if unusable.iter().all(|u| *u) {
            return Err(last);
        }
        if part_len() > before {
            dry_passes = 0;
        } else {
            dry_passes += 1;
        }
        if dry_passes >= opts.max_passes.max(1) {
            return Err(last);
        }
        let wait = (opts.pass_backoff)(dry_passes);
        crate::model_log::event(&format!(
            "every source failed this pass; trying again in {}s ({last})",
            wait.as_secs()
        ));
        set_live_retry(&part, Some(wait), Some(last.user_message()));
        if !sleep_unless_paused(&part, wait) {
            set_live_retry(&part, None, None);
            let _ = mark_paused(dir, &opts.file, opts.expected_size, &opts.expected_hash);
            crate::model_log::event(&format!(
                "{}: paused while waiting for the connection",
                opts.file
            ));
            return Err(ModelError::Paused);
        }
        set_live_retry(&part, None, None);
    }
}

fn source_label(labels: &[String], i: usize) -> String {
    labels
        .get(i)
        .map(|u| crate::model_log::host_of(u))
        .unwrap_or_else(|| format!("#{}", i + 1))
}

/// Download and verify `dir/<file>` with `download`, and when the verify finds the bytes do not
/// match the pinned SHA-256 (the file is then deleted), run ONE more clean download from byte 0 and
/// verify again. A partial that came from another file, or a source that served wrong bytes once,
/// then heals without the member doing anything; a second mismatch is final.
pub fn download_and_verify_with_clean_retry(
    dir: &std::path::Path,
    file: &str,
    expected_hash: &str,
    expected_size: u64,
    mut download: impl FnMut() -> Result<()>,
) -> Result<()> {
    let verifier = || {
        ModelManager::new(
            dir.to_path_buf(),
            Box::new(UreqModelTransport::new(String::new())),
            expected_hash.to_string(),
            expected_size,
        )
        .with_file(file.to_string())
    };
    let part = dir.join(format!("{file}.part"));
    for attempt in 1..=2 {
        download()?;
        let mark = BusyMark::new(part.clone());
        let verified = verifier().verify();
        drop(mark);
        match verified {
            Ok(()) => {
                crate::model_log::event(&format!("{file}: verified (sha256 matched)"));
                return Ok(());
            }
            Err(ModelError::ChecksumMismatch) if attempt == 1 => {
                crate::model_log::event(&format!(
                    "{file}: checksum mismatch, file deleted; downloading it again from the start"
                ));
                let _ = std::fs::remove_file(dir.join(format!("{file}.part")));
                remove_part_meta(dir, file);
            }
            Err(e) => {
                crate::model_log::event(&format!("{file}: verify failed: {e}"));
                return Err(e);
            }
        }
    }
    Err(ModelError::ChecksumMismatch)
}

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free (this module handles no secrets).
// ---------------------------------------------------------------------------

/// A model download/verify error. Every `Display` is safe to surface.
#[derive(Debug)]
pub enum ModelError {
    /// The HTTP transport failed (connection, TLS, non-2xx, range).
    Transport(String),
    /// The first bytes were not the GGUF magic (wrong/HTML-error body). Aborted.
    BadMagic,
    /// The stream ended before the pinned total length arrived (truncated/failed).
    ShortStream { got: u64, want: u64 },
    /// The stream delivered MORE than the pinned total length — e.g. a server that
    /// ignores `Range` and re-sends the full body onto a resume. The oversized
    /// `.part` is reset; the download NEVER finalizes an over-length file.
    Overshoot { got: u64, want: u64 },
    /// The on-disk file length did not match the pinned [`MODEL_SIZE_BYTES`].
    SizeMismatch { got: u64, want: u64 },
    /// The streamed SHA-256 did not equal the pinned [`MODEL_SHA256`] — the file
    /// was quarantined; NEVER marked Ready.
    ChecksumMismatch,
    /// No model file (or partial) is present to verify.
    NotPresent,
    /// A filesystem error (create dir, write, rename, read).
    Io(String),
    /// HUP-S0.3: another download of the SAME file is already running in this process. A second
    /// writer appending to the same `.part` corrupts it, so it is refused (the UI attaches to the
    /// running download's progress events instead).
    AlreadyDownloading,
    /// HUP-S0.3: a resume (`Range` from a non-zero offset) got a full `200` body instead of
    /// `206 Partial Content`. Refused up front, before any byte is appended to the `.part`.
    RangeIgnored,
    /// HUP-S0.3b: a Hugging Face origin answered `401`/`403`: the repo is gated or private and
    /// needs a token with access. Final (not retried); the message says where to add the token.
    Gated { token_sent: bool },
    /// A different model is already saved under this file name; nothing was changed.
    Occupied,
    /// No byte arrived for `secs` seconds; the attempt was abandoned (retried from the exact byte).
    Stalled { secs: u64 },
    /// The disk holding the model folder has `free` bytes and the download needs `need`.
    DiskFull { need: u64, free: u64 },
    /// The member paused the download; the `.part` is kept and resumes on the next start.
    Paused,
    /// The source has no such file (`404`): it is skipped for this download.
    NotServed(u16),
    /// The source serves a file of another length than the pinned one: it is skipped.
    WrongSize { got: u64, want: u64 },
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Transport(m) => write!(f, "model transport error: {m}"),
            ModelError::AlreadyDownloading => {
                write!(
                    f,
                    "this model is already downloading; progress continues in the background"
                )
            }
            ModelError::RangeIgnored => {
                write!(
                    f,
                    "the model server ignored the resume range (expected 206 Partial Content)"
                )
            }
            ModelError::Gated { token_sent } => write!(
                f,
                "{}",
                crate::hf_auth::FetchError::Gated {
                    token_sent: *token_sent
                }
            ),
            ModelError::BadMagic => write!(f, "model download is not a GGUF file (bad magic)"),
            ModelError::Occupied => write!(
                f,
                "a different model is already saved under this file name; nothing was changed"
            ),
            ModelError::ShortStream { got, want } => {
                write!(f, "model download ended short: {got} of {want} bytes")
            }
            ModelError::Overshoot { got, want } => {
                write!(
                    f,
                    "model download overshot: {got} exceeds expected {want} bytes"
                )
            }
            ModelError::SizeMismatch { got, want } => {
                write!(f, "model file size mismatch: {got} != expected {want}")
            }
            ModelError::ChecksumMismatch => {
                write!(f, "model checksum mismatch — file quarantined (not ready)")
            }
            ModelError::NotPresent => write!(f, "no model file present to verify"),
            ModelError::Io(m) => write!(f, "model io error: {m}"),
            ModelError::Stalled { secs } => {
                write!(f, "model download stalled: no data for {secs}s")
            }
            ModelError::DiskFull { need, free } => write!(
                f,
                "not enough disk space for the model: need {need} bytes, {free} free"
            ),
            ModelError::Paused => write!(f, "model download paused"),
            ModelError::NotServed(s) => write!(f, "the model source has no such file (HTTP {s})"),
            ModelError::WrongSize { got, want } => write!(
                f,
                "the model source serves a file of {got} bytes, expected {want}"
            ),
        }
    }
}

/// Bytes as GB with two decimals (decimal GB, as the onboarding step shows sizes).
fn gb(bytes: u64) -> String {
    format!("{:.2} GB", bytes as f64 / 1e9)
}

impl ModelError {
    /// What the member reads: plain words, what happened and what to do. Always one or more
    /// complete sentences. (`Display` stays the technical line the log keeps.)
    pub fn user_message(&self) -> String {
        match self {
            ModelError::Transport(m) => format!(
                "The model servers could not be reached ({}). Your progress is saved; Retry continues where it stopped.",
                m.trim_end_matches('.')
            ),
            ModelError::Stalled { secs } => format!(
                "The connection stopped sending data for {secs} seconds. Your progress is saved; Retry continues where it stopped."
            ),
            ModelError::DiskFull { need, free } => format!(
                "Not enough free disk space for the local model: it needs {} and this disk has {} free. Free up space and retry, or choose the smaller model.",
                gb(*need),
                gb(*free)
            ),
            ModelError::Paused => "The download is paused. Resume continues where it stopped.".to_string(),
            ModelError::ChecksumMismatch | ModelError::SizeMismatch { .. } => {
                "The downloaded file did not match the published checksum, so it was deleted. Retry downloads it again.".to_string()
            }
            ModelError::BadMagic
            | ModelError::RangeIgnored
            | ModelError::NotServed(_)
            | ModelError::WrongSize { .. } => {
                "None of the model servers is serving the model file right now. Retry in a few minutes, or choose the smaller model.".to_string()
            }
            ModelError::ShortStream { .. } | ModelError::Overshoot { .. } => {
                "The model server sent an incomplete file. Your progress is saved; Retry continues where it stopped.".to_string()
            }
            ModelError::Io(m) => format!(
                "The model could not be saved on this computer ({}). Check the disk and retry.",
                m.trim_end_matches('.')
            ),
            ModelError::AlreadyDownloading => {
                "The model is already downloading. Its progress shows here.".to_string()
            }
            ModelError::Gated { .. } | ModelError::Occupied | ModelError::NotPresent => {
                let m = self.to_string();
                if m.ends_with('.') { m } else { format!("{m}.") }
            }
        }
    }
}

/// An I/O error while writing the `.part`: a full disk (`ENOSPC`) is [`ModelError::DiskFull`]
/// (`need` = bytes still to write, `free` = what the disk reports), anything else is `Io`.
pub fn io_to_model_error(e: std::io::Error, need: u64, free: Option<u64>) -> ModelError {
    #[cfg(unix)]
    if e.raw_os_error() == Some(libc::ENOSPC) {
        return ModelError::DiskFull {
            need,
            free: free.unwrap_or(0),
        };
    }
    let _ = (need, free);
    ModelError::Io(e.to_string())
}

impl std::error::Error for ModelError {}

type Result<T> = std::result::Result<T, ModelError>;

// ---------------------------------------------------------------------------
// The status shape surfaced to the frontend (serde camelCase). `Ready` is EARNED
// only by verify (Rule 1). A tagged enum so the frontend renders each honest
// state (notPresent / downloading{pct} / verifying / ready / error{msg}).
// ---------------------------------------------------------------------------

/// The honest model state. Serialized as `{ state, … }` (camelCase) so the bridge
/// `model` domain reads one contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ModelStatus {
    /// No file and no partial download present.
    NotPresent,
    /// A `.part` download is in progress (or was interrupted). `pct` in [0,100]. While a download
    /// runs in this process the live fields say how fast it moves, when it should finish, and,
    /// while it waits to retry, how long and why.
    #[serde(rename_all = "camelCase")]
    Downloading {
        downloaded_bytes: u64,
        total_bytes: u64,
        pct: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bytes_per_sec: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        eta_secs: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_in_secs: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
    },
    /// The member paused the download; the `.part` is kept (no download runs).
    #[serde(rename_all = "camelCase")]
    Paused {
        downloaded_bytes: u64,
        total_bytes: u64,
        pct: f64,
    },
    /// A completed file is being streamed through SHA-256.
    Verifying,
    /// The file is present AND a real verify matched the pinned hash. The ONLY
    /// state in which the local model may be served (BC-3.2 fail-closed gate).
    Ready,
    /// An error occurred (bad magic, mismatch, io). Carries a safe message.
    Error { msg: String },
}

// ---------------------------------------------------------------------------
// The status side-file (records the EARNED verified flag).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StatusFile {
    /// True iff a real `verify()` matched the pinned hash for the current file.
    verified: bool,
    /// The SHA-256 that `verify()` matched (lowercase hex). Absent when not verified, and in a
    /// status file written before the hash was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

/// Whether `<dir>/<file>` is a verified-Ready model, WITHOUT needing a transport or the pinned
/// size — reads the same on-disk facts as [`ModelManager::status`] (the final file is present
/// AND its `<file>.status.json` records the EARNED `verified` flag). The model-SELECT path
/// (CX-S1.5) gates on this for an arbitrary catalog model file, where no per-model
/// [`ModelManager`] is otherwise in hand. No Ready without a real verify still holds.
pub fn is_file_ready(dir: &std::path::Path, file: &str) -> bool {
    if !dir.join(file).exists() {
        return false;
    }
    std::fs::read_to_string(dir.join(format!("{file}.status.json")))
        .ok()
        .and_then(|s| serde_json::from_str::<StatusFile>(&s).ok())
        .map(|s| s.verified)
        .unwrap_or(false)
}

/// The SHA-256 a verified-Ready `<dir>/<file>` was verified against, or `None` when the file is
/// not verified-Ready or its status names no hash. Tells which bytes a ready file holds without
/// hashing it again (the Store uses it to recognise its own install).
pub fn verified_sha256(dir: &std::path::Path, file: &str) -> Option<String> {
    if !dir.join(file).exists() {
        return None;
    }
    std::fs::read_to_string(dir.join(format!("{file}.status.json")))
        .ok()
        .and_then(|s| serde_json::from_str::<StatusFile>(&s).ok())
        .filter(|s| s.verified)
        .and_then(|s| s.sha256)
}

// ---------------------------------------------------------------------------
// The HTTP transport seam (injectable — mirrors rpc.rs::RpcTransport). Two ops:
// the total size (a HEAD-like read) and a byte-range GET stream from an offset
// (the resume primitive). Tests inject a fixture; production is `ureq`.
// ---------------------------------------------------------------------------

/// The HTTP operations the streamed, resumable download needs. Abstracted so
/// tests drive a small fixture stream (no 5 GB pull, no network) and assert the
/// resume/magic/short-stream behaviour on the request shape.
pub trait ModelTransport: Send + Sync {
    /// The full content length of the remote model (bytes). Used to seed the
    /// progress total and the finalize length check.
    fn total_size(&self) -> Result<u64>;
    /// A streaming reader of the remote body STARTING at byte `offset` (an HTTP
    /// `Range: bytes=offset-`). `offset == 0` is a full GET; `offset > 0` is a
    /// RESUME. The returned reader yields the remaining bytes.
    fn get_from(&self, offset: u64) -> Result<Box<dyn Read + Send>>;
    /// HUP-S0.3: a reader for the byte range `[start, end)` — one download SEGMENT. Production
    /// sends `Range: bytes=start-(end-1)` under a per-segment deadline, so a stalled body fails the
    /// segment (and is retried from the exact byte) instead of hanging forever. The default serves
    /// the remainder from `start` (fixtures); the download loop tolerates either shape.
    fn get_range(&self, start: u64, _end: u64) -> Result<Box<dyn Read + Send>> {
        self.get_from(start)
    }
}

/// HUP-S0.3 — bytes requested per ranged segment. Each attempt starts again from the origin URL,
/// so a segment is also the longest any one signed CDN URL is used for.
pub const SEGMENT_BYTES: u64 = 64 << 20; // 64 MiB
/// A backstop deadline for one segment request (connect, headers and body). The real guard is
/// [`STALL_TIMEOUT`]: a segment still receiving bytes is never abandoned before this, and this
/// only bounds a request whose socket stopped answering, so its reader thread ends.
pub const SEGMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// No byte (and no response) for this long abandons the attempt; it is retried from the exact byte
/// on disk. A slow download that keeps moving is never cut, however long it takes.
pub const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);
/// HUP-S0.3 — consecutive transport failures tolerated (with backoff) before giving up. Any
/// segment that makes progress resets the count, so a slow-but-alive link always finishes.
pub const MAX_SEGMENT_RETRIES: u32 = 8;
/// Free space kept beyond the model's remaining bytes before a download starts.
pub const DISK_MARGIN_BYTES: u64 = 256 << 20; // 256 MiB

/// Default retry backoff: 1, 2, 4, 8, 16, 30, 30… seconds.
fn default_retry_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_secs((1u64 << attempt.min(5).saturating_sub(1)).min(30))
}

/// HUP-S0.3 — a resume must be answered with `206 Partial Content`. A `200` on a non-zero start
/// means the server ignored `Range` and would re-send the whole file onto the `.part`.
pub fn check_range_status(start: u64, status: u16) -> Result<()> {
    match (start, status) {
        (_, 206) => Ok(()),
        (0, 200) => Ok(()),
        (s, 200) if s > 0 => Err(ModelError::RangeIgnored),
        (_, other) => Err(ModelError::Transport(format!(
            "unexpected HTTP status {other}"
        ))),
    }
}

/// A `206` must describe the bytes asked for: its `Content-Range` must start at `start` and, when
/// the pinned length is known, end the file at that length. Otherwise the source is answering for
/// some other range or file, and nothing it sends may be appended to the `.part`.
pub fn check_content_range(
    start: u64,
    status: u16,
    content_range: Option<&str>,
    content_length: Option<u64>,
    expected_total: Option<u64>,
) -> Result<()> {
    if status == 200 {
        // A full body (only accepted at offset 0 by check_range_status) is the whole file.
        return match (content_length, expected_total) {
            (Some(got), Some(want)) if got != want => Err(ModelError::WrongSize { got, want }),
            _ => Ok(()),
        };
    }
    let Some(cr) = content_range else {
        return if start == 0 {
            Ok(())
        } else {
            Err(ModelError::RangeIgnored)
        };
    };
    // "bytes <first>-<last>/<total|*>"
    let spec = cr.trim().strip_prefix("bytes").map(str::trim).unwrap_or("");
    let (range, total) = spec.split_once('/').unwrap_or((spec, "*"));
    let first = range
        .split('-')
        .next()
        .and_then(|v| v.trim().parse::<u64>().ok());
    if first != Some(start) {
        return Err(ModelError::RangeIgnored);
    }
    if let (Ok(got), Some(want)) = (total.trim().parse::<u64>(), expected_total) {
        if got != want {
            return Err(ModelError::WrongSize { got, want });
        }
    }
    Ok(())
}

/// What [`ModelManager::download_and_verify`] does with a file already at the final path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingFile {
    /// Refuse it: the caller is installing a new file under this name (the Store).
    Refuse,
    /// Finish it: an unverified file is this name's own interrupted download (the Models screen).
    ResumeUnverified,
}

/// HUP-S0.3 — single-flight guard: at most one download per `.part` path in this process.
pub struct DownloadGuard {
    path: PathBuf,
}

fn in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

impl DownloadGuard {
    /// Claim `part`; `Err(AlreadyDownloading)` if another download holds it.
    pub fn acquire(part: &std::path::Path) -> Result<Self> {
        let mut set = in_flight()
            .lock()
            .map_err(|_| ModelError::Io("download registry poisoned".into()))?;
        if !set.insert(part.to_path_buf()) {
            return Err(ModelError::AlreadyDownloading);
        }
        Ok(DownloadGuard {
            path: part.to_path_buf(),
        })
    }
}

/// Whether a download currently holds `part` (the Models screen's delete state).
pub fn download_in_flight(part: &std::path::Path) -> bool {
    in_flight()
        .lock()
        .map(|set| set.contains(part))
        .unwrap_or(true)
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = in_flight().lock() {
            set.remove(&self.path);
        }
        // A multi-source download keeps its session (live facts, a pending pause) across the
        // manager it hands over from; the session's own mark clears them when it ends.
        if !is_marked_busy(&self.path) {
            if let Ok(mut set) = pause_requests().lock() {
                set.remove(&self.path);
            }
            if let Ok(mut map) = live().lock() {
                map.remove(&self.path);
            }
        }
    }
}

fn busy_marks() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, usize>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, usize>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Marks `part`'s file as busy for as long as it lives: a multi-source download between its
/// passes (waiting for the connection), or its verify. Counted, so marks nest.
struct BusyMark(PathBuf);

impl BusyMark {
    fn new(part: PathBuf) -> Self {
        if let Ok(mut m) = busy_marks().lock() {
            *m.entry(part.clone()).or_insert(0) += 1;
        }
        BusyMark(part)
    }
}

impl Drop for BusyMark {
    fn drop(&mut self) {
        if let Ok(mut m) = busy_marks().lock() {
            if let Some(n) = m.get_mut(&self.0) {
                *n -= 1;
                if *n == 0 {
                    m.remove(&self.0);
                    if let Ok(mut l) = live().lock() {
                        l.remove(&self.0);
                    }
                    if let Ok(mut p) = pause_requests().lock() {
                        p.remove(&self.0);
                    }
                }
            }
        }
    }
}

fn is_marked_busy(part: &std::path::Path) -> bool {
    busy_marks()
        .lock()
        .map(|m| m.contains_key(part))
        .unwrap_or(false)
}

/// Whether a download or its verify is running on `part` in this process.
pub fn download_busy(part: &std::path::Path) -> bool {
    download_in_flight(part) || is_marked_busy(part)
}

fn pause_requests() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Ask the download running on `part` to pause: it stops at the next chunk, keeps the `.part`,
/// records itself as paused, and returns [`ModelError::Paused`]. `false` when none is running.
pub fn request_pause(part: &std::path::Path) -> bool {
    if !download_busy(part) {
        return false;
    }
    pause_requests()
        .lock()
        .map(|mut s| s.insert(part.to_path_buf()))
        .is_ok()
}

fn pause_requested(part: &std::path::Path) -> bool {
    pause_requests()
        .lock()
        .map(|s| s.contains(part))
        .unwrap_or(false)
}

/// Sleep `d`, waking early (and returning `false`) when a pause is asked for.
fn sleep_unless_paused(part: &std::path::Path, d: std::time::Duration) -> bool {
    let until = std::time::Instant::now() + d;
    loop {
        if pause_requested(part) {
            return false;
        }
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return true;
        }
        std::thread::sleep(left.min(std::time::Duration::from_millis(200)));
    }
}

/// The live facts of a running download, read by [`ModelManager::status`].
#[derive(Clone, Debug, Default)]
struct Live {
    bytes_per_sec: Option<f64>,
    eta_secs: Option<u64>,
    retry_until: Option<std::time::Instant>,
    last_error: Option<String>,
}

fn live() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, Live>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Live>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

fn set_live_speed(part: &std::path::Path, bytes_per_sec: f64, left: u64) {
    if let Ok(mut map) = live().lock() {
        let e = map.entry(part.to_path_buf()).or_default();
        e.bytes_per_sec = Some(bytes_per_sec);
        e.eta_secs = (bytes_per_sec > 0.0).then(|| (left as f64 / bytes_per_sec).ceil() as u64);
        e.retry_until = None;
        e.last_error = None;
    }
}

fn set_live_retry(part: &std::path::Path, wait: Option<std::time::Duration>, why: Option<String>) {
    if let Ok(mut map) = live().lock() {
        let e = map.entry(part.to_path_buf()).or_default();
        e.retry_until = wait.map(|w| std::time::Instant::now() + w);
        if why.is_some() {
            e.last_error = why;
            e.bytes_per_sec = None;
            e.eta_secs = None;
        }
    }
}

fn live_of(part: &std::path::Path) -> Live {
    live()
        .lock()
        .ok()
        .and_then(|m| m.get(part).cloned())
        .unwrap_or_default()
}

/// Production transport: blocking `ureq` (rustls TLS), the same client ai.rs and
/// rpc.rs use — no new heavy dep. Range requests power the resume.
///
/// HUP-S0.3b: redirects are followed hop by hop by [`crate::hf_auth::fetch`], which attaches the
/// member's Hugging Face token (when one is set via [`Self::with_hf_token`]) ONLY to hops whose
/// origin is exactly `https://huggingface.co` / `https://hf.co`; the CDN hop a resolve redirects
/// to never carries it. Without a token no `Authorization` header is ever sent.
pub struct UreqModelTransport {
    url: String,
    token: Option<crate::hf_auth::HfToken>,
    scope: crate::hf_auth::AuthScope,
    expected_total: Option<u64>,
}

impl UreqModelTransport {
    pub fn new(url: impl Into<String>) -> Self {
        UreqModelTransport {
            url: url.into(),
            token: None,
            scope: crate::hf_auth::AuthScope::huggingface(),
            expected_total: None,
        }
    }

    /// The pinned length: a source whose `Content-Range` names another total is a different file
    /// ([`ModelError::WrongSize`]) and is skipped before any byte of it is kept.
    pub fn with_expected_total(mut self, total: u64) -> Self {
        self.expected_total = Some(total);
        self
    }

    /// HUP-S0.3b: the member's Hugging Face token for gated/private repos (builder).
    pub fn with_hf_token(mut self, token: Option<crate::hf_auth::HfToken>) -> Self {
        self.token = token;
        self
    }

    /// Point the token scope at loopback test servers (tests only; production is always
    /// [`crate::hf_auth::AuthScope::huggingface`]).
    #[cfg(test)]
    pub fn with_auth_scope(mut self, scope: crate::hf_auth::AuthScope) -> Self {
        self.scope = scope;
        self
    }

    fn fetch(
        &self,
        range: &str,
        timeouts: crate::hf_auth::Timeouts,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        crate::hf_auth::fetch(
            &self.url,
            Some(range),
            &self.scope,
            self.token.as_ref(),
            &timeouts,
        )
        .map_err(|e| match e {
            crate::hf_auth::FetchError::Gated { token_sent } => ModelError::Gated { token_sent },
            // A missing object is not a network failure: this source does not have the file.
            // (A 401/403/410 from a CDN hop, an expired signed URL, stays a Transport error and is
            // retried, which starts again from the origin URL with a fresh redirect.)
            crate::hf_auth::FetchError::Status(404) => ModelError::NotServed(404),
            other => ModelError::Transport(other.to_string()),
        })
    }
}

impl ModelTransport for UreqModelTransport {
    fn total_size(&self) -> Result<u64> {
        // A ranged GET of the first byte returns Content-Range with the total; a
        // plain HEAD can be blocked by some CDNs, so we ask for bytes=0-0 and read
        // the total from Content-Range, falling back to Content-Length.
        // Bounded timeouts (CX-S1): a HF resolve now 302-redirects to a signed Xet CDN URL
        // (us.aws.cdn.hf.co). Without a connect/response timeout, a CDN that accepts the socket
        // but never answers hangs the whole download FOREVER — the reported "downloads time out /
        // never finish, logged as unfinished". This is a header-only probe, so a short global cap
        // is safe and turns a hang into an honest, retryable error (Rule 1).
        let resp = self.fetch(
            "bytes=0-0",
            crate::hf_auth::Timeouts {
                global: Some(std::time::Duration::from_secs(45)),
                ..Default::default()
            },
        )?;
        if let Some(cr) = resp
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
        {
            // Content-Range: bytes 0-0/<total>
            if let Some(total) = cr
                .rsplit('/')
                .next()
                .and_then(|s| s.trim().parse::<u64>().ok())
            {
                return Ok(total);
            }
        }
        if let Some(cl) = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
        {
            return Ok(cl);
        }
        Err(ModelError::Transport("no content length".into()))
    }

    fn get_from(&self, offset: u64) -> Result<Box<dyn Read + Send>> {
        let range = format!("bytes={offset}-");
        // Bound the CONNECT + response-header phase only — NOT the body. A multi-GB stream over a
        // slow link can legitimately take many minutes, so a global/body cap would kill a healthy
        // download; but a Xet CDN that stalls before sending headers (the observed hang) must fail
        // fast into a retryable error instead of blocking the download thread forever.
        let resp = self.fetch(
            &range,
            crate::hf_auth::Timeouts {
                connect: Some(std::time::Duration::from_secs(30)),
                recv_response: Some(std::time::Duration::from_secs(90)),
                global: None,
            },
        )?;
        Ok(Box::new(resp.into_body().into_reader()))
    }

    fn get_range(&self, start: u64, end: u64) -> Result<Box<dyn Read + Send>> {
        let range = format!("bytes={start}-{}", end.saturating_sub(1));
        // Every call starts again from `self.url` (the unsigned origin) and follows the redirects
        // fresh, so a signed CDN URL is used for one segment at most and never stored. The stall
        // guard lives in the download loop; SEGMENT_TIMEOUT is only a backstop for a dead socket.
        let resp = self.fetch(
            &range,
            crate::hf_auth::Timeouts {
                connect: Some(std::time::Duration::from_secs(30)),
                recv_response: Some(std::time::Duration::from_secs(60)),
                global: Some(SEGMENT_TIMEOUT),
            },
        )?;
        let status = resp.status().as_u16();
        check_range_status(start, status)?;
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        check_content_range(
            start,
            status,
            header("content-range").as_deref(),
            header("content-length").and_then(|v| v.parse().ok()),
            self.expected_total,
        )?;
        Ok(Box::new(resp.into_body().into_reader()))
    }
}

// ---------------------------------------------------------------------------
// The model manager — owns the app-data model dir + the injected transport + the
// pinned expected hash/size. Production pins the real Gemma hash/size; tests pin
// a fixture hash so the verify LOGIC is proven without the real bytes.
// ---------------------------------------------------------------------------

/// How many bytes to stream per read while downloading (a bounded buffer, not the
/// whole 5 GB in memory).
const DOWNLOAD_CHUNK: usize = 1 << 20; // 1 MiB

/// The process-wide model manager. Holds the model dir, the HTTP transport, and
/// the pinned expected hash + size. Managed as Tauri state; `status`/`download`/
/// `verify` are called from the invoke commands.
pub struct ModelManager {
    /// The app-data `models/` dir (the final file + `.part` + status side-file).
    dir: PathBuf,
    /// The HTTP transport (production `ureq`; tests inject a fixture). Shared so an attempt's
    /// reader thread can own it while the download loop watches for a stall or a pause.
    transport: std::sync::Arc<dyn ModelTransport>,
    /// The pinned expected sha256 hex (real Gemma in prod; fixture in tests).
    expected_hash: String,
    /// The pinned expected byte length.
    expected_size: u64,
    /// The model's on-disk filename. Defaults to [`MODEL_FILE`] (the bundled Gemma) for
    /// backward compatibility; the catalog (CX-S1) overrides it per model via [`with_file`]
    /// / [`from_descriptor`], so multiple models coexist in `models/` by filename. The
    /// `.part` and `.status.json` side-files are DERIVED from it.
    file: String,
    /// HUP-S0.3: bytes per ranged segment ([`SEGMENT_BYTES`]; tests shrink it).
    segment_bytes: u64,
    /// HUP-S0.3: backoff before retry `n` ([`default_retry_backoff`]; tests use zero).
    retry_backoff: fn(u32) -> std::time::Duration,
    /// No byte for this long abandons an attempt ([`STALL_TIMEOUT`]).
    stall_timeout: std::time::Duration,
    /// Free bytes on the disk holding a path ([`disk_free`]; tests inject one).
    free_space: fn(&std::path::Path) -> Option<u64>,
    /// The unsigned source URLs recorded in the `.part` sidecar.
    source_labels: Vec<String>,
}

impl ModelManager {
    /// Build a manager over an explicit model dir + transport + pinned hash/size. The model
    /// filename defaults to [`MODEL_FILE`] (Gemma) — use [`with_file`] for a catalog model.
    /// Production uses [`build_model_state`] (real Gemma pins); tests inject a fixture.
    pub fn new(
        dir: PathBuf,
        transport: Box<dyn ModelTransport>,
        expected_hash: String,
        expected_size: u64,
    ) -> Self {
        Self::with_shared_transport(
            dir,
            std::sync::Arc::from(transport),
            expected_hash,
            expected_size,
        )
    }

    /// [`Self::new`] over a transport shared with other managers (one per source pass).
    pub fn with_shared_transport(
        dir: PathBuf,
        transport: std::sync::Arc<dyn ModelTransport>,
        expected_hash: String,
        expected_size: u64,
    ) -> Self {
        ModelManager {
            dir,
            transport,
            expected_hash,
            expected_size,
            file: MODEL_FILE.to_string(),
            segment_bytes: SEGMENT_BYTES,
            retry_backoff: default_retry_backoff,
            stall_timeout: STALL_TIMEOUT,
            free_space: disk_free,
            source_labels: Vec::new(),
        }
    }

    /// Override the stall timeout (builder; tests use a short one).
    pub fn with_stall_timeout(mut self, d: std::time::Duration) -> Self {
        self.stall_timeout = d.max(std::time::Duration::from_millis(10));
        self
    }

    /// Override the free-space probe (builder; tests inject a full or a roomy disk).
    pub fn with_free_space_probe(mut self, f: fn(&std::path::Path) -> Option<u64>) -> Self {
        self.free_space = f;
        self
    }

    /// The unsigned source URLs to record in the `.part` sidecar (builder).
    pub fn with_source_labels(mut self, labels: Vec<String>) -> Self {
        self.source_labels = labels;
        self
    }

    /// Override the ranged-segment size (builder; tests use tiny segments).
    pub fn with_segment_bytes(mut self, n: u64) -> Self {
        self.segment_bytes = n.max(1);
        self
    }

    /// Override the retry backoff (builder; tests use zero).
    pub fn with_retry_backoff(mut self, f: fn(u32) -> std::time::Duration) -> Self {
        self.retry_backoff = f;
        self
    }

    /// Override the model filename (builder) — the catalog sets this per model so several
    /// models live in the same `models/` dir. `.part`/`.status.json` derive from it.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Build a manager for a specific [`ModelDescriptor`] (CX-S1 catalog): its pinned
    /// hash/size/filename over the given dir + transport.
    pub fn from_descriptor(
        dir: PathBuf,
        transport: Box<dyn ModelTransport>,
        desc: &ModelDescriptor,
    ) -> Self {
        // PBA-L7b-013: the local name is the file's final segment (HF sub-directory GGUFs).
        ModelManager::new(dir, transport, desc.sha256.clone(), desc.size_bytes)
            .with_file(crate::model_catalog::local_file_name(&desc.file))
    }

    /// The final model file path (`<dir>/<file>`).
    fn final_path(&self) -> PathBuf {
        self.dir.join(&self.file)
    }

    /// The in-progress partial path (`<dir>/<file>.part`).
    fn part_path(&self) -> PathBuf {
        self.dir.join(format!("{}.part", self.file))
    }

    /// The status side-file path (`<dir>/<file>.status.json`).
    fn status_path(&self) -> PathBuf {
        self.dir.join(format!("{}.status.json", self.file))
    }

    /// Read the recorded verified flag (default false / absent).
    fn read_verified(&self) -> bool {
        std::fs::read(self.status_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<StatusFile>(&b).ok())
            .map(|s| s.verified)
            .unwrap_or(false)
    }

    /// Write the verified flag (the ONLY writer sets it true from `verify`).
    fn write_verified(&self, verified: bool) -> Result<()> {
        let blob = serde_json::to_vec(&StatusFile {
            verified,
            sha256: verified.then(|| self.expected_hash.clone()),
        })
        .map_err(|e| ModelError::Io(e.to_string()))?;
        std::fs::write(self.status_path(), blob).map_err(|e| ModelError::Io(e.to_string()))
    }

    /// The honest, file-derived status. `Ready` requires BOTH the final file
    /// present AND the recorded verified flag (Rule 1 — presence never earns
    /// Ready). A `.part` yields `Downloading` from its current size.
    pub fn status(&self) -> ModelStatus {
        let final_path = self.final_path();
        if final_path.exists() {
            if self.read_verified() {
                return ModelStatus::Ready;
            }
            // Present but not verified: being verified right now (the download that holds this
            // file's lock is hashing it), or a complete download whose verify has not been earned
            // yet, reported as 100% downloaded so the UI can prompt "verify".
            if download_busy(&self.part_path()) {
                return ModelStatus::Verifying;
            }
            let size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
            return ModelStatus::Downloading {
                downloaded_bytes: size,
                total_bytes: self.expected_size,
                pct: 100.0,
                bytes_per_sec: None,
                eta_secs: None,
                retry_in_secs: None,
                last_error: None,
            };
        }
        let part = self.part_path();
        if part.exists() {
            let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            let total = self.expected_size.max(1);
            let pct = ((got as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
            let running = download_busy(&part);
            if !running && read_part_meta(&self.dir, &self.file).is_some_and(|m| m.paused) {
                return ModelStatus::Paused {
                    downloaded_bytes: got,
                    total_bytes: self.expected_size,
                    pct,
                };
            }
            let l = if running {
                live_of(&part)
            } else {
                Live::default()
            };
            let retry_in_secs = l.retry_until.map(|t| {
                t.saturating_duration_since(std::time::Instant::now())
                    .as_secs()
            });
            return ModelStatus::Downloading {
                downloaded_bytes: got,
                total_bytes: self.expected_size,
                pct,
                bytes_per_sec: l.bytes_per_sec,
                eta_secs: l.eta_secs,
                retry_in_secs,
                last_error: l.last_error,
            };
        }
        ModelStatus::NotPresent
    }

    /// STREAMED, resumable download. Writes to `.part`, resumes from its current
    /// size via a `Range` request, checks the GGUF magic on the first bytes, and
    /// renames `.part` → final only once exactly [`Self::expected_size`] bytes
    /// arrived. Returns the list of requested Range offsets (so a test proves a
    /// resume continued from the partial size rather than restarting).
    ///
    /// A fresh download starts a NEW `.part` (any bad-magic body aborts before a
    /// final file exists). A resume of an existing `.part` continues its bytes.
    /// Download (unless a complete file is already here) and verify, as one operation on this
    /// file name. `existing` says what a file already at the final path means.
    ///
    /// The whole operation holds this file's [`DownloadGuard`] (the Store and the Models screen
    /// share it), so no other download can create or replace the file between the check, the
    /// download and the verify. A file already here that is verified against ANOTHER hash is
    /// never re-verified against this one (that would quarantine a different model): it is
    /// refused with [`ModelError::Occupied`].
    ///
    /// `on_claimed` runs once the lock is held, before anything else: a caller's record of this
    /// download (the Models screen's resume marker) is made only by the download that owns the
    /// file, never by one the lock refused.
    pub fn download_and_verify(
        &self,
        existing: ExistingFile,
        on_claimed: impl FnOnce() -> Result<()>,
        mut on_progress: impl FnMut(u64, u64),
    ) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| ModelError::Io(e.to_string()))?;
        let _guard = DownloadGuard::acquire(&self.part_path())?;
        on_claimed()?;
        if self.final_path().exists() {
            if existing == ExistingFile::Refuse {
                return Err(ModelError::Occupied);
            }
            if self.read_verified() {
                // Verified already: this model when the recorded hash is ours, another one when
                // it is not (or none is recorded and the bytes differ). Never quarantined here.
                let ours = match verified_sha256(&self.dir, &self.file) {
                    Some(h) => h.eq_ignore_ascii_case(&self.expected_hash),
                    None => self.matches_pinned()?,
                };
                if !ours {
                    return Err(ModelError::Occupied);
                }
                on_progress(self.expected_size, self.expected_size);
                return self.write_verified(true);
            }
            // An unverified file under the lock is this name's own interrupted download.
            on_progress(self.expected_size, self.expected_size);
            return self.verify();
        }
        self.download_locked(&mut on_progress)?;
        self.verify()
    }

    /// [`Self::verify`] while the caller holds this file's [`DownloadGuard`] (the one the Store and
    /// the Models screen share), so no download can rename other bytes in under the verify. A
    /// guard for another file is refused.
    pub fn verify_held(&self, guard: &DownloadGuard) -> Result<()> {
        if guard.path != self.part_path() {
            return Err(ModelError::Io(
                "the download lock held is not this file's".into(),
            ));
        }
        self.verify()
    }

    /// Confirm a file that may not be this manager's, while the caller holds this file's
    /// [`DownloadGuard`]: when it has the pinned size and SHA-256 the verified flag is recorded
    /// (`Ok(true)`); any other file is left exactly as it is (`Ok(false)`), never quarantined.
    /// A guard for another file is refused.
    pub fn confirm_held(&self, guard: &DownloadGuard) -> Result<bool> {
        if guard.path != self.part_path() {
            return Err(ModelError::Io(
                "the download lock held is not this file's".into(),
            ));
        }
        if !self.matches_pinned()? {
            return Ok(false);
        }
        self.write_verified(true)?;
        Ok(true)
    }

    /// Whether the final file has the pinned size and SHA-256 (no side effects).
    fn matches_pinned(&self) -> Result<bool> {
        use sha2::{Digest, Sha256};
        let path = self.final_path();
        let size = std::fs::metadata(&path)
            .map_err(|e| ModelError::Io(e.to_string()))?
            .len();
        if size != self.expected_size {
            return Ok(false);
        }
        let mut file = std::fs::File::open(&path).map_err(|e| ModelError::Io(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; DOWNLOAD_CHUNK];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| ModelError::Io(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hex::encode(hasher.finalize()) == self.expected_hash)
    }

    pub fn download(&self) -> Result<Vec<u64>> {
        self.download_with_progress(|_, _| {})
    }

    /// Like [`Self::download`] but reports byte progress via `on_progress(downloaded, total)`
    /// after each 1 MiB chunk (and once at the resume offset). A multi-GB catalog download can
    /// then drive a real progress bar instead of a silent, multi-minute block that looks stuck.
    pub fn download_with_progress(
        &self,
        mut on_progress: impl FnMut(u64, u64),
    ) -> Result<Vec<u64>> {
        std::fs::create_dir_all(&self.dir).map_err(|e| ModelError::Io(e.to_string()))?;
        let part = self.part_path();

        // HUP-S0.3: a complete file already on disk is never downloaded again (an app quit between
        // the rename and the verify used to restart a multi-GB pull from byte 0). The caller runs
        // `verify`, which quarantines it if it is wrong.
        if self.final_path().exists() {
            on_progress(self.expected_size, self.expected_size);
            return Ok(Vec::new());
        }
        // HUP-S0.3: exactly one writer per `.part` (a second appender corrupts it).
        let _guard = DownloadGuard::acquire(&part)?;
        self.download_locked(&mut on_progress)
    }

    /// The download itself, run while the caller holds this file's [`DownloadGuard`].
    ///
    /// - The `.part` is resumed only when its sidecar (`<file>.part.json`) names the same pinned
    ///   length and SHA-256; a `.part` left for another pin is discarded and the download starts
    ///   at byte 0. A `.part` from an older app (no sidecar) is resumed; the final verify covers it.
    /// - The free disk space must hold what is left plus [`DISK_MARGIN_BYTES`] before any request.
    /// - Each attempt asks the transport for one segment from the exact byte on disk. Its reader runs
    ///   on its own thread, so the loop notices a pause at once and abandons an attempt that has
    ///   received nothing for [`Self::stall_timeout`]. A download that keeps moving is never cut.
    /// - Network failures are retried with backoff from the exact byte on disk; any progress
    ///   resets the count.
    fn download_locked(&self, on_progress: &mut dyn FnMut(u64, u64)) -> Result<Vec<u64>> {
        use std::io::Write;
        let part = self.part_path();

        // The sidecar decides whether an existing `.part` belongs to this pin.
        let meta = read_part_meta(&self.dir, &self.file);
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let foreign = meta
            .as_ref()
            .is_some_and(|m| m.size != self.expected_size || m.sha256 != self.expected_hash);
        // Never resume past what we expect (a corrupt oversized part → restart), nor onto another
        // pinned file.
        if have >= self.expected_size || foreign {
            if have > 0 || foreign {
                crate::model_log::event(&format!(
                    "{}: discarding a {have}-byte partial ({})",
                    self.file,
                    if foreign {
                        "left for another pinned file"
                    } else {
                        "longer than the file"
                    }
                ));
            }
            have = 0;
            let _ = std::fs::remove_file(&part);
        }

        // Disk space for what is left (plus a margin), checked before any byte is requested.
        let left = self.expected_size - have;
        let free = (self.free_space)(&self.dir);
        if let Some(free) = free {
            let need = left.saturating_add(DISK_MARGIN_BYTES);
            if free < need {
                crate::model_log::event(&format!(
                    "{}: not enough disk space: need {need} bytes, {free} free",
                    self.file
                ));
                return Err(ModelError::DiskFull { need, free });
            }
        }

        let mut meta = PartMeta::new(
            &self.file,
            self.expected_size,
            self.expected_hash.clone(),
            self.source_labels.clone(),
        );
        write_part_meta(&self.dir, &meta)?;

        on_progress(have, self.expected_size); // initial (resume offset, or 0)
        crate::model_log::event(&format!(
            "{}: download {} at byte {have} of {}",
            self.file,
            if have > 0 { "resumes" } else { "starts" },
            self.expected_size
        ));
        let mut offsets = Vec::new();

        // Open the part file for append (resume) or create (fresh).
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .map_err(|e| io_to_model_error(e, left, free))?;

        // On a FRESH download (have == 0) the first 4 bytes must be the GGUF magic.
        // We buffer the first bytes to check before committing them.
        let mut written = have;
        let mut magic_checked = have >= GGUF_MAGIC.len() as u64;
        let mut magic_buf: Vec<u8> = Vec::with_capacity(GGUF_MAGIC.len());
        // HUP-S0.3: consecutive transport failures; any segment that makes progress resets it.
        let mut failures: u32 = 0;
        let mut speed = Speed::new(written);

        'segments: while written < self.expected_size {
            if pause_requested(&part) {
                return Err(self.take_pause(&mut file, &mut meta, written));
            }
            let seg_end = written
                .saturating_add(self.segment_bytes)
                .min(self.expected_size);
            offsets.push(written);
            let before = written;
            let mut transient: Option<ModelError> = None;
            let rx = spawn_attempt(self.transport.clone(), written, seg_end);
            let mut last_activity = std::time::Instant::now();
            loop {
                if pause_requested(&part) {
                    return Err(self.take_pause(&mut file, &mut meta, written));
                }
                let wait = self
                    .stall_timeout
                    .min(std::time::Duration::from_millis(200));
                let msg = match rx.recv_timeout(wait) {
                    Ok(m) => m,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if last_activity.elapsed() >= self.stall_timeout {
                            transient = Some(ModelError::Stalled {
                                secs: self.stall_timeout.as_secs(),
                            });
                            break;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        transient = Some(ModelError::Transport("the connection closed".into()));
                        break;
                    }
                };
                last_activity = std::time::Instant::now();
                let data = match msg {
                    AttemptMsg::Opened => continue,
                    AttemptMsg::OpenFailed(ModelError::Transport(m)) => {
                        transient = Some(ModelError::Transport(m));
                        break;
                    }
                    // A policy failure (range ignored, gated, missing, another file) is not retried.
                    AttemptMsg::OpenFailed(e) => return Err(e),
                    AttemptMsg::ReadFailed(e) => {
                        transient = Some(ModelError::Transport(e));
                        break;
                    }
                    AttemptMsg::Eof => break,
                    AttemptMsg::Data(d) => d,
                };
                let mut chunk = &data[..];

                // Magic gate on a fresh download: accumulate the first 4 bytes and
                // check them BEFORE any are persisted, so a wrong body never lands.
                if !magic_checked {
                    let need = GGUF_MAGIC.len() - magic_buf.len();
                    let take = need.min(chunk.len());
                    magic_buf.extend_from_slice(&chunk[..take]);
                    chunk = &chunk[take..];
                    if magic_buf.len() == GGUF_MAGIC.len() {
                        if magic_buf.as_slice() != GGUF_MAGIC {
                            // Abort: remove the (empty) part file; never finalize.
                            drop(file);
                            let _ = std::fs::remove_file(&part);
                            remove_part_meta(&self.dir, &self.file);
                            return Err(ModelError::BadMagic);
                        }
                        // Magic OK — persist it, then continue with the rest.
                        file.write_all(&magic_buf).map_err(|e| {
                            io_to_model_error(
                                e,
                                self.expected_size - written,
                                (self.free_space)(&self.dir),
                            )
                        })?;
                        written += magic_buf.len() as u64;
                        magic_checked = true;
                    }
                }
                if magic_checked && !chunk.is_empty() {
                    file.write_all(chunk).map_err(|e| {
                        let _ = file.flush();
                        io_to_model_error(
                            e,
                            self.expected_size - written,
                            (self.free_space)(&self.dir),
                        )
                    })?;
                    written += chunk.len() as u64;
                    // Overshoot guard: a server that IGNORES `Range` re-sends the full
                    // body onto a resume, driving `written` past the pinned length.
                    // Reset the corrupt oversized `.part` and fail closed — never carry
                    // an over-length partial forward, never finalize it.
                    if written > self.expected_size {
                        drop(file);
                        let _ = std::fs::remove_file(&part);
                        remove_part_meta(&self.dir, &self.file);
                        return Err(ModelError::Overshoot {
                            got: written,
                            want: self.expected_size,
                        });
                    }
                }
                if let Some(bps) = speed.sample(written) {
                    set_live_speed(&part, bps, self.expected_size - written);
                }
                on_progress(written, self.expected_size);
            }
            if let Some(err) = transient {
                // HUP-S0.3: resume from the exact byte written, with bounded, backed-off retries.
                if written > before {
                    failures = 0;
                }
                failures += 1;
                if failures > MAX_SEGMENT_RETRIES {
                    file.flush().map_err(|e| io_to_model_error(e, 0, None))?;
                    return Err(err);
                }
                let wait = (self.retry_backoff)(failures);
                crate::model_log::event(&format!(
                    "{}: attempt failed at byte {written} ({err}); retry {failures} of {MAX_SEGMENT_RETRIES} in {}s",
                    self.file,
                    wait.as_secs()
                ));
                set_live_retry(&part, Some(wait), Some(err.user_message()));
                // A pause during the wait is taken at the top of the next attempt.
                let _ = sleep_unless_paused(&part, wait);
                set_live_retry(&part, None, None);
                continue 'segments;
            }
            if written >= seg_end {
                failures = 0;
                continue 'segments;
            }
            // A clean end-of-stream short of the segment: stop and report it below (the
            // `.part` stays resumable).
            break 'segments;
        }
        file.flush().map_err(|e| io_to_model_error(e, 0, None))?;
        drop(file);

        // The stream must have delivered the full pinned length. A shortfall is a
        // failed/truncated download — keep the `.part` (resumable next time) but
        // NEVER finalize (fail closed, never Ready). (An overshoot was already
        // caught + reset in the loop above, so `written` here is <= expected.)
        if written != self.expected_size {
            return Err(ModelError::ShortStream {
                got: written,
                want: self.expected_size,
            });
        }

        // Full length arrived: atomically rename `.part` → final. Any prior
        // verified flag is cleared (a new file must re-earn Ready).
        std::fs::rename(&part, self.final_path()).map_err(|e| ModelError::Io(e.to_string()))?;
        remove_part_meta(&self.dir, &self.file);
        let _ = self.write_verified(false);
        crate::model_log::event(&format!("{}: all {written} bytes arrived", self.file));
        Ok(offsets)
    }

    /// Stop for a pause: flush the `.part`, record the pause in its sidecar (so a restart does not
    /// resume it on its own), and return [`ModelError::Paused`].
    fn take_pause(
        &self,
        file: &mut std::fs::File,
        meta: &mut PartMeta,
        written: u64,
    ) -> ModelError {
        use std::io::Write;
        let _ = file.flush();
        meta.paused = true;
        let _ = write_part_meta(&self.dir, meta);
        crate::model_log::event(&format!("{}: paused at byte {written}", self.file));
        ModelError::Paused
    }

    /// Stream the final file through SHA-256 and compare to the pinned hash. On a
    /// size or checksum mismatch the file is QUARANTINED (removed) and an error is
    /// returned — NEVER marked Ready. On a match, the verified flag is written and
    /// the next `status()` reports `Ready`.
    pub fn verify(&self) -> Result<()> {
        use sha2::{Digest, Sha256};
        let path = self.final_path();
        if !path.exists() {
            return Err(ModelError::NotPresent);
        }
        // Belt-and-suspenders: the on-disk length must match the pinned size even
        // if a caller bypassed the streaming length check.
        let size = std::fs::metadata(&path)
            .map_err(|e| ModelError::Io(e.to_string()))?
            .len();
        if size != self.expected_size {
            self.quarantine();
            return Err(ModelError::SizeMismatch {
                got: size,
                want: self.expected_size,
            });
        }

        let mut file = std::fs::File::open(&path).map_err(|e| ModelError::Io(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; DOWNLOAD_CHUNK];
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| ModelError::Io(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let got = hex::encode(hasher.finalize());
        // Constant-time-ish compare is unnecessary (a public hash), but a plain
        // eq on the lowercase hex is the correct check.
        if got != self.expected_hash {
            self.quarantine();
            return Err(ModelError::ChecksumMismatch);
        }
        // Earned Ready: record the verified flag.
        self.write_verified(true)
    }

    /// Remove the final file + clear the verified flag so a mismatched/corrupt
    /// model can NEVER be served or reported Ready.
    fn quarantine(&self) {
        let _ = std::fs::remove_file(self.final_path());
        let _ = self.write_verified(false);
    }

    /// Whether the model is verified-Ready (the BC-3.2 serve gate reads this).
    pub fn is_ready(&self) -> bool {
        matches!(self.status(), ModelStatus::Ready)
    }

    /// The verified-Ready model path (for the llama-server `-m` arg), if Ready.
    pub fn ready_path(&self) -> Option<PathBuf> {
        if self.is_ready() {
            Some(self.final_path())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// One download attempt runs its request and body reads on its own thread, so the download loop
// can watch the clock (stall) and the pause flag while a read blocks.
// ---------------------------------------------------------------------------

enum AttemptMsg {
    /// The response headers arrived and passed the range checks.
    Opened,
    /// The request failed before any body byte.
    OpenFailed(ModelError),
    Data(Vec<u8>),
    ReadFailed(String),
    Eof,
}

fn spawn_attempt(
    transport: std::sync::Arc<dyn ModelTransport>,
    start: u64,
    end: u64,
) -> std::sync::mpsc::Receiver<AttemptMsg> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<AttemptMsg>(4);
    // If the thread cannot start, `tx` is dropped and the loop sees a closed connection (retried).
    let _ = std::thread::Builder::new()
        .name("model-download".into())
        .spawn(move || {
            let mut reader = match transport.get_range(start, end) {
                Ok(r) => {
                    if tx.send(AttemptMsg::Opened).is_err() {
                        return;
                    }
                    r
                }
                Err(e) => {
                    let _ = tx.send(AttemptMsg::OpenFailed(e));
                    return;
                }
            };
            let mut buf = vec![0u8; DOWNLOAD_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(AttemptMsg::Eof);
                        return;
                    }
                    Ok(n) => {
                        // A closed receiver means the attempt was abandoned (stall or pause).
                        if tx.send(AttemptMsg::Data(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        let _ = tx.send(AttemptMsg::ReadFailed(e.to_string()));
                        return;
                    }
                }
            }
        });
    rx
}

/// Download speed, smoothed over half-second windows.
struct Speed {
    since: std::time::Instant,
    at: u64,
    ewma: Option<f64>,
}

impl Speed {
    fn new(at: u64) -> Self {
        Speed {
            since: std::time::Instant::now(),
            at,
            ewma: None,
        }
    }

    /// A new bytes-per-second figure once a window has passed.
    fn sample(&mut self, written: u64) -> Option<f64> {
        let dt = self.since.elapsed().as_secs_f64();
        if dt < 0.5 {
            return None;
        }
        let inst = written.saturating_sub(self.at) as f64 / dt;
        let next = match self.ewma {
            None => inst,
            Some(e) => 0.3 * inst + 0.7 * e,
        };
        self.ewma = Some(next);
        self.since = std::time::Instant::now();
        self.at = written;
        Some(next)
    }
}

// ---------------------------------------------------------------------------
// The `.part` sidecar: `<file>.part.json` records which pinned file a `.part` belongs to (length,
// SHA-256) and the UNSIGNED source URLs, so a resume after a restart only continues the same file.
// It never holds a redirect target: a signed CDN URL expires and is re-resolved on every attempt.
// ---------------------------------------------------------------------------

/// What a `.part` is a partial of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartMeta {
    pub v: u32,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub sources: Vec<String>,
    /// The member paused it: a restart leaves it paused instead of resuming on its own.
    #[serde(default)]
    pub paused: bool,
}

impl PartMeta {
    pub fn new(file: &str, size: u64, sha256: String, sources: Vec<String>) -> Self {
        PartMeta {
            v: 1,
            file: file.to_string(),
            size,
            sha256,
            // Only the scheme, host and path of a source are kept (no query, so never a signature).
            sources: sources
                .iter()
                .map(|u| u.split(['?', '#']).next().unwrap_or("").to_string())
                .collect(),
            paused: false,
        }
    }
}

fn part_meta_path(dir: &std::path::Path, file: &str) -> PathBuf {
    dir.join(format!("{file}.part.json"))
}

/// The sidecar of `dir/<file>.part`, when there is a readable one.
pub fn read_part_meta(dir: &std::path::Path, file: &str) -> Option<PartMeta> {
    std::fs::read(part_meta_path(dir, file))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// Write the sidecar of `dir/<meta.file>.part` (atomically, through a unique temporary file).
pub fn write_part_meta(dir: &std::path::Path, meta: &PartMeta) -> Result<()> {
    let blob = serde_json::to_vec(meta).map_err(|e| ModelError::Io(e.to_string()))?;
    crate::atomic_file::replace(&part_meta_path(dir, &meta.file), &blob)
        .map_err(|e| ModelError::Io(e.to_string()))
}

/// Remove the sidecar of `dir/<file>.part` (the `.part` itself is gone or finished).
pub fn remove_part_meta(dir: &std::path::Path, file: &str) {
    let _ = std::fs::remove_file(part_meta_path(dir, file));
}

/// Mark a stopped download of `dir/<file>` as paused (the member paused it while no download was
/// running, e.g. while it waited to start). Nothing to do without a `.part`.
pub fn mark_paused(dir: &std::path::Path, file: &str, size: u64, sha256: &str) -> Result<()> {
    if !dir.join(format!("{file}.part")).exists() {
        return Ok(());
    }
    let mut meta = read_part_meta(dir, file)
        .unwrap_or_else(|| PartMeta::new(file, size, sha256.to_string(), Vec::new()));
    meta.paused = true;
    write_part_meta(dir, &meta)
}

/// Free bytes on the disk holding `dir` (`None` when unknown; the check is then skipped).
pub fn disk_free(dir: &std::path::Path) -> Option<u64> {
    crate::tier::disk_free(dir)
}

/// Managed Tauri state: the onboarding model the member chose ([`LocalModel`], persisted in the
/// models folder) over the app-data `models/` dir. Status, the Ready gate and the serve path all
/// read the CHOSEN model's file and pins, so a member who picked the smaller model gets it served
/// on every launch.
pub struct ModelState {
    dir: PathBuf,
    choice: std::sync::Mutex<LocalModel>,
}

impl ModelState {
    /// The state over `dir`, with the choice recorded there.
    pub fn new(dir: PathBuf) -> Self {
        let choice = read_local_choice(&dir);
        ModelState {
            dir,
            choice: std::sync::Mutex::new(choice),
        }
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn choice(&self) -> LocalModel {
        self.choice.lock().map(|c| *c).unwrap_or_default()
    }

    /// Record a new choice (persisted first, so a failure changes nothing).
    pub fn set_choice(&self, m: LocalModel) -> Result<()> {
        write_local_choice(&self.dir, m)?;
        if let Ok(mut c) = self.choice.lock() {
            *c = m;
        }
        Ok(())
    }

    /// A manager for the chosen model (status / verify; it never downloads with this transport).
    pub fn manager(&self) -> ModelManager {
        manager_for(&self.dir, self.choice())
    }

    pub fn status(&self) -> ModelStatus {
        self.manager().status()
    }

    pub fn is_ready(&self) -> bool {
        self.manager().is_ready()
    }

    pub fn ready_path(&self) -> Option<PathBuf> {
        self.manager().ready_path()
    }
}

/// A status/verify manager for pinned model `m` in `dir`. Its transport is never used: downloads
/// go through [`download_with`] with real sources.
fn manager_for(dir: &std::path::Path, m: LocalModel) -> ModelManager {
    let pin = m.pin();
    ModelManager::new(
        dir.to_path_buf(),
        Box::new(UreqModelTransport::new(String::new())),
        pin.sha256.to_string(),
        pin.size_bytes,
    )
    .with_file(pin.file)
}

/// Build the managed model state from a live app handle: the app-data `models/` dir and the
/// recorded onboarding choice. Also opens the download log under the app log dir.
pub fn build_model_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<ModelState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = data_root.join("models");
    if let Ok(log_dir) = app.path().app_log_dir() {
        crate::model_log::init(&log_dir);
    }
    let state = ModelState::new(dir.clone());
    // First-run SEED: if the default model isn't already verified-Ready and a copy is BUNDLED
    // in the app resources (`resources/models/<MODEL_FILE>`), seed it locally instead
    // of a network download — so a fresh install has chat out-of-the-box (offline).
    if !manager_for(&dir, LocalModel::Default).is_ready() {
        if let Ok(res) = app.path().resource_dir() {
            let bundled = res.join("models").join(MODEL_FILE);
            if bundled.exists() {
                seed_from_bundle(dir, bundled);
            }
        }
    }
    Ok(state)
}

/// Seed the app-data model from a BUNDLED copy on a background thread: copy the
/// resource GGUF into `dir/<MODEL_FILE>` (if absent), then run the REAL `verify()`
/// (size + SHA-256) so `Ready` is still EARNED, never fabricated (Rule 1). Off the
/// main thread because a ~4.6 GB copy + hash must not block app startup. On any
/// error the model simply stays not-Ready and the normal download path still works.
fn seed_from_bundle(dir: PathBuf, bundled: PathBuf) {
    std::thread::spawn(move || {
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let final_path = dir.join(MODEL_FILE);
        if !final_path.exists() && std::fs::copy(&bundled, &final_path).is_err() {
            return;
        }
        // Earn Ready via the real verify (transport unused by `verify()`).
        let _ = manager_for(&dir, LocalModel::Default).verify();
    });
}

// ---------------------------------------------------------------------------
// Tauri commands — the `model` bridge surface. Return status / () only; no
// secrets, no key material (Rule 1: downloading/ready reflect real bytes+hash).
// ---------------------------------------------------------------------------

use tauri::State;

/// **Command — model_status.** The honest, file-derived state of the CHOSEN onboarding model.
/// `Ready` only when a real verify matched the pinned hash (never mere presence).
#[tauri::command]
pub async fn model_status(app_h: tauri::AppHandle) -> std::result::Result<ModelStatus, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<ModelState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        model_status_sync(st0)
    })
    .await
}

/// Blocking body of [`model_status`]; reached only through [`crate::blocking::off_main`].
pub fn model_status_sync(state: State<'_, ModelState>) -> std::result::Result<ModelStatus, String> {
    Ok(state.status())
}

/// The sources for pinned model `m`: `CITRATE_MODEL_URL` replaces the default model's list (the
/// config seam); the smaller model always uses its own list.
fn sources_for(m: LocalModel) -> Vec<String> {
    match m {
        LocalModel::Default => model_source_urls(std::env::var(MODEL_URL_ENV).ok()),
        LocalModel::Small => SMALL_PIN.source_urls(),
    }
}

/// The last error of a download of `part` that ended, for a second caller that waited on it.
fn last_errors() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, String>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, String>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Download and verify pinned model `m` into `dir`, blocking. A second call while one runs WAITS
/// for it and returns its outcome (so the UI's Retry or a relaunch never shows a false error while
/// the first download keeps going). Errors are [`ModelError::user_message`] text.
pub fn download_pinned_blocking(
    dir: &std::path::Path,
    m: LocalModel,
) -> std::result::Result<(), String> {
    let pin = m.pin();
    let part = dir.join(format!("{}.part", pin.file));
    if download_busy(&part) {
        crate::model_log::event(&format!(
            "{}: a download is already running; waiting on it",
            pin.file
        ));
        while download_busy(&part) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        let mgr = manager_for(dir, m);
        if mgr.is_ready() {
            return Ok(());
        }
        let last = last_errors()
            .lock()
            .ok()
            .and_then(|e| e.get(&part).cloned());
        return Err(last.unwrap_or_else(|| ModelError::Paused.user_message()));
    }
    let urls = sources_for(m);
    crate::model_log::event(&format!(
        "{}: download requested ({} bytes) from {}",
        pin.file,
        pin.size_bytes,
        urls.iter()
            .map(|u| crate::model_log::host_of(u))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut round = 0usize;
    let res =
        download_and_verify_with_clean_retry(dir, pin.file, pin.sha256, pin.size_bytes, || {
            // The clean retry after a checksum mismatch starts with the NEXT source, so a source that
            // served wrong bytes is not the first one asked again.
            let mut order = urls.clone();
            if !order.is_empty() {
                let k = round % order.len();
                order.rotate_left(k);
            }
            round += 1;
            let opts = ModelDownloadOptions::for_pin(pin, order.clone());
            let sources: Vec<Box<dyn ModelTransport>> = order
                .iter()
                .map(|u| {
                    Box::new(UreqModelTransport::new(u.clone()).with_expected_total(pin.size_bytes))
                        as Box<dyn ModelTransport>
                })
                .collect();
            // The lock covers the download; the verify after it runs once the lock is released, and
            // `status` reports it from the file state.
            let _ = std::fs::create_dir_all(dir);
            download_with(dir, sources, &opts)
        });
    match res {
        Ok(()) => {
            if let Ok(mut e) = last_errors().lock() {
                e.remove(&part);
            }
            Ok(())
        }
        Err(e) => {
            crate::model_log::event(&format!("{}: download stopped: {e}", pin.file));
            let msg = e.user_message();
            if let Ok(mut map) = last_errors().lock() {
                map.insert(part, msg.clone());
            }
            Err(msg)
        }
    }
}

/// **Command — model_download.** Stream + resume + verify the onboarding model into app-data.
/// `model` is `"default"` or `"small"`; when given it also becomes the member's choice (status,
/// the Ready gate and the serve path follow it). Resolves once the model is verified-Ready; an
/// honest, plain-words error otherwise (never a fake completion). A pause rejects with the paused
/// message; `model_status` then reads `paused`.
///
/// ASYNC + `spawn_blocking` — a multi-GB streamed download MUST NOT run on the main
/// thread. As a synchronous `#[tauri::command]` Tauri ran it on the UI thread, so the
/// whole download (minutes) beach-balled the window (observed 2026-09-10, same class as
/// the `membership_grant_status` fix). The blocking pool keeps the UI responsive; the
/// verify/quarantine honesty guarantees are unchanged.
#[tauri::command]
pub async fn model_download(
    app: tauri::AppHandle,
    model: Option<String>,
) -> std::result::Result<(), String> {
    let (dir, m) = {
        let st = tauri::Manager::try_state::<ModelState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let m = match model.as_deref() {
            None => st.choice(),
            Some(k) => {
                LocalModel::from_key(k).ok_or_else(|| format!("unknown local model '{k}'"))?
            }
        };
        if m != st.choice() {
            let old = st.dir().join(st.choice().pin().file);
            st.set_choice(m).map_err(|e| e.user_message())?;
            crate::model_log::event(&format!("local model choice is now {}", m.pin().file));
            if let Some(serve) = tauri::Manager::try_state::<crate::serve::ServeState>(&app) {
                serve.0.retarget_default(&old, st.dir().join(m.pin().file));
            }
        }
        (st.dir().to_path_buf(), m)
    };
    tauri::async_runtime::spawn_blocking(move || download_pinned_blocking(&dir, m))
        .await
        .map_err(|e| format!("model download: background task failed: {e}"))?
}

/// **Command — model_download_pause.** Pause the running download of the chosen model: it stops
/// at the next chunk and keeps its `.part` (resumed by the next `model_download`). With no download
/// running, a `.part` on disk is marked paused so a relaunch does not resume it on its own.
#[tauri::command]
pub async fn model_download_pause(app_h: tauri::AppHandle) -> std::result::Result<bool, String> {
    crate::blocking::off_main(move || {
        let st = tauri::Manager::try_state::<ModelState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let pin = st.choice().pin();
        let part = st.dir().join(format!("{}.part", pin.file));
        if request_pause(&part) {
            return Ok(true);
        }
        mark_paused(st.dir(), pin.file, pin.size_bytes, pin.sha256)
            .map_err(|e| e.user_message())?;
        Ok(false)
    })
    .await
}

/// One pinned model, as the onboarding step shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
    pub hf_repo: &'static str,
}

/// The onboarding model options and the facts the step shows next to them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelOptions {
    /// The member's current choice (`"default"` / `"small"`).
    pub chosen: &'static str,
    pub options: Vec<LocalModelInfo>,
    /// Free bytes on the disk holding the models folder, when known.
    pub free_bytes: Option<u64>,
    /// Bytes kept free beyond a model's size before a download starts.
    pub margin_bytes: u64,
}

fn info(p: &PinnedModel) -> LocalModelInfo {
    LocalModelInfo {
        key: p.key,
        label: p.label,
        file: p.file,
        size_bytes: p.size_bytes,
        sha256: p.sha256,
        hf_repo: p.hf_repo,
    }
}

/// **Command — model_local_options.** The default and the smaller onboarding model, the current
/// choice, and the free disk space (data source: the pinned constants and `statvfs` on the models
/// folder).
#[tauri::command]
pub async fn model_local_options(
    app_h: tauri::AppHandle,
) -> std::result::Result<LocalModelOptions, String> {
    crate::blocking::off_main(move || {
        let st = tauri::Manager::try_state::<ModelState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        Ok(LocalModelOptions {
            chosen: st.choice().pin().key,
            options: vec![info(&DEFAULT_PIN), info(&SMALL_PIN)],
            free_bytes: disk_free(st.dir()),
            margin_bytes: DISK_MARGIN_BYTES,
        })
    })
    .await
}

/// **Command — model_verify.** Stream the chosen model's file through SHA-256 and compare to the
/// pinned hash. On a match the model becomes `Ready`; on a mismatch the file is quarantined and an
/// honest error returned (never Ready). An already verified-Ready file is not hashed again.
///
/// ASYNC + `spawn_blocking` — hashing a ~4.6 GB file is seconds of blocking I/O that
/// must not run on the main thread (see `model_download`).
#[tauri::command]
pub async fn model_verify(app: tauri::AppHandle) -> std::result::Result<(), String> {
    let (dir, m) = {
        let st = tauri::Manager::try_state::<ModelState>(&app)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        (st.dir().to_path_buf(), st.choice())
    };
    tauri::async_runtime::spawn_blocking(move || {
        let mgr = manager_for(&dir, m);
        if mgr.is_ready() {
            return Ok(());
        }
        let r = mgr.verify();
        crate::model_log::event(&format!(
            "{}: verify {}",
            m.pin().file,
            match &r {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("failed: {e}"),
            }
        ));
        r.map_err(|e| e.user_message())
    })
    .await
    .map_err(|e| format!("model verify: background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("model_tests.rs");
}

#[cfg(test)]
mod dl_tests {
    include!("model_dl_tests.rs");
}

#[cfg(test)]
mod live_tests {
    include!("model_live_tests.rs");
}
