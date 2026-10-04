//! HUP-S9.4 (n5): the eval gate's two runs inside the app, against the app's own llama-server.
//!
//! The eval CLIs (`scripts/eval-tools.mjs`) need Node and a terminal. Here the same deterministic
//! scorers (`src/agent/eval/runner.ts`) run in the app, and core runs the model:
//!
//! 1. **Begin.** Core hashes the candidate adapter (it must be the sha256 the member was given,
//!    and GGUF), copies it to `<app data>/adapters/eval/<sha256>.gguf`, and restarts llama-server
//!    with `--lora-scaled <sha256>.gguf:0` (run in that directory). Scale 0 means a request that
//!    does not name the adapter is served by the base model alone, so the member's chats are never
//!    answered by an adapter that has not passed the gate. (Measured on the bundled build 10909:
//!    `--lora-init-without-apply` does NOT do this, an omitted request still applies the adapter;
//!    `--lora-scaled <file>:0` does.) Core then reads `GET /lora-adapters` with the session key
//!    and refuses to go on unless it lists exactly this file at scale 0. An adapter the member had
//!    loaded is suspended for the run.
//! 2. **Complete.** The runner's requests go through core, which adds `lora: [{id: 0, scale}]`:
//!    0 for the base arm, 1 for the candidate arm, temperature 0, and counts each arm's answers.
//! 3. **Finish.** The runner hands back both scorecards. Core requires each arm's count to equal
//!    the scorecard's `n` (no scorecard core did not see run, no best-of-several), stamps the
//!    candidate with the session's adapter hash, and applies the same gate decision as the CLI
//!    path (`fl_rounds::decide_eval_gate`). The record is bound to the content-addressed copy.
//!    The server is then restarted without the candidate.
//!
//! Only the tool-call + injection suite runs in the app. The QA suite needs the docs tree on disk
//! and stays on the CLI path. Nothing here holds a key or signs (Rule 3); the llama-server API key
//! is the per-session key `serve` already holds.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::fl_rounds::{decide_eval_gate, parse_tools_scorecard, AdapterGateRecord, EvalPair};

/// The longest one eval completion may take (matches the CLI's request timeout).
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(180);
/// How long `begin` waits for the restarted server to answer `/health`.
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

/// Which run a request belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    Base,
    Candidate,
}

