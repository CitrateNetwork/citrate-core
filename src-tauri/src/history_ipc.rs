//! AMC-H1 (0.5.3, P-11): the conversation history commands.
//!
//! **Data source (Rule 7), for every command here:** the local history store,
//! `<app data>/history/` (catalog, conversation logs and artifact blobs, sealed with AES-256-GCM),
//! opened with the history key in the OS keyring (`ai.citrate.core` / `history-kek-v1`). Nothing
//! is read from or sent to the network. `history_start_chat` also reads the provider's model from
//! core's sealed provider config (`ai.rs`); `history_export` writes only into a live write folder
//! grant the member chose (`agent_grants.rs`), like a media copy.
//!
//! While the feature gate is off (`history_recorder::ENABLE_ENV`, the 0.5.2 default) every command
//! except `history_status` answers [`crate::history_recorder::OFF_MESSAGE`].

use base64::Engine as _;
use serde::Serialize;

use crate::history::{Conversation, Hit, Record, Summary};
use crate::history_recorder::{now_ms, HistoryState};

fn state(app: &tauri::AppHandle) -> Result<tauri::State<'_, HistoryState>, String> {
    tauri::Manager::try_state::<HistoryState>(app)
        .ok_or_else(|| "internal: history state unavailable".to_string())
}

fn with_store<T>(
    app: &tauri::AppHandle,
    f: impl FnOnce(&crate::history::HistoryStore) -> crate::history::Result<T>,
) -> Result<T, String> {
    let st = state(app)?;
    let store = st.store()?;
    f(store).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatusView {
    /// The feature gate.
    pub enabled: bool,
    /// The store opened and its catalog reads.
    pub available: bool,
    /// Why it is not available, when it is not.
    pub reason: Option<String>,
    pub conversations: Option<usize>,
    pub retention_days: Option<u32>,
    pub key_generation: Option<u64>,
}

/// The status view for a state (pure, so the off and failed cases are testable).
pub fn status_view(st: &HistoryState) -> HistoryStatusView {
    let unavailable = |reason: String| HistoryStatusView {
        enabled: st.enabled(),
        available: false,
        reason: Some(reason),
        conversations: None,
        retention_days: None,
        key_generation: None,
    };
    match st.store() {
        Err(reason) => unavailable(reason),
        Ok(s) => match s.status() {
            Err(e) => unavailable(e.to_string()),
            Ok(x) => HistoryStatusView {
                enabled: true,
                available: true,
                reason: None,
                conversations: Some(x.conversations),
                retention_days: x.retention_days,
                key_generation: Some(x.key_generation),
            },
        },
    }
}

/// **history_status** — whether history is on and readable, how many conversations it holds,
/// the retention setting and the key generation.
#[tauri::command]
pub async fn history_status(app: tauri::AppHandle) -> Result<HistoryStatusView, String> {
    crate::blocking::off_main(move || Ok(status_view(&*state(&app)?))).await
}

/// **history_list** — every conversation's summary, most recent first.
#[tauri::command]
pub async fn history_list(app: tauri::AppHandle) -> Result<Vec<Summary>, String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.list())).await
}

/// **history_read** — one conversation with all its records (untrusted ones are flagged).
#[tauri::command]
pub async fn history_read(app: tauri::AppHandle, id: String) -> Result<Conversation, String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.read(&id))).await
}

