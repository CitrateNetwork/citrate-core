//! HUP-S3.1 — first-run import of the bundled Hermes knowledge corpus.
//!
//! The release ships a verified knowledge corpus (built in citrate-memories by
//! `mem-corpus`: per-tenant SyncBundles + `manifest.json` + `skills.lock` +
//! `NOTICE.md`) as the app resource `knowledge-corpus/`. On first run this module
//! imports it into the member's local memory store by running the bundled
//! memory binary as `mem-mcp import-corpus <store> <corpus-dir>` and reading
//! its JSON-lines progress contract (`mem_corpus::progress`, mirrored by
//! [`ImportLine`]).
//!
//! - **Store lock.** The import opens the RocksDB store directly, so the memory
//!   daemon is stopped for the import and restarted afterwards if it was running.
//! - **Idempotent.** The corpus's `bundle_digest` is written to
//!   `memory/knowledge-corpus.imported` only after a `done` line whose digest
//!   matches the bundled manifest. A matching marker skips the import without
//!   stopping the daemon. The store also records each tenant's bundle hash, so
//!   even a lost marker re-imports nothing.
//! - **Semantic only.** The import embeds with the store's embedder. Without the
//!   bundled BGE model a fresh store would be locked to lexical vectors, so the
//!   import is skipped (`not-semantic`), the same gate as the W3.2 docs preload.
//! - **Honest failure.** Any `error` line, a non-zero exit, a missing `done`
//!   line, or a digest mismatch is reported as `failed` with the reason, and no
//!   marker is written, so the next launch retries.
//!
//! Nothing here signs, holds a key, or touches the chain (Rule 3): it is a local
//! memory-store write of release content.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The Tauri event carrying each [`ImportLine`] while an import runs.
pub const PROGRESS_EVENT: &str = "memory://knowledge-import-progress";

/// Marker file (next to the store) holding the last imported `bundle_digest`.
pub const MARKER_FILE: &str = "knowledge-corpus.imported";

/// The app resource directory holding the bundled corpus.
pub const RESOURCE_DIR: &str = "knowledge-corpus";

/// Dev/test override for the corpus directory.
pub const CORPUS_DIR_ENV: &str = "CITRATE_KNOWLEDGE_CORPUS_DIR";

/// Most stderr bytes kept for an error message.
const STDERR_TAIL: usize = 2048;

/// One import at a time per store (a second request reports `in-progress`).
static IN_PROGRESS: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// Holds a store's slot in [`IN_PROGRESS`] for the duration of one import.
pub(crate) struct ImportGuard(PathBuf);

impl ImportGuard {
    pub(crate) fn acquire(store_path: &Path) -> Option<Self> {
        let mut set = IN_PROGRESS.lock().unwrap_or_else(|e| e.into_inner());
        set.insert(store_path.to_path_buf())
            .then(|| ImportGuard(store_path.to_path_buf()))
    }
}

impl Drop for ImportGuard {
    fn drop(&mut self) {
        IN_PROGRESS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// One line of the `mem-mcp import-corpus` JSON-lines contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ImportLine {
    Verified {
        bundle_digest: String,
        tenants: u64,
        nodes: u64,
        edges: u64,
    },
    TenantStart {
        tenant: String,
        nodes: u64,
        edges: u64,
    },
    Progress {
        tenant: String,
        done: u64,
        total: u64,
    },
    TenantSkipped {
        tenant: String,
        reason: String,
    },
    TenantDone {
        tenant: String,
    },
    Done {
        bundle_digest: String,
        embed_model: String,
        nodes_added: u64,
        nodes_merged: u64,
        edges_added: u64,
        tenants_imported: Vec<String>,
        tenants_skipped: Vec<String>,
        /// Nodes the importer embedded on this machine (0 from an older importer).
        #[serde(default)]
        nodes_embedded: u64,
        /// Nodes that took the corpus's precomputed vectors (0 from an older importer).
        #[serde(default)]
        vectors_reused: u64,
    },
    Error {
        stage: String,
        message: String,
    },
}

/// Parse one stdout line. Anything that is not a known contract line is `None`
/// (ignored), so a newer importer that adds events cannot break an older app.
pub fn parse_line(line: &str) -> Option<ImportLine> {
    serde_json::from_str(line.trim()).ok()
}

/// What an import did, surfaced to the UI. Never fabricated: counts come only
/// from the importer's own `done` line.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeImportReport {
    /// "imported" | "skipped" | "failed".
    pub state: String,
    /// Why nothing ran: "no-bundle" | "not-semantic" | "already-imported" | "in-progress".
    pub skipped: Option<String>,
    /// The failure reason, when `state == "failed"`.
    pub error: Option<String>,
    pub bundle_digest: Option<String>,
    pub embed_model: Option<String>,
    pub nodes_added: u64,
    pub edges_added: u64,
    pub tenants_imported: Vec<String>,
    pub tenants_skipped: Vec<String>,
    /// Nodes embedded on this machine during the import.
    pub nodes_embedded: u64,
    /// Nodes that took the release's precomputed vectors instead (no CPU embedding).
    pub vectors_reused: u64,
}