impl Arm {
    fn scale(self) -> f64 {
        match self {
            Arm::Base => 0.0,
            Arm::Candidate => 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalSessionInfo {
    pub session_id: String,
    pub adapter_sha256: String,
    /// The served model the runs measure (file stem), which the scorecards must name.
    pub model: String,
    pub started_at_ms: u64,
    pub base_calls: u64,
    pub candidate_calls: u64,
}

struct Session {
    info: EvalSessionInfo,
    copy: PathBuf,
}

/// Managed state: at most one eval run.
#[derive(Default)]
pub struct FlEval {
    inner: Mutex<Option<Session>>,
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn model_stem(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    lower.strip_suffix(".gguf").unwrap_or(&lower).to_string()
}

fn hash_gguf(path: &Path) -> Result<(String, bool), String> {
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut head = Vec::with_capacity(4);
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if head.len() < 4 {
            let take = (4 - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        h.update(&buf[..n]);
    }
    Ok((hex::encode(h.finalize()), head.as_slice() == b"GGUF"))
}

/// Check the candidate and copy it to `<eval_dir>/<sha256>.gguf`, re-hashing the copy.
pub fn stage_candidate(
    src: &Path,
    expected_sha256: &str,
    eval_dir: &Path,
) -> Result<PathBuf, String> {
    let sha = expected_sha256.trim().to_ascii_lowercase();
    if !is_sha256_hex(&sha) {
        return Err("the expected adapter hash must be a sha256 hex string (64 characters)".into());
    }
    let (actual, is_gguf) = hash_gguf(src)?;
    if !is_gguf {
        return Err("the adapter is not a GGUF file".into());
    }
    if actual != sha {
        return Err(format!(
            "the adapter's sha256 {actual} does not match the expected {sha}"
        ));
    }
    std::fs::create_dir_all(eval_dir).map_err(|e| e.to_string())?;
    let dest = eval_dir.join(format!("{sha}.gguf"));
    let part = eval_dir.join(format!("{sha}.gguf.part"));
    std::fs::copy(src, &part).map_err(|e| format!("cannot copy the adapter for the eval ({e})"))?;
    match hash_gguf(&part) {
        Ok((h, true)) if h == sha => {}
        _ => {
            let _ = std::fs::remove_file(&part);
            return Err("the adapter changed while it was copied; try again".into());
        }
    }
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// The `/v1/chat/completions` body for one eval request: the runner's messages and tools as
/// given (the same body the CLI sends), plus the arm's adapter scale.
pub fn completion_body(
    model: &str,
    messages_json: &str,
    tools_json: &str,
    arm: Arm,
) -> Result<Value, String> {
    let messages: Value =
        serde_json::from_str(messages_json).map_err(|_| "messages are not JSON".to_string())?;
    if !messages.is_array() {
        return Err("messages must be a JSON array".into());
    }
    let tools: Value =
        serde_json::from_str(tools_json).map_err(|_| "tools are not JSON".to_string())?;
    if !tools.is_array() {
        return Err("tools must be a JSON array".into());
    }
    Ok(serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "temperature": 0,
        "lora": [{ "id": 0, "scale": arm.scale() }],
    }))
}

#[derive(Deserialize)]
struct WireAdapter {
    id: u64,
    path: String,
    scale: f64,
}

/// `GET /lora-adapters` must list exactly the candidate (by file name), as id 0, at scale 0.
pub fn check_lora_adapters(body: &str, expected_file: &str) -> Result<(), String> {
    let list: Vec<WireAdapter> = serde_json::from_str(body)
        .map_err(|_| "llama-server's adapter list is not the expected shape".to_string())?;
    let [a] = list.as_slice() else {
        return Err(format!(
            "llama-server lists {} adapters; the eval run needs exactly the candidate",
            list.len()
        ));
    };
    let file = Path::new(&a.path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    if a.id != 0 || file != expected_file {
        return Err("llama-server is not serving the candidate adapter as expected".into());
    }
    if a.scale != 0.0 {
        return Err(format!(
            "llama-server applies the candidate at scale {} by default; chats must stay on the base model (scale 0)",
            a.scale
        ));
    }
    Ok(())
}

fn local(port: u16, path: &str) -> String {
    format!("http://127.0.0.1:{port}{path}")
}

/// POST one eval request to the local llama-server; returns `choices[0].message` as JSON.
pub fn post_completion(port: u16, api_key: &str, body: &Value) -> Result<String, String> {
    let mut resp = ureq::post(&local(port, "/v1/chat/completions"))
        .config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(COMPLETION_TIMEOUT))
        .build()
        .header("authorization", &format!("Bearer {api_key}"))
        .send_json(body)
        .map_err(|e| format!("the local model could not be reached ({e})"))?;
    let code = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| format!("could not read the local model's reply ({e})"))?;
    if !(200..300).contains(&code) {
        return Err(format!("the local model answered HTTP {code}"));
    }
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| "the local model's reply is not JSON".to_string())?;
    let msg = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .filter(|m| m.is_object())
        .ok_or_else(|| "the local model's reply has no choices[0].message".to_string())?;
    Ok(msg.to_string())
}

/// `GET /lora-adapters` with the session key.
pub fn get_lora_adapters(port: u16, api_key: &str) -> Result<String, String> {
    let mut resp = ureq::get(&local(port, "/lora-adapters"))
        .config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .header("authorization", &format!("Bearer {api_key}"))
        .call()
        .map_err(|e| format!("the local model could not be reached ({e})"))?;
    let code = resp.status().as_u16();
    if !(200..300).contains(&code) {
        return Err(format!(
            "llama-server answered HTTP {code} for its adapter list"
        ));
    }
    resp.body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// Wait for `/health` to answer 200.
pub fn wait_healthy(port: u16, timeout: Duration) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        let ok = ureq::get(&local(port, "/health"))
            .config()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(2)))
            .build()
            .call()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err("the local model did not come back after the restart".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

impl FlEval {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Open a run for a staged candidate. Refused while another run is open.
    pub fn begin(
        &self,
        sha: &str,
        model_file: &str,
        copy: PathBuf,
        now_ms: u64,
    ) -> Result<EvalSessionInfo, String> {
        let mut g = self.lock();
        if g.is_some() {
            return Err("an eval run is in progress; finish or end it first".into());
        }
        let mut r = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut r);
        let info = EvalSessionInfo {
            session_id: hex::encode(r),
            adapter_sha256: sha.to_ascii_lowercase(),
            model: model_stem(model_file),
            started_at_ms: now_ms,
            base_calls: 0,
            candidate_calls: 0,
        };
        *g = Some(Session {
            info: info.clone(),
            copy,
        });
        Ok(info)
    }

    pub fn current(&self) -> Option<EvalSessionInfo> {
        self.lock().as_ref().map(|s| s.info.clone())
    }

    fn check_id<'a>(g: &'a mut Option<Session>, id: &str) -> Result<&'a mut Session, String> {
        match g.as_mut() {
            Some(s) if s.info.session_id == id => Ok(s),
            _ => Err("no eval run with that id is in progress".into()),
        }
    }

    /// The session's model + copy, for a request.
    pub fn session(&self, id: &str) -> Result<(EvalSessionInfo, PathBuf), String> {
        let mut g = self.lock();
        let s = Self::check_id(&mut g, id)?;
        Ok((s.info.clone(), s.copy.clone()))
    }

    /// Count one answered request.
    pub fn note_call(&self, id: &str, arm: Arm) -> Result<(), String> {
        let mut g = self.lock();
        let s = Self::check_id(&mut g, id)?;
        match arm {
            Arm::Base => s.info.base_calls += 1,
            Arm::Candidate => s.info.candidate_calls += 1,
        }
        Ok(())
    }

    /// Close the run without a decision.
    pub fn end(&self, id: &str) -> Result<(), String> {
        let mut g = self.lock();
        Self::check_id(&mut g, id)?;
        *g = None;
        Ok(())
    }

    /// Decide from the two scorecards. On success the run is closed; on a refusal it stays open
    /// so the caller can end it.
    pub fn finish(
        &self,
        id: &str,
        base_json: &str,
        candidate_json: &str,
        served_model_file: &str,
        now_ms: u64,
    ) -> Result<AdapterGateRecord, String> {
        let mut g = self.lock();
        let s = Self::check_id(&mut g, id)?;
        let info = s.info.clone();
        let base = parse_tools_scorecard(base_json)?;
        let mut cand = parse_tools_scorecard(candidate_json)?;
        if model_stem(served_model_file) != info.model {
            return Err(format!(
                "the served model changed during the eval run (now {served_model_file}); start again"
            ));
        }
        if model_stem(&base.model) != info.model || model_stem(&cand.model) != info.model {
            return Err(format!(
                "the scorecards measured model {} / {}, not the served model {}",
                base.model, cand.model, info.model
            ));
        }
        if base.adapter_sha256.is_some() {
            return Err("the base run's scorecard is stamped with an adapter".into());
        }
        match cand.adapter_sha256.as_deref() {
            None => cand.adapter_sha256 = Some(info.adapter_sha256.clone()),
            Some(a) if a == info.adapter_sha256 => {}
            Some(_) => {
                return Err("the candidate scorecard is stamped with a different adapter".into())
            }
        }
        if info.base_calls != base.n {
            return Err(format!(
                "the base scorecard reports {} items but core answered {} base requests",
                base.n, info.base_calls
            ));
        }
        if info.candidate_calls != cand.n {
            return Err(format!(
                "the candidate scorecard reports {} items but core answered {} candidate requests",
                cand.n, info.candidate_calls
            ));
        }
        let pair = EvalPair {
            base_tools: base,
            candidate_tools: cand,
            base_qa: None,
            candidate_qa: None,
        };
        let decision = decide_eval_gate(&info.adapter_sha256, &pair);
        let rec = AdapterGateRecord {
            adapter_sha256: info.adapter_sha256.clone(),
            adapter_path: s.copy.to_string_lossy().to_string(),
            base_model: pair.base_tools.model.clone(),
            decided_at_ms: now_ms,
            decision,
        };
        *g = None;
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

fn eval_state(app: &tauri::AppHandle) -> Result<tauri::State<'_, FlEval>, String> {
    tauri::Manager::try_state::<FlEval>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())
}