/// **history_search** — titles and record text, decrypted in memory.
#[tauri::command]
pub async fn history_search(
    app: tauri::AppHandle,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<Hit>, String> {
    crate::blocking::off_main(move || {
        with_store(&app, |s| s.search(&query, limit.unwrap_or(50) as usize))
    })
    .await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub sha256: String,
    pub untrusted: bool,
    pub data_base64: String,
}

/// **history_artifact_read** — an artifact's bytes (checked against its recorded hash).
#[tauri::command]
pub async fn history_artifact_read(
    app: tauri::AppHandle,
    id: String,
    artifact_id: String,
) -> Result<ArtifactView, String> {
    crate::blocking::off_main(move || {
        let (entry, bytes) = with_store(&app, |s| s.read_artifact(&id, &artifact_id))?;
        match entry.record {
            Record::Artifact {
                id,
                name,
                mime,
                size,
                sha256,
                ..
            } => Ok(ArtifactView {
                id,
                name,
                mime,
                size,
                sha256,
                untrusted: entry.untrusted,
                data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            }),
            _ => Err("that is not an artifact".into()),
        }
    })
    .await
}

/// **history_artifact_add** — the member attaches a file to a conversation (origin: member).
/// The bytes come from the member's own pick in the webview; they are sealed like every other
/// artifact and capped at `history::MAX_ARTIFACT_BYTES`. Returns the artifact id.
#[tauri::command]
pub async fn history_artifact_add(
    app: tauri::AppHandle,
    id: String,
    name: String,
    mime: String,
    data_base64: String,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        // Refuse an oversized upload before decoding it (base64 is 4/3 of the bytes).
        if data_base64.len() > crate::history::MAX_ARTIFACT_BYTES / 3 * 4 + 4 {
            return Err("that file is larger than an artifact can be".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_base64.as_bytes())
            .map_err(|_| "that file's data is not base64".to_string())?;
        with_store(&app, |s| {
            s.put_artifact(
                &id,
                crate::history::NewArtifact {
                    name: &name,
                    mime: &mime,
                    origin: crate::history::Origin::Member,
                    bytes: &bytes,
                },
                now_ms(),
            )
        })
        .map(|r| r.id)
    })
    .await
}

/// **history_rename** — the member renames a conversation.
#[tauri::command]
pub async fn history_rename(
    app: tauri::AppHandle,
    id: String,
    title: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.rename(&id, &title))).await
}

/// **history_delete** — delete a conversation so it cannot be read back (key erase plus file
/// removal, see `history.rs`).
#[tauri::command]
pub async fn history_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.delete(&id))).await
}

/// **history_set_retention** — keep conversations `days` after their last activity (`null`:
/// until deleted), then purge what is already past it. Returns the purged ids.
#[tauri::command]
pub async fn history_set_retention(
    app: tauri::AppHandle,
    days: Option<u32>,
) -> Result<Vec<String>, String> {
    crate::blocking::off_main(move || {
        with_store(&app, |s| {
            s.set_retention(days)?;
            s.enforce_retention(now_ms())
        })
    })
    .await
}

/// **history_rotate_key** — reseal the catalog under a new history key. Returns its generation.
#[tauri::command]
pub async fn history_rotate_key(app: tauri::AppHandle) -> Result<u64, String> {
    crate::blocking::off_main(move || with_store(&app, |s| s.rotate_key())).await
}

/// **history_export** — write a readable copy (`.md` and `.json`) of one conversation into a
/// write folder grant the member picked. Returns the two paths.
#[tauri::command]
pub async fn history_export(
    app: tauri::AppHandle,
    id: String,
    grant_id: String,
) -> Result<Vec<String>, String> {
    crate::blocking::off_main(move || {
        let export = with_store(&app, |s| s.export(&id))?;
        let grants = crate::media::grant_state(&app)?;
        let root = crate::media::target_root(&grants, &grant_id, now_ms() / 1000)?;
        let stamp = now_ms() / 1000;
        let md = crate::media::write_new_file(
            &root,
            &format!("{}-{stamp}.md", export.stem),
            export.markdown.as_bytes(),
        )?;
        let json = crate::media::write_new_file(
            &root,
            &format!("{}-{stamp}.json", export.stem),
            export.json.as_bytes(),
        )?;
        Ok(vec![md.display().to_string(), json.display().to_string()])
    })
    .await
}

/// **history_start_chat** — start a chat conversation for `provider_id` (`local` for the local
/// model). Core resolves the model from its own state; the webview names only the provider id.
/// The returned id is passed back on `ai_chat` / `ai_chat_local` as `conversationId`.
#[tauri::command]
pub async fn history_start_chat(
    app: tauri::AppHandle,
    provider_id: String,
    title: Option<String>,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let st = state(&app)?;
        st.store()?;
        let rec = st
            .recorder()
            .ok_or_else(|| crate::history_recorder::OFF_MESSAGE.to_string())?;
        let model = crate::history_recorder::resolve_model(&app, &provider_id)
            .ok_or("that provider is not configured")?;
        rec.start_chat(&provider_id, &model, title.as_deref(), now_ms())
            .map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("history_ipc_tests.rs");
}
