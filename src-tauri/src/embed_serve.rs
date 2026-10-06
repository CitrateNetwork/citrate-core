//! HUP-S1.2 / US-1.4 — the loopback embedding server Hermes ranks tools and skills with.
//!
//! The Hermes sidecar ranks the tools it offers per request (at most 8 schemas) and the skills it
//! surfaces per turn by blending keyword scores with embedding similarity. Without an embedding
//! endpoint every session ranks lexically and says so. This module starts the bundled
//! `llama-server` a second time, on the bundled BGE model (`bge-base-en-v1.5`, GGUF, pinned by
//! sha256), in embedding-only mode:
//!
//! `llama-server -m <bge.gguf> --host 127.0.0.1 --port 18085 --embeddings --pooling cls
//!  --ctx-size 512 --batch-size 512 --ubatch-size 512 --threads 2 --no-webui --no-slots`
//!
//! - Loopback only, and only when nothing else holds the port (PBA-L7b-009).
//! - A per-launch API key goes to the child through `LLAMA_API_KEY` (never argv), and to the
//!   sidecar as a `0600` FILE path (`CITRATE_HERMES_EMBED_KEY_FILE`), never the key itself.
//! - The model file is checked against [`EMBED_MODEL_SHA256`] before the first start; a missing or
//!   different file means no embedding server, and the sessions report lexical ranking.
//! - Supervised like the chat server (bounded restart, `/health` liveness).
//!
//! BGE's sentence embedding is its `[CLS]` token, hence `--pooling cls`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::supervisor::{BackoffPolicy, HealthCheck, SidecarSpec, Supervisor, SupervisorConfig};

/// The loopback port of the embedding server (the chat server is 18080).
pub const EMBED_PORT: u16 = 18085;
/// Where the bundled model sits under the app's resources.
pub const EMBED_MODEL_DIR: &str = "models/bge-base-en-v1.5-gguf";
/// The bundled model file (converted from BAAI/bge-base-en-v1.5, see
/// `scripts/build-bge-gguf.sh`; staged from the `runtime-deps` prerelease).
pub const EMBED_MODEL_FILE: &str = "bge-base-en-v1.5-f16.gguf";
/// The model file's sha256 (also pinned in `src-tauri/runtime-deps.sha256`).
pub const EMBED_MODEL_SHA256: &str =
    "fa9e1f6c0109aa2507a09d97d02aad84774d5b2ba6d3199f7af7e0fba4c6eaee";
/// Env override for the model path (dev and tests). The digest is still checked.
pub const EMBED_MODEL_ENV: &str = "CITRATE_EMBED_GGUF";
/// The sidecar env naming the embedding endpoint (runtime `retrieval_http::EMBED_URL_ENV`).
pub const HERMES_EMBED_URL_ENV: &str = "CITRATE_HERMES_EMBED_URL";
/// The sidecar env naming the key FILE (runtime `retrieval_http::EMBED_KEY_FILE_ENV`).
pub const HERMES_EMBED_KEY_FILE_ENV: &str = "CITRATE_HERMES_EMBED_KEY_FILE";
/// BGE's context is 512 tokens; a tool description or skill summary is far shorter.
const EMBED_CTX: u32 = 512;
/// Embedding a tool catalog is light; keep the server from competing with the chat model.
const EMBED_THREADS: u32 = 2;
/// A 220 MB model loads in about a second; allow for a cold disk.
const EMBED_START_GRACE: Duration = Duration::from_secs(60);
const EMBED_HEALTHY_AFTER: Duration = Duration::from_secs(30);
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);

/// Why the embedding server is not running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbedError {
    BinaryNotFound,
    ModelNotFound,
    /// The model file is not the pinned one.
    ModelMismatch,
    PortInUse(u16),
    KeyFile(String),
    Spawn(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedError::BinaryNotFound => write!(f, "the llama-server binary is not bundled"),
            EmbedError::ModelNotFound => write!(f, "the BGE embedding model is not bundled"),
            EmbedError::ModelMismatch => {
                write!(f, "the BGE embedding model is not the pinned file")
            }
            EmbedError::PortInUse(p) => write!(
                f,
                "loopback port {p} is held by another process; not starting the embedding server"
            ),
            EmbedError::KeyFile(m) => write!(f, "the embedding key file could not be written: {m}"),
            EmbedError::Spawn(m) => write!(f, "the embedding server could not start: {m}"),
        }
    }
}

/// The supervised embedding `llama-server`.
pub struct EmbedServer {
    bin: PathBuf,
    model: PathBuf,
    port: u16,
    key_file: PathBuf,
    crash_record_path: PathBuf,
    api_key: Zeroizing<String>,
    /// `Some(true)` once the model matched the pin (checked once per launch).
    verified: Mutex<Option<bool>>,
    sup: Mutex<Option<Supervisor>>,
}