fn serve_state(
    app: &tauri::AppHandle,
) -> Result<tauri::State<'_, crate::serve::ServeState>, String> {
    tauri::Manager::try_state::<crate::serve::ServeState>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())
}

/// The run in progress, for the overview.
pub fn current_session(app: &tauri::AppHandle) -> Option<EvalSessionInfo> {
    tauri::Manager::try_state::<FlEval>(app).and_then(|s| s.current())
}

/// Take the candidate out of llama-server again (best effort on an error path).
fn restore_serving(app: &tauri::AppHandle) -> Result<(), String> {
    let serve = serve_state(app)?;
    serve.0.set_eval_lora(None);
    crate::fl_rounds::restart_serving(app, &serve)
}

/// **Command — fl_eval_begin.** Stage the candidate and restart the local model with it loaded at
/// scale 0. Needs the local model running.
#[tauri::command]
pub async fn fl_eval_begin(
    app_h: tauri::AppHandle,
    adapter_path: String,
    expected_sha256: String,
) -> Result<EvalSessionInfo, String> {
    crate::blocking::off_main(move || {
        let fe = eval_state(&app_h)?;
        let serve = serve_state(&app_h)?;
        if fe.current().is_some() {
            return Err("an eval run is in progress; finish or end it first".into());
        }
        if !serve.0.is_running() {
            return Err("start the local model first; the eval runs on it".into());
        }
        let dir = crate::fl_rounds::adapter_store(&app_h)?.join("eval");
        let copy = stage_candidate(Path::new(adapter_path.trim()), &expected_sha256, &dir)?;
        let sha = expected_sha256.trim().to_ascii_lowercase();
        let model_file = serve.0.current_model_file();
        let info = fe.begin(&sha, &model_file, copy.clone(), crate::fl_rounds::now_ms())?;
        serve.0.set_eval_lora(Some(copy));
        let ready = crate::fl_rounds::restart_serving(&app_h, &serve)
            .and_then(|_| wait_healthy(serve.0.port(), READY_TIMEOUT))
            .and_then(|_| get_lora_adapters(serve.0.port(), &serve.0.api_key()))
            .and_then(|list| check_lora_adapters(&list, &format!("{sha}.gguf")));
        if let Err(e) = ready {
            let _ = fe.end(&info.session_id);
            let _ = restore_serving(&app_h);
            return Err(e);
        }
        Ok(info)
    })
    .await
}

