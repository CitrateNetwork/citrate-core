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
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelError::Transport(m) => write!(f, "model transport error: {m}"),
            ModelError::BadMagic => write!(f, "model download is not a GGUF file (bad magic)"),
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
        }
    }
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
    /// A `.part` download is in progress (or was interrupted). `pct` in [0,100].
    #[serde(rename_all = "camelCase")]
    Downloading {
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
}

/// Production transport: blocking `ureq` (rustls TLS), the same client ai.rs and
/// rpc.rs use — no new heavy dep. Range requests power the resume.
pub struct UreqModelTransport {
    url: String,
}

impl UreqModelTransport {
    pub fn new(url: impl Into<String>) -> Self {
        UreqModelTransport { url: url.into() }
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
        let resp = ureq::get(&self.url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(45)))
            .build()
            .header("Range", "bytes=0-0")
            .call()
            .map_err(|e| ModelError::Transport(e.to_string()))?;
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
        let resp = ureq::get(&self.url)
            .config()
            .timeout_connect(Some(std::time::Duration::from_secs(30)))
            .timeout_recv_response(Some(std::time::Duration::from_secs(90)))
            .build()
            .header("Range", &range)
            .call()
            .map_err(|e| ModelError::Transport(e.to_string()))?;
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
    /// The HTTP transport (production `ureq`; tests inject a fixture).
    transport: Box<dyn ModelTransport>,
    /// The pinned expected sha256 hex (real Gemma in prod; fixture in tests).
    expected_hash: String,
    /// The pinned expected byte length.
    expected_size: u64,
    /// The model's on-disk filename. Defaults to [`MODEL_FILE`] (the bundled Gemma) for
    /// backward compatibility; the catalog (CX-S1) overrides it per model via [`with_file`]
    /// / [`from_descriptor`], so multiple models coexist in `models/` by filename. The
    /// `.part` and `.status.json` side-files are DERIVED from it.
    file: String,
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
        ModelManager {
            dir,
            transport,
            expected_hash,
            expected_size,
            file: MODEL_FILE.to_string(),
        }
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
        let blob = serde_json::to_vec(&StatusFile { verified })
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
            // Present but not verified — honestly report the download as complete
            // but NOT ready (the verify has not been earned yet). Reported as a
            // 100%-downloaded state so the UI can prompt "verify".
            let size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
            return ModelStatus::Downloading {
                downloaded_bytes: size,
                total_bytes: self.expected_size,
                pct: 100.0,
            };
        }
        let part = self.part_path();
        if part.exists() {
            let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            let total = self.expected_size.max(1);
            let pct = (got as f64 / total as f64) * 100.0;
            return ModelStatus::Downloading {
                downloaded_bytes: got,
                total_bytes: self.expected_size,
                pct: pct.clamp(0.0, 100.0),
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

        // Resume from the current partial size, if any.
        let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        // Never resume past what we expect (a corrupt oversized part → restart).
        if have >= self.expected_size {
            have = 0;
            let _ = std::fs::remove_file(&part);
        }

        on_progress(have, self.expected_size); // initial (resume offset, or 0)
        let offsets = vec![have];
        let mut reader = self.transport.get_from(have)?;

        // Open the part file for append (resume) or create (fresh).
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .map_err(|e| ModelError::Io(e.to_string()))?;

        // On a FRESH download (have == 0) the first 4 bytes must be the GGUF magic.
        // We buffer the first bytes to check before committing them.
        let mut written = have;
        let mut magic_checked = have >= GGUF_MAGIC.len() as u64;
        let mut magic_buf: Vec<u8> = Vec::with_capacity(GGUF_MAGIC.len());

        let mut buf = vec![0u8; DOWNLOAD_CHUNK];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| ModelError::Transport(e.to_string()))?;
            if n == 0 {
                break;
            }
            let mut chunk = &buf[..n];

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
                        return Err(ModelError::BadMagic);
                    }
                    // Magic OK — persist it, then continue with the rest.
                    file.write_all(&magic_buf)
                        .map_err(|e| ModelError::Io(e.to_string()))?;
                    written += magic_buf.len() as u64;
                    magic_checked = true;
                }
            }
            if magic_checked && !chunk.is_empty() {
                file.write_all(chunk)
                    .map_err(|e| ModelError::Io(e.to_string()))?;
                written += chunk.len() as u64;
                // Overshoot guard: a server that IGNORES `Range` re-sends the full
                // body onto a resume, driving `written` past the pinned length.
                // Reset the corrupt oversized `.part` and fail closed — never carry
                // an over-length partial forward, never finalize it.
                if written > self.expected_size {
                    drop(file);
                    let _ = std::fs::remove_file(&part);
                    return Err(ModelError::Overshoot {
                        got: written,
                        want: self.expected_size,
                    });
                }
            }
            on_progress(written, self.expected_size);
        }
        file.flush().map_err(|e| ModelError::Io(e.to_string()))?;
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
        let _ = self.write_verified(false);
        Ok(offsets)
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