/// sha256 of a file, hex.
pub fn file_sha256(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

impl EmbedServer {
    pub fn new(
        bin: PathBuf,
        model: PathBuf,
        port: u16,
        key_file: PathBuf,
        crash_record_path: PathBuf,
    ) -> Self {
        EmbedServer {
            bin,
            model,
            port,
            key_file,
            crash_record_path,
            api_key: crate::serve::mint_api_key(),
            verified: Mutex::new(None),
            sup: Mutex::new(None),
        }
    }

    /// The model file this server loads (the Models screen never deletes it while it runs).
    pub fn model_path(&self) -> &Path {
        &self.model
    }

    /// The OpenAI-style base URL the sidecar embeds through (no key in it).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The argv: loopback, embedding-only, BGE's CLS pooling.
    pub fn spawn_args(&self) -> Vec<String> {
        vec![
            "-m".into(),
            self.model.to_string_lossy().to_string(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            self.port.to_string(),
            "--embeddings".into(),
            "--pooling".into(),
            "cls".into(),
            "--ctx-size".into(),
            EMBED_CTX.to_string(),
            // An embedding input is processed in one micro-batch: size both to the model's context.
            "--batch-size".into(),
            EMBED_CTX.to_string(),
            "--ubatch-size".into(),
            EMBED_CTX.to_string(),
            "--threads".into(),
            EMBED_THREADS.to_string(),
            "--no-webui".into(),
            "--no-slots".into(),
        ]
    }

    /// The env for the sidecar while this server runs: the URL and the key FILE path.
    pub fn sidecar_env(&self) -> Vec<(String, String)> {
        vec![
            (HERMES_EMBED_URL_ENV.to_string(), self.url()),
            (
                HERMES_EMBED_KEY_FILE_ENV.to_string(),
                self.key_file.to_string_lossy().to_string(),
            ),
        ]
    }

    fn model_ok(&self) -> Result<(), EmbedError> {
        let mut v = self.verified.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ok) = *v {
            return if ok {
                Ok(())
            } else {
                Err(EmbedError::ModelMismatch)
            };
        }
        let digest = file_sha256(&self.model).map_err(|_| EmbedError::ModelNotFound)?;
        let ok = digest == EMBED_MODEL_SHA256;
        *v = Some(ok);
        if ok {
            Ok(())
        } else {
            Err(EmbedError::ModelMismatch)
        }
    }

    /// Whether the server was started and its supervisor has not given up (it may still be
    /// loading the model; a session opened meanwhile ranks lexically and says why).
    pub fn is_started(&self) -> bool {
        self.sup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|s| {
                !matches!(
                    s.status().state,
                    crate::supervisor::SupervisorState::Failed
                        | crate::supervisor::SupervisorState::Off
                )
            })
    }

    /// Start the server if it is not running. Fails closed on a missing binary, a missing or
    /// different model, or a taken port; every failure leaves sessions ranking lexically.
    pub fn ensure_started(&self) -> Result<(), EmbedError> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        if !self.bin.exists() {
            return Err(EmbedError::BinaryNotFound);
        }
        self.model_ok()?;
        if !crate::serve::loopback_port_is_free(self.port) {
            return Err(EmbedError::PortInUse(self.port));
        }
        citrate_core_kit::fsutil::write_secret_file(&self.key_file, self.api_key.as_bytes())
            .map_err(|e| EmbedError::KeyFile(e.kind().to_string()))?;
        let mut spec = SidecarSpec::new("llama-embed", self.bin.clone(), self.spawn_args());
        spec.env.push((
            crate::serve::LLAMA_API_KEY_ENV.to_string(),
            self.api_key.to_string(),
        ));
        let health_url = format!("{}/health", self.url());
        spec.health_check = Some(HealthCheck {
            interval: HEALTH_INTERVAL,
            grace: EMBED_START_GRACE,
            probe: std::sync::Arc::new(move || crate::serve::http_health_ok(&health_url)),
        });
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        config.healthy_after = EMBED_HEALTHY_AFTER;
        let sup = Supervisor::start(config).map_err(|e| EmbedError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the server (graceful, no orphan). Idempotent.
    pub fn stop(&self) {
        let sup = self.sup.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(sup) = sup {
            sup.stop();
        }
        let _ = std::fs::remove_file(&self.key_file);
    }
}

#[cfg(test)]
impl EmbedServer {
    /// Test hook: the server child's pid while the supervisor holds one (SCL-S0.5 coverage).
    pub(crate) fn pid_for_test(&self) -> Option<u32> {
        self.sup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|s| s.status().pid)
    }

    /// Test hook: treat the model as the pinned file (only the real 220 MB file has the digest).
    pub(crate) fn assume_verified_for_test(self) -> Self {
        *self.verified.lock().unwrap_or_else(|e| e.into_inner()) = Some(true);
        self
    }
}

/// The bundled model path: [`EMBED_MODEL_ENV`] when set, else under the resource dir.
pub fn resolve_model(resource_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(EMBED_MODEL_ENV).filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    resource_dir.map(|r| r.join(EMBED_MODEL_DIR).join(EMBED_MODEL_FILE))
}

#[cfg(test)]
mod tests {
    include!("embed_serve_tests.rs");
}
