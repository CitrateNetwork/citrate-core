//! Deleting a downloaded model from this machine (the member's Models screen only).
//!
//! Data sources (Rule 7): the app's `models/` folder (the verified GGUFs `model_catalog_local`
//! lists, with their `.status.json`, `.download.json` and `.part` side files), the chat
//! llama-server's selected model (`serve.rs`), the embedding llama-server's model
//! (`embed_serve.rs`, inside the Hermes manager) and the download single-flight registry
//! (`model::DownloadGuard`).
//!
//! **What may be deleted.** Only a verified model file that sits directly in the models folder:
//! a plain file name ending in `.gguf` (no separators, no `..`, no hidden names), resolved inside
//! the canonical models folder, not a symlink, a regular file, and recorded verified
//! (`model::is_file_ready`). A model shipped in the app bundle (`<resources>/models/<file>`) is
//! not deletable: the app seeds it again on the next start.
//!
//! **When it is refused.** While the chat llama-server (which chat and Hermes use) is running or
//! starting on it, while the embedding server runs on it, or while a download of it is in progress.
//! The download lock is held for the whole delete, so a download cannot start meanwhile.
//!
//! **What is removed.** The GGUF and its side files (`.part`, `.status.json`, `.download.json`).
//! The freed bytes are returned. When the deleted model was the chat server's selected model,
//! the selection falls back to the default model path (the server is not started).
//!
//! **Member-only.** A `#[tauri::command]` in the main window's ACL. It is not a Hermes tool, not a
//! node MCP tool and not in the chat tool list: an agent can never delete a model.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The side files a model leaves next to its GGUF.
pub const SIDE_SUFFIXES: &[&str] = &[".part", ".status.json", ".download.json"];
/// Longest file name accepted.
const MAX_NAME: usize = 255;

/// Why a model cannot be deleted. The text is shown to the member as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteRefusal {
    /// Not a plain model file name, or it resolves outside the models folder.
    BadName(String),
    /// No verified model by that name in the models folder.
    Unknown(String),
    /// The name is a link (it could point anywhere).
    Symlink,
    /// Shipped in the app bundle.
    Bundled,
    /// In use (the reason says by what).
    InUse(String),
    /// A download of it is running.
    Downloading,
    Io(String),
}

impl std::fmt::Display for DeleteRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeleteRefusal::BadName(why) => write!(f, "that is not a downloaded model: {why}"),
            DeleteRefusal::Unknown(file) => {
                write!(f, "{file} is not a downloaded model on this machine")
            }
            DeleteRefusal::Symlink => f.write_str(
                "that model file is a link, so it is not deleted from here; remove it in Finder or your file manager",
            ),
            DeleteRefusal::Bundled => f.write_str(
                "this model ships with Citrate Core and comes back on the next start, so it cannot be deleted",
            ),
            DeleteRefusal::InUse(why) => write!(f, "in use: {why}"),
            DeleteRefusal::Downloading => {
                f.write_str("a download of this model is in progress; wait for it to finish first")
            }
            DeleteRefusal::Io(e) => write!(f, "could not delete the model: {e}"),
        }
    }
}

/// What is using a model file right now. Production reads the live servers; tests pass a fake.
pub trait ModelUsage {
    /// A plain reason when the model at `canonical` is in use, else `None`.
    fn in_use(&self, canonical: &Path) -> Option<String>;
}

/// The result of a delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub file: String,
    /// Bytes removed (the GGUF plus any side files).
    pub freed_bytes: u64,
    /// The chat server had this model selected; the selection fell back to the default.
    pub selection_cleared: bool,
}

/// One row of the Models screen's delete state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteState {
    pub file: String,
    pub size_bytes: u64,
    pub deletable: bool,
    /// Why not, when not deletable.
    pub reason: Option<String>,
}

/// Accept `local:<file>` (the Models list id) or a bare file name; return the file name.
pub fn file_from_id(id: &str) -> Result<String, DeleteRefusal> {
    let file = id.strip_prefix("local:").unwrap_or(id);
    let bad = |why: &str| Err(DeleteRefusal::BadName(why.to_string()));
    if file.is_empty() {
        return bad("no file name");
    }
    if file.len() > MAX_NAME {
        return bad("the name is too long");
    }
    if file.contains('/') || file.contains('\\') || file.contains('\0') {
        return bad("a model is named by its file name only, not a path");
    }
    if file == "." || file == ".." || file.contains("..") || file.starts_with('.') {
        return bad("the name is not a plain file name");
    }
    if !file.ends_with(".gguf") {
        return bad("only .gguf model files can be deleted here");
    }
    Ok(file.to_string())
}

/// Every check except the download lock: the canonical path of a deletable model.
pub fn check(
    models_dir: &Path,
    bundled_dir: Option<&Path>,
    file: &str,
    usage: &dyn ModelUsage,
) -> Result<PathBuf, DeleteRefusal> {
    let file = file_from_id(file)?;
    let dir =
        std::fs::canonicalize(models_dir).map_err(|_| DeleteRefusal::Unknown(file.clone()))?;
    let path = dir.join(&file);
    let meta =
        std::fs::symlink_metadata(&path).map_err(|_| DeleteRefusal::Unknown(file.clone()))?;
    if meta.file_type().is_symlink() {
        return Err(DeleteRefusal::Symlink);
    }
    if !meta.is_file() {
        return Err(DeleteRefusal::Unknown(file.clone()));
    }
    // Belt and braces: the resolved file must sit directly in the canonical models folder.
    let canonical = std::fs::canonicalize(&path).map_err(|e| DeleteRefusal::Io(e.to_string()))?;
    if canonical.parent() != Some(dir.as_path()) {
        return Err(DeleteRefusal::BadName(
            "it resolves outside the models folder".into(),
        ));
    }
    if !crate::model::is_file_ready(&dir, &file) {
        return Err(DeleteRefusal::Unknown(file));
    }
    if bundled_dir.is_some_and(|b| b.join(&file).exists()) {
        return Err(DeleteRefusal::Bundled);
    }
    if let Some(why) = usage.in_use(&canonical) {
        return Err(DeleteRefusal::InUse(why));
    }
    Ok(canonical)
}

