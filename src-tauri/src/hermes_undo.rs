//! HUP-S2.9 — undo for agent file changes, core's half.
//!
//! The sidecar's file tools (`fs_write`, `fs_edit`, `fs_delete`, `fs_rename`) take an undo
//! checkpoint around every change, keyed by the agent session id. This module calls the sidecar's
//! bearer-gated `/checkpoints` routes so the member can list a session's recent changes and undo
//! one of them or all of them, from the change card in the chat or from the Activity monitor.
//!
//! Data source (Rule 7): the sidecar's checkpoint store, a directory under the app's local data
//! (`<app_local_data>/hermes/checkpoints`, passed to the child as `CITRATE_HERMES_CHECKPOINTS`).
//! The agent cannot reach it: the default-deny list covers Citrate Core app data.
//!
//! Every refusal (a path changed since the step, the step was pruned, undo is not enabled, an older
//! sidecar without the routes) comes back as an [`UndoOutcome`] with `ok: false`, its kind and the
//! sidecar's reason, so the UI shows it as is. Nothing here writes a file itself, holds a key or
//! signs (Rule 3).
//!
//! The file tools stay off by default: core sets neither `CITRATE_HERMES_FILES` nor a grants file,
//! because there is no Grants screen yet and turning agent writes on is pending owner sign-off.

use serde::{Deserialize, Serialize};

use super::{valid_session_id, ControlResp, HermesError, HermesManager, Result};

/// Env the Hermes child reads its undo checkpoint store directory from.
pub const HERMES_CHECKPOINTS_ENV: &str = "CITRATE_HERMES_CHECKPOINTS";

/// One checkpointed step, as the sidecar lists it (newest first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointStep {
    pub seq: u64,
    /// `prepared` | `committed` | `interrupted` | `undone`.
    pub status: String,
    /// Paths touched, relative to `root`.
    pub paths: Vec<String>,
    /// The granted folder the paths are relative to.
    pub root: String,
}

/// A session's checkpointed steps. `enabled: false` with a `note` when the sidecar cannot undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointList {
    pub session: String,
    pub enabled: bool,
    pub steps: Vec<CheckpointStep>,
    pub note: Option<String>,
}

/// One path that changed since the step an undo would restore it from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoConflict {
    pub seq: u64,
    pub path: String,
    /// What is on disk now (`file sha256:…`, `absent`, `directory`).
    pub found: String,
}

/// What an undo did, or why it did nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoOutcome {
    pub ok: bool,
    /// Steps undone, newest first (empty on a refusal).
    pub undone: Vec<u64>,
    /// Paths restored (empty on a refusal).
    pub restored: Vec<String>,
    /// For a session undo: steps at or below this were pruned earlier and could not be undone.
    pub pruned_through: Option<u64>,
    /// On a refusal: `conflict`, `busy`, `already_undone`, `not_found`, `pruned`, `disabled`,
    /// `invalid` or `unsupported`.
    pub kind: Option<String>,
    /// On a refusal: the reason in words, shown as is.
    pub reason: Option<String>,
    pub conflicts: Vec<UndoConflict>,
}