impl KnowledgeImportReport {
    fn skipped(reason: &str, digest: Option<String>) -> Self {
        KnowledgeImportReport {
            state: "skipped".into(),
            skipped: Some(reason.into()),
            bundle_digest: digest,
            ..Default::default()
        }
    }
    fn failed(error: String, digest: Option<String>) -> Self {
        KnowledgeImportReport {
            state: "failed".into(),
            error: Some(error),
            bundle_digest: digest,
            ..Default::default()
        }
    }
}

/// Everything one import needs; built from the live [`crate::memory::MemoryManager`]
/// in production and directly in tests.
#[derive(Debug, Clone)]
pub struct ImportPlan {
    /// The memory binary that implements `import-corpus` (the bundled `mem-mcp`).
    pub importer: PathBuf,
    pub store_path: PathBuf,
    /// The bundled corpus directory, `None` when the app ships without one.
    pub corpus_dir: Option<PathBuf>,
    /// Whether the BGE embedder is bundled (see the module docs).
    pub semantic: bool,
    /// Environment for the importer (the same embedder env the daemon gets).
    pub env: Vec<(String, String)>,
    /// How long to wait for the importer's first line (it verifies the manifest
    /// and prints `verified` or `error` long before any embedding starts).
    pub first_line_timeout: std::time::Duration,
}

/// Production bound for [`ImportPlan::first_line_timeout`]: verification of a
/// release corpus (tens of MB of sha256) takes seconds, not minutes.
pub const FIRST_LINE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// The marker path for a store.
pub fn marker_path(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(MARKER_FILE)
}

/// The corpus's `bundle_digest` from its `manifest.json` (64 lowercase hex).
pub fn read_bundle_digest(corpus_dir: &Path) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Head {
        bundle_digest: String,
    }
    let path = corpus_dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "knowledge corpus manifest unreadable ({}): {e}",
            path.display()
        )
    })?;
    let head: Head = serde_json::from_str(&text)
        .map_err(|e| format!("knowledge corpus manifest malformed: {e}"))?;
    let ok = head.bundle_digest.len() == 64
        && head
            .bundle_digest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !ok {
        return Err("knowledge corpus manifest has no valid bundle_digest".into());
    }
    Ok(head.bundle_digest)
}

fn recorded_digest(store_path: &Path) -> Option<String> {
    // A marker without a store is stale (a wiped store): never trust it.
    if !store_path.exists() {
        return None;
    }
    std::fs::read_to_string(marker_path(store_path))
        .ok()
        .map(|s| s.trim().to_string())
}