/// **Command — fl_eval_complete.** One eval request on the given arm. Returns the assistant
/// message JSON.
#[tauri::command]
pub async fn fl_eval_complete(
    app_h: tauri::AppHandle,
    session_id: String,
    arm: Arm,
    messages_json: String,
    tools_json: String,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let fe = eval_state(&app_h)?;
        let serve = serve_state(&app_h)?;
        let (info, copy) = fe.session(&session_id)?;
        if serve.0.eval_lora().as_deref() != Some(copy.as_path()) {
            return Err(
                "the eval run was interrupted (the local model changed); end it and start again"
                    .into(),
            );
        }
        let body = completion_body(
            &serve.0.current_model_file(),
            &messages_json,
            &tools_json,
            arm,
        )?;
        let msg = post_completion(serve.0.port(), &serve.0.api_key(), &body)?;
        fe.note_call(&info.session_id, arm)?;
        Ok(msg)
    })
    .await
}

/// **Command — fl_eval_finish.** Decide from the two scorecards, record the verdict, and take the
/// candidate out of the local model.
#[tauri::command]
pub async fn fl_eval_finish(
    app_h: tauri::AppHandle,
    session_id: String,
    base_scorecard_json: String,
    candidate_scorecard_json: String,
) -> Result<AdapterGateRecord, String> {
    crate::blocking::off_main(move || {
        let fe = eval_state(&app_h)?;
        let serve = serve_state(&app_h)?;
        let fl = crate::fl_rounds::state(&app_h)?;
        let (_, copy) = fe.session(&session_id)?;
        if serve.0.eval_lora().as_deref() != Some(copy.as_path()) {
            return Err(
                "the eval run was interrupted (the local model changed); end it and start again"
                    .into(),
            );
        }
        let rec = fe.finish(
            &session_id,
            &base_scorecard_json,
            &candidate_scorecard_json,
            &serve.0.current_model_file(),
            crate::fl_rounds::now_ms(),
        )?;
        fl.record_gate(rec.clone())?;
        // The loaded adapter (suspended during the run) comes back unless this record revoked it.
        let store = crate::fl_rounds::adapter_store(&app_h)?;
        if crate::fl_rounds::must_unload_after_gate(
            &rec,
            serve.0.lora().as_deref(),
            &store,
            &serve.0.current_model_file(),
        ) {
            serve.0.set_lora(None);
        }
        restore_serving(&app_h)?;
        Ok(rec)
    })
    .await
}

/// **Command — fl_eval_end.** Close the run without a decision and take the candidate out.
#[tauri::command]
pub async fn fl_eval_end(app_h: tauri::AppHandle, session_id: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let fe = eval_state(&app_h)?;
        fe.end(&session_id)?;
        restore_serving(&app_h)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fl_eval_tests.rs");
}