/// Managed Tauri state: the process-wide model manager.
pub struct ModelState(pub ModelManager);

/// Build the managed model state from a live app handle: the app-data `models/`
/// dir, the real `ureq` transport pointed at the resolved URL (env-overridable),
/// and the pinned real Gemma hash/size.
pub fn build_model_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<ModelState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let dir = data_root.join("models");
    let url = resolve_model_url(std::env::var(MODEL_URL_ENV).ok());
    let mgr = ModelManager::new(
        dir.clone(),
        Box::new(UreqModelTransport::new(url)),
        MODEL_SHA256.to_string(),
        MODEL_SIZE_BYTES,
    );
    // First-run SEED: if the model isn't already verified-Ready and a copy is BUNDLED
    // in the app resources (`resources/models/<MODEL_FILE>`), seed it locally instead
    // of a network download — so a fresh install has chat out-of-the-box (offline).
    if !mgr.is_ready() {
        if let Ok(res) = app.path().resource_dir() {
            let bundled = res.join("models").join(MODEL_FILE);
            if bundled.exists() {
                seed_from_bundle(dir, bundled);
            }
        }
    }
    Ok(ModelState(mgr))
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
        let mgr = ModelManager::new(
            dir,
            Box::new(UreqModelTransport::new(String::new())),
            MODEL_SHA256.to_string(),
            MODEL_SIZE_BYTES,
        );
        let _ = mgr.verify();
    });
}

// ---------------------------------------------------------------------------
// Tauri commands — the `model` bridge surface. Return status / () only; no
// secrets, no key material (Rule 1: downloading/ready reflect real bytes+hash).
// ---------------------------------------------------------------------------

use tauri::State;

/// **Command — model_status.** The honest, file-derived model state. `Ready`
/// only when a real verify matched the pinned hash (never mere presence).
#[tauri::command]
pub fn model_status(state: State<'_, ModelState>) -> std::result::Result<ModelStatus, String> {
    Ok(state.0.status())
}

/// The app-data `models/` dir + resolved Gemma URL, for rebuilding a bare manager
/// off the main thread. We rebuild (rather than move the managed `State`) because the
/// manager holds a `Box<dyn ModelTransport>` that is not `Send`-cloneable across
/// `spawn_blocking`, and `build_model_state` has a first-run seed side effect we must
/// not repeat here. The pins are the same process-wide Gemma constants.
fn gemma_dir_and_url(app: &tauri::AppHandle) -> std::result::Result<(PathBuf, String), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let url = resolve_model_url(std::env::var(MODEL_URL_ENV).ok());
    Ok((dir, url))
}

/// **Command — model_download.** Stream + resume the pinned Gemma GGUF into
/// app-data, checking the GGUF magic and the pinned length. Returns () on a
/// completed download; an honest error otherwise (never a fake completion).
///
/// ASYNC + `spawn_blocking` — a multi-GB streamed download MUST NOT run on the main
/// thread. As a synchronous `#[tauri::command]` Tauri ran it on the UI thread, so the
/// whole download (minutes) beach-balled the window (observed 2026-09-10, same class as
/// the `membership_grant_status` fix). The blocking pool keeps the UI responsive; the
/// verify/quarantine honesty guarantees are unchanged.
#[tauri::command]
pub async fn model_download(app: tauri::AppHandle) -> std::result::Result<(), String> {
    let (dir, url) = gemma_dir_and_url(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mgr = ModelManager::new(
            dir,
            Box::new(UreqModelTransport::new(url)),
            MODEL_SHA256.to_string(),
            MODEL_SIZE_BYTES,
        );
        mgr.download().map(|_| ()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("model download: background task failed: {e}"))?
}

/// **Command — model_verify.** Stream the downloaded file through SHA-256 and
/// compare to the pinned hash. On a match the model becomes `Ready`; on a
/// mismatch the file is quarantined and an honest error returned (never Ready).
///
/// ASYNC + `spawn_blocking` — hashing a ~4.6 GB file is seconds of blocking I/O that
/// must not run on the main thread (see `model_download`).
#[tauri::command]
pub async fn model_verify(app: tauri::AppHandle) -> std::result::Result<(), String> {
    let (dir, url) = gemma_dir_and_url(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mgr = ModelManager::new(
            dir,
            Box::new(UreqModelTransport::new(url)),
            MODEL_SHA256.to_string(),
            MODEL_SIZE_BYTES,
        );
        mgr.verify().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("model verify: background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("model_tests.rs");
}