fn write_marker(store_path: &Path, digest: &str) -> Result<(), String> {
    let path = marker_path(store_path);
    let tmp = path.with_extension("imported.tmp");
    std::fs::write(&tmp, format!("{digest}\n")).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// Decide, then (if needed) run the importer. Does not touch the daemon; see
/// [`import_with_manager`] for the stop/restart around it (production takes the
/// import slot there, before the daemon is touched, so this entry is test-only).
#[cfg(test)]
pub fn run_plan(plan: &ImportPlan, on_line: impl FnMut(&ImportLine)) -> KnowledgeImportReport {
    let Some(_gate) = ImportGuard::acquire(&plan.store_path) else {
        return KnowledgeImportReport::skipped("in-progress", None);
    };
    run_plan_locked(plan, on_line)
}

/// [`run_plan`] for a caller that already holds the store's [`ImportGuard`].
fn run_plan_locked(plan: &ImportPlan, on_line: impl FnMut(&ImportLine)) -> KnowledgeImportReport {
    match decide(plan) {
        Decision::Skip(r) => r,
        Decision::Run { corpus_dir, digest } => run_importer(plan, &corpus_dir, &digest, on_line),
    }
}

enum Decision {
    Skip(KnowledgeImportReport),
    Run { corpus_dir: PathBuf, digest: String },
}

fn decide(plan: &ImportPlan) -> Decision {
    let Some(corpus_dir) = plan.corpus_dir.clone() else {
        return Decision::Skip(KnowledgeImportReport::skipped("no-bundle", None));
    };
    let digest = match read_bundle_digest(&corpus_dir) {
        Ok(d) => d,
        Err(e) => return Decision::Skip(KnowledgeImportReport::failed(e, None)),
    };
    if recorded_digest(&plan.store_path).as_deref() == Some(digest.as_str()) {
        return Decision::Skip(KnowledgeImportReport::skipped(
            "already-imported",
            Some(digest),
        ));
    }
    if !plan.semantic {
        return Decision::Skip(KnowledgeImportReport::skipped("not-semantic", Some(digest)));
    }
    Decision::Run { corpus_dir, digest }
}

fn needs_run(plan: &ImportPlan) -> bool {
    matches!(decide(plan), Decision::Run { .. })
}

fn run_importer(
    plan: &ImportPlan,
    corpus_dir: &Path,
    digest: &str,
    mut on_line: impl FnMut(&ImportLine),
) -> KnowledgeImportReport {
    let fail = |e: String| KnowledgeImportReport::failed(e, Some(digest.to_string()));
    if let Some(parent) = plan.store_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return fail(format!("cannot create the memory directory: {e}"));
        }
    }
    let mut cmd = Command::new(&plan.importer);
    cmd.arg("import-corpus")
        .arg(&plan.store_path)
        .arg(corpus_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in &plan.env {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return fail(format!(
                "cannot start the memory importer ({}): {e}",
                plan.importer.display()
            ))
        }
    };
    // Drain stderr on its own thread so a chatty importer can never block on a
    // full pipe; keep only the tail for the error message.
    let stderr_tail = child.stderr.take().map(|mut err| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            let start = buf.len().saturating_sub(STDERR_TAIL);
            String::from_utf8_lossy(&buf[start..]).trim().to_string()
        })
    });
    // Read stdout on its own thread so the first line can be waited on with a bound:
    // a bundled mem-mcp that predates `import-corpus` would take the arguments as a
    // store path and serve forever instead of answering.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    let mut done: Option<ImportLine> = None;
    let mut error: Option<String> = None;
    let mut first = true;
    loop {
        let next = if first {
            first = false;
            match rx.recv_timeout(plan.first_line_timeout) {
                Ok(l) => Some(l),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return fail(format!(
                        "memory importer did not respond within {} s (is the bundled mem-mcp older than the corpus?)",
                        plan.first_line_timeout.as_secs()
                    ));
                }
            }
        } else {
            rx.recv().ok()
        };
        let Some(line) = next else { break };
        let Some(parsed) = parse_line(&line) else {
            continue;
        };
        on_line(&parsed);
        match &parsed {
            ImportLine::Done { .. } => done = Some(parsed.clone()),
            ImportLine::Error { stage, message } => error = Some(format!("{stage}: {message}")),
            _ => {}
        }
    }
    let status = child.wait();
    let stderr = stderr_tail.and_then(|h| h.join().ok()).unwrap_or_default();
    let exit_ok = matches!(&status, Ok(s) if s.success());
    if let Some(e) = error {
        return fail(e);
    }
    if !exit_ok {
        let code = match &status {
            Ok(s) => s.to_string(),
            Err(e) => e.to_string(),
        };
        let detail = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        return fail(format!(
            "memory importer exited unsuccessfully ({code}){detail}"
        ));
    }
    let Some(ImportLine::Done {
        bundle_digest,
        embed_model,
        nodes_added,
        edges_added,
        tenants_imported,
        tenants_skipped,
        nodes_embedded,
        vectors_reused,
        ..
    }) = done
    else {
        return fail("memory importer exited without reporting completion".into());
    };
    if bundle_digest != digest {
        return fail(format!(
            "memory importer reported corpus digest {bundle_digest}, expected the bundled {digest}"
        ));
    }
    if let Err(e) = write_marker(&plan.store_path, digest) {
        return fail(format!(
            "imported, but the completion marker could not be written: {e}"
        ));
    }
    KnowledgeImportReport {
        state: "imported".into(),
        skipped: None,
        error: None,
        bundle_digest: Some(bundle_digest),
        embed_model: Some(embed_model),
        nodes_added,
        edges_added,
        tenants_imported,
        tenants_skipped,
        nodes_embedded,
        vectors_reused,
    }
}