/// Delete one model: [`check`], then under the download lock remove the GGUF and its side files.
pub fn delete(
    models_dir: &Path,
    bundled_dir: Option<&Path>,
    file: &str,
    usage: &dyn ModelUsage,
) -> Result<Deleted, DeleteRefusal> {
    let canonical = check(models_dir, bundled_dir, file, usage)?;
    let dir = canonical
        .parent()
        .ok_or_else(|| DeleteRefusal::Io("no models folder".into()))?
        .to_path_buf();
    let file = file_from_id(file)?;
    // The same single-flight lock a download takes: none can start while this runs, and a
    // running one makes this refuse.
    let _lock = crate::model::DownloadGuard::acquire(&dir.join(format!("{file}.part")))
        .map_err(|_| DeleteRefusal::Downloading)?;
    // Checked again under the lock (a download may have finished or a server started meanwhile).
    let canonical = check(&dir, bundled_dir, &file, usage)?;
    let mut freed = 0u64;
    let mut remove = |p: &Path| -> Result<(), DeleteRefusal> {
        match std::fs::symlink_metadata(p) {
            Ok(m) => {
                // A side file that is a link is removed as a link (its target is left alone) and
                // counts nothing.
                let len = if m.is_file() { m.len() } else { 0 };
                std::fs::remove_file(p).map_err(|e| DeleteRefusal::Io(e.to_string()))?;
                freed = freed.saturating_add(len);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(DeleteRefusal::Io(e.to_string())),
        }
    };
    // The GGUF first: once it is gone the model is no longer listed or selectable.
    remove(&canonical)?;
    for suffix in SIDE_SUFFIXES {
        remove(&dir.join(format!("{file}{suffix}")))?;
    }
    Ok(Deleted {
        file,
        freed_bytes: freed,
        selection_cleared: false,
    })
}

/// The delete state of every listed (verified) model.
pub fn states(
    models_dir: &Path,
    bundled_dir: Option<&Path>,
    usage: &dyn ModelUsage,
) -> Result<Vec<DeleteState>, String> {
    let models = crate::model_catalog::read_local_models(models_dir)?;
    Ok(models
        .into_iter()
        .map(|m| {
            let mut reason = check(models_dir, bundled_dir, &m.file, usage)
                .err()
                .map(|r| r.to_string());
            if reason.is_none()
                && crate::model::download_in_flight(&models_dir.join(format!("{}.part", m.file)))
            {
                reason = Some(DeleteRefusal::Downloading.to_string());
            }
            DeleteState {
                file: m.file,
                size_bytes: m.size_bytes,
                deletable: reason.is_none(),
                reason,
            }
        })
        .collect())
}

/// The live usage: the chat llama-server (chat and Hermes) and the embedding server.
struct LiveUsage<'a> {
    serve: &'a crate::serve::LlamaServerManager,
}

fn same_file(a: &Path, canonical: &Path) -> bool {
    std::fs::canonicalize(a).is_ok_and(|c| c == canonical)
}

impl ModelUsage for LiveUsage<'_> {
    fn in_use(&self, canonical: &Path) -> Option<String> {
        if self.serve.status().state != "stopped"
            && same_file(&self.serve.current_model_path(), canonical)
        {
            return Some(
                "the local model server is running on it for chat and Hermes; switch to another model or stop the local model first"
                    .into(),
            );
        }
        if crate::hermes::embed_model_in_use().is_some_and(|p| same_file(&p, canonical)) {
            return Some("Hermes's embedding server is running on it; stop Hermes first".into());
        }
        None
    }
}

fn dirs<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    use tauri::Manager;
    let models = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models");
    let bundled = app.path().resource_dir().ok().map(|r| r.join("models"));
    Ok((models, bundled))
}

/// **model_delete_states**: which downloaded models can be deleted now, and why not (main
/// window only).
#[tauri::command]
pub async fn model_delete_states(app: tauri::AppHandle) -> Result<Vec<DeleteState>, String> {
    crate::blocking::off_main(move || {
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app)
            .ok_or("internal: serve state unavailable")?;
        let (models, bundled) = dirs(&app)?;
        states(&models, bundled.as_deref(), &LiveUsage { serve: &serve.0 })
    })
    .await
}

/// **model_delete**: delete one downloaded model by its file name (or `local:<file>` id). The
/// member confirmed it on the Models screen (main window only; never an agent tool).
#[tauri::command]
pub async fn model_delete(app: tauri::AppHandle, file: String) -> Result<Deleted, String> {
    crate::blocking::off_main(move || {
        let serve = tauri::Manager::try_state::<crate::serve::ServeState>(&app)
            .ok_or("internal: serve state unavailable")?;
        let (models, bundled) = dirs(&app)?;
        let usage = LiveUsage { serve: &serve.0 };
        let mut out =
            delete(&models, bundled.as_deref(), &file, &usage).map_err(|e| e.to_string())?;
        // The chat server's selection pointed at the deleted file: fall back to the default path
        // (not started; the Models screen shows nothing selected).
        out.selection_cleared = serve.0.forget_model_if(
            &models.join(&out.file),
            models.join(crate::model::MODEL_FILE),
        );
        Ok(out)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("model_delete_tests.rs");
}