const UNSUPPORTED: &str =
    "this agent sidecar does not support undo yet; update Citrate Core to get it";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListBody {
    session: String,
    steps: Vec<CheckpointStep>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UndoBody {
    undone: Vec<u64>,
    restored: Vec<String>,
    #[serde(default)]
    pruned_through: Option<u64>,
}

#[derive(Deserialize)]
struct RefusalBody {
    error: String,
    kind: String,
    #[serde(default)]
    conflicts: Vec<UndoConflict>,
}

/// A sidecar refusal (`{error, kind}` on 4xx/503), or a 404 without one (an older sidecar).
fn refusal(resp: &ControlResp) -> Option<RefusalBody> {
    if !matches!(resp.status, 400 | 404 | 409 | 410 | 503) {
        return None;
    }
    match serde_json::from_str::<RefusalBody>(&resp.body) {
        Ok(r) => Some(r),
        Err(_) if resp.status == 404 => Some(RefusalBody {
            error: UNSUPPORTED.into(),
            kind: "unsupported".into(),
            conflicts: Vec::new(),
        }),
        Err(_) => None,
    }
}

fn bad_request(msg: impl Into<String>) -> HermesError {
    HermesError::Control {
        status: 400,
        msg: msg.into(),
    }
}

fn decode<T: serde::de::DeserializeOwned>(resp: &ControlResp) -> Result<T> {
    if !(200..300).contains(&resp.status) {
        return Err(HermesError::Control {
            status: resp.status,
            msg: resp.body.chars().take(200).collect(),
        });
    }
    serde_json::from_str(&resp.body).map_err(|e| HermesError::Decode(e.to_string()))
}

fn outcome(resp: &ControlResp) -> Result<UndoOutcome> {
    if let Some(r) = refusal(resp) {
        return Ok(UndoOutcome {
            ok: false,
            undone: Vec::new(),
            restored: Vec::new(),
            pruned_through: None,
            kind: Some(r.kind),
            reason: Some(r.error),
            conflicts: r.conflicts,
        });
    }
    let b: UndoBody = decode(resp)?;
    Ok(UndoOutcome {
        ok: true,
        undone: b.undone,
        restored: b.restored,
        pruned_through: b.pruned_through,
        kind: None,
        reason: None,
        conflicts: Vec::new(),
    })
}

impl HermesManager {
    /// Point the child at its undo checkpoint store (`CITRATE_HERMES_CHECKPOINTS`). Prod passes
    /// `<app_local_data>/hermes/checkpoints`. Absent: the env is not set and undo is off.
    pub fn with_checkpoints_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.checkpoints_dir = Some(dir);
        self
    }

    fn checkpoints_url(&self, id: &str, tail: &str) -> Result<String> {
        valid_session_id(id).map_err(bad_request)?;
        Ok(format!("{}/checkpoints/{id}{tail}", self.control_url()))
    }

    /// `GET /checkpoints/:id` — the session's steps, newest first.
    pub fn checkpoints_list(&self, id: &str) -> Result<CheckpointList> {
        let url = self.checkpoints_url(id, "")?;
        let bearer = self.bearer()?;
        let resp = self.control.get(&url, &bearer)?;
        if let Some(r) = refusal(&resp) {
            if r.kind == "disabled" || r.kind == "unsupported" {
                return Ok(CheckpointList {
                    session: id.to_string(),
                    enabled: false,
                    steps: Vec::new(),
                    note: Some(r.error),
                });
            }
            return Err(HermesError::Control {
                status: resp.status,
                msg: r.error.chars().take(200).collect(),
            });
        }
        let b: ListBody = decode(&resp)?;
        Ok(CheckpointList {
            session: b.session,
            enabled: true,
            steps: b.steps,
            note: None,
        })
    }

    /// `POST /checkpoints/:id/steps/:seq/undo` — undo one step.
    pub fn undo_step(&self, id: &str, seq: u64) -> Result<UndoOutcome> {
        if seq == 0 {
            return Err(bad_request("the step must be a positive whole number"));
        }
        let url = self.checkpoints_url(id, &format!("/steps/{seq}/undo"))?;
        let bearer = self.bearer()?;
        outcome(&self.control.post(&url, &bearer, "{}")?)
    }

    /// `POST /checkpoints/:id/undo` — undo every step of the session not undone yet (all or nothing).
    pub fn undo_session(&self, id: &str) -> Result<UndoOutcome> {
        let url = self.checkpoints_url(id, "/undo")?;
        let bearer = self.bearer()?;
        outcome(&self.control.post(&url, &bearer, "{}")?)
    }
}

/// **hermes_checkpoints** — the agent session's recent file changes that can be undone.
#[tauri::command]
pub async fn hermes_checkpoints(
    app: tauri::AppHandle,
    id: String,
) -> std::result::Result<CheckpointList, String> {
    crate::blocking::off_main(move || {
        super::manager(&app)?
            .checkpoints_list(&id)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_undo_step** — undo one agent file change (a member action; a conflict is refused).
#[tauri::command]
pub async fn hermes_undo_step(
    app: tauri::AppHandle,
    id: String,
    seq: u64,
) -> std::result::Result<UndoOutcome, String> {
    crate::blocking::off_main(move || {
        super::manager(&app)?
            .undo_step(&id, seq)
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_undo_session** — undo every agent file change of the session (all or nothing).
#[tauri::command]
pub async fn hermes_undo_session(
    app: tauri::AppHandle,
    id: String,
) -> std::result::Result<UndoOutcome, String> {
    crate::blocking::off_main(move || {
        super::manager(&app)?
            .undo_session(&id)
            .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_undo_tests.rs");
}