/// Production path: stop the daemon if it is running (the import needs the
/// store lock), import, and restart it if it was running. A restart failure is
/// appended to the report, never hidden.
pub fn import_with_manager(
    mgr: &crate::memory::MemoryManager,
    corpus_dir: Option<PathBuf>,
    on_line: impl FnMut(&ImportLine),
) -> KnowledgeImportReport {
    let plan = ImportPlan {
        importer: mgr.daemon_bin().to_path_buf(),
        store_path: mgr.store_path().to_path_buf(),
        corpus_dir,
        semantic: mgr.status().semantic,
        env: mgr.embedder_env(),
        first_line_timeout: FIRST_LINE_TIMEOUT,
    };
    // Take the store's import slot BEFORE touching the daemon: a caller that loses
    // the race must neither stop nor restart a daemon around someone else's import.
    let Some(_gate) = ImportGuard::acquire(&plan.store_path) else {
        return KnowledgeImportReport::skipped("in-progress", None);
    };
    // Only stop the daemon when there is real work to do.
    if !needs_run(&plan) {
        return run_plan_locked(&plan, on_line);
    }
    let was_running = mgr.is_running_now();
    if was_running {
        mgr.stop();
    }
    let mut report = run_plan_locked(&plan, on_line);
    if was_running {
        if let Err(e) = mgr.start() {
            let note = format!("memory daemon restart after import failed: {e}");
            report.error = Some(match report.error.take() {
                Some(prev) => format!("{prev}; {note}"),
                None => note,
            });
        }
    }
    report
}

/// `dir` when it holds a staged corpus (a `manifest.json` file), else `None`.
/// Every build ships `knowledge-corpus/README.md` so the bundle resource glob
/// always matches; only a release that staged a corpus adds the manifest, so a
/// README-only directory is an honest `no-bundle`, not a failed import.
pub fn staged_corpus_dir(dir: &Path) -> Option<PathBuf> {
    let manifest = dir.join("manifest.json");
    std::fs::metadata(&manifest)
        .map(|m| m.is_file())
        .unwrap_or(false)
        .then(|| dir.to_path_buf())
}

/// The bundled corpus directory: [`CORPUS_DIR_ENV`] first (dev), else the app
/// resource dir. `None` when no corpus is staged: the app honestly ships
/// without one until the release stages it (`scripts/stage-knowledge-corpus.sh`).
fn resolve_corpus_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    use tauri::Manager;
    if let Ok(p) = std::env::var(CORPUS_DIR_ENV) {
        if let Some(d) = staged_corpus_dir(Path::new(&p)) {
            return Some(d);
        }
    }
    let d = app.path().resource_dir().ok()?.join(RESOURCE_DIR);
    staged_corpus_dir(&d)
}

/// HUP-S3.1 — import the bundled knowledge corpus (first run; idempotent).
/// Emits [`PROGRESS_EVENT`] with each [`ImportLine`] while it runs.
#[tauri::command]
pub async fn memory_import_knowledge<R: tauri::Runtime>(
    app_h: tauri::AppHandle<R>,
) -> Result<KnowledgeImportReport, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st = tauri::Manager::try_state::<crate::memory::MemoryState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        memory_import_knowledge_sync(app_h.clone(), st)
    })
    .await
}

/// Blocking body of [`memory_import_knowledge`]; reached only through [`crate::blocking::off_main`].
pub fn memory_import_knowledge_sync<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: tauri::State<'_, crate::memory::MemoryState>,
) -> Result<KnowledgeImportReport, String> {
    use tauri::Emitter;
    let dir = resolve_corpus_dir(&app);
    Ok(import_with_manager(&state.0, dir, |line| {
        let _ = app.emit(PROGRESS_EVENT, line);
    }))
}

#[cfg(test)]
mod tests {
    include!("knowledge_import_tests.rs");
}
