//! D-11 (RC52) — the Cognition (Devin) remote-agent delegate. ADR-2026-10-26 P-6 and P-8.
//!
//! Hermes may *propose* handing a task to a Devin session. Nothing is sent until the member
//! approves that exact task on an approval card (HIC-1, every task):
//!
//! 1. **Prepare** ([`DelegationBook::prepare`]): core checks the proposed scope (one repository,
//!    the file paths to focus on, the task and the context text) and assembles the exact prompt it
//!    would send ([`assemble_prompt`]). The card shows that prompt word for word, with a hash of
//!    the scope and the prompt.
//! 2. **Approve** ([`DelegationBook::take_approved`]): the member's approval names the delegation
//!    id and echoes the hash. The payload is built only from what core stored at prepare time
//!    ([`session_body`]); the approve call carries no scope, so the model cannot widen it. One
//!    approval sends one session; it expires after [`APPROVAL_TTL_MS`].
//! 3. **Send** through the core model proxy ([`crate::model_proxy::ModelProxy::logged_op`]): the
//!    pinned host, the member's caps and halt, the key from the keychain, a begin and an end
//!    record in the signed usage log.
//! 4. **Results** ([`SessionView`]) are third-party output: the webview hands them to the model
//!    only inside an untrusted-data fence, and the tool is annotated `trust: "untrusted"` so the
//!    session's taint rules apply.
//!
//! No local file is read or uploaded: Devin works on the repository through the member's own Devin
//! GitHub connection; the file paths are named in the prompt only.
//!
//! ## The Devin API (v3, verified 2026-10-26 against the official docs)
//! - Base `https://api.devin.ai/v3`, `Authorization: Bearer cog_…` (service user key or personal
//!   access token): <https://docs.devin.ai/api-reference/authentication>. v1/v2 and `apk_` keys are
//!   deprecated: <https://docs.devin.ai/api-reference/getting-started/migration-guide>.
//! - `GET /v3/self`: the caller's identity and organization, no permission needed (the key test):
//!   <https://docs.devin.ai/api-reference/v3/self/self>.
//! - `POST /v3/organizations/{org_id}/sessions` `{prompt, repos, title, tags}` → `{session_id,
//!   url, status, …}`: <https://docs.devin.ai/api-reference/v3/sessions/post-organizations-sessions>.
//! - `GET /v3/organizations/{org_id}/sessions/{devin_id}` → `status`, `status_detail`,
//!   `pull_requests[{pr_url, pr_state}]`, `structured_output`, `acus_consumed`:
//!   <https://docs.devin.ai/api-reference/v3/sessions/get-organizations-session>.
//! - `GET /v3/organizations/{org_id}/sessions/{devin_id}/messages` → `items[{source, message}]`:
//!   <https://docs.devin.ai/api-reference/v3/sessions/get-organizations-session-messages>.
//!
//! ## Data sources (Rule 7)
//! The key: the keychain (`provider-key:cognition`, set in Settings › Models). Delegations: the
//! in-memory book (pending approvals and started sessions; nothing on disk beyond the usage log
//! and the HIC outbox). Session state and results: the Devin API, live.

use crate::model_provider::{
    AuthPlacement, ChatRequest, ChatResponse, KeyFormat, KeyTest, Method, ModelInfo, ModelProvider,
    OutboundRequest, ProviderCtx, ProviderError, ProviderId,
};
use crate::model_proxy::{ModelProxy, ProxyError, DELEGATE_MODEL};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// A Devin v3 key: a service user key or personal access token, `cog_…` (the v1 `apk_` keys are
/// deprecated and refused). The documentation gives no character set, so any printable ASCII.
pub const COGNITION_KEY_FORMAT: KeyFormat = KeyFormat {
    prefixes: &["cog_"],
    min_len: 20,
    max_len: 512,
    charset: crate::model_provider::KeyCharset::Printable,
};

/// How long an opened approval may be answered.
pub const APPROVAL_TTL_MS: u64 = 10 * 60 * 1000;
/// Open approvals and started delegations kept (oldest dropped first).
pub const MAX_PENDING: usize = 16;
pub const MAX_STARTED: usize = 64;
/// Scope bounds.
pub const MAX_TASK_CHARS: usize = 4000;
pub const MAX_CONTEXT_CHARS: usize = 8000;
pub const MAX_FILES: usize = 50;
pub const MAX_PATH_CHARS: usize = 300;
/// Messages per page (the API's maximum) and the pages one status read may take.
pub const MESSAGE_PAGE: u32 = 200;
pub const MAX_MESSAGE_PAGES: usize = 20;
/// Result bounds (what the model and the card are shown).
pub const MAX_MESSAGES: usize = 20;
pub const MAX_MESSAGE_CHARS: usize = 4000;
pub const MAX_STRUCTURED_BYTES: usize = 16 * 1024;
/// The Devin spend ceiling per delegated task, in ACUs (owner decision 2026-10-27: 5 by default,
/// changeable per task, shown on the card and covered by the approval hash).
pub const DEFAULT_MAX_ACU_LIMIT: u32 = 5;
/// The highest ceiling one task may ask for. Pending owner sign-off.
pub const MAX_ACU_LIMIT: u32 = 100;
/// The tag every delegated session carries.
pub const SESSION_TAG: &str = "citrate-core";

// ---------------------------------------------------------------------------
// The adapter (keys and the key test; never a chat model)
// ---------------------------------------------------------------------------

/// Cognition behind the provider trait, so the key store, the key test and the usage log treat
/// it like the vendors. `complete` always refuses.
pub struct CognitionAdapter;

impl ModelProvider for CognitionAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Cognition
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: DELEGATE_MODEL.to_string(),
            display_name: "Devin (remote agent session)".to_string(),
            supports_tools: false,
            input_micros_per_mtok: None,
            output_micros_per_mtok: None,
        }]
    }

    fn key_format(&self) -> KeyFormat {
        COGNITION_KEY_FORMAT
    }

    /// Never a chat call (`complete` refuses), so nothing is reserved.
    fn output_bound(&self, _req: &ChatRequest) -> u64 {
        0
    }

    fn test_key(&self, ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError> {
        let v = get_json(ctx, "/self")?;
        org_id_from_self(&v)?;
        Ok(KeyTest { models: vec![] })
    }

    fn complete(
        &self,
        _ctx: &ProviderCtx<'_>,
        _req: &ChatRequest,
        _on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        Err(ProviderError::Egress(
            "Cognition runs delegated tasks; it is not a chat model",
        ))
    }
}

fn get_json(ctx: &ProviderCtx<'_>, path: &str) -> Result<Value, ProviderError> {
    send_json(ctx, Method::Get, path, None)
}

fn send_json(
    ctx: &ProviderCtx<'_>,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, ProviderError> {
    let resp = ctx.send(OutboundRequest {
        method,
        path: path.to_string(),
        headers: vec![("accept", "application/json".to_string())],
        auth: AuthPlacement::Bearer,
        body,
    })?;
    if !(200..300).contains(&resp.status) {
        return Err(crate::model_provider::status_error(resp.status));
    }
    resp.json()
}

/// An id Devin issued (`org-…`, `devin-…`), safe to put in a path segment.
pub fn valid_api_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The organization this key's sessions run in (`GET /v3/self`): a service user's `org_id`; a
/// personal access token's `devin_sessions_org_id`, else its `org_id`.
pub fn org_id_from_self(v: &Value) -> Result<String, ProviderError> {
    let pick = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let org = match v.get("principal_type").and_then(Value::as_str) {
        Some("pat_user") => pick("devin_sessions_org_id").or_else(|| pick("org_id")),
        Some(_) => pick("org_id"),
        None => None,
    }
    .ok_or(ProviderError::BadResponse(
        "this Devin key is not in an organization that can run sessions",
    ))?;
    if !valid_api_id(&org) {
        return Err(ProviderError::BadResponse("an unexpected organization id"));
    }
    Ok(org)
}

// ---------------------------------------------------------------------------
// The scope and the exact payload
// ---------------------------------------------------------------------------

/// What Hermes proposes and the member approves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationScope {
    pub task: String,
    /// `owner/name` on the member's Devin-connected git host.
    pub repo: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub context: String,
    /// The most Devin usage (ACUs) this one session may consume (`max_acu_limit`); absent = the
    /// default.
    #[serde(default)]
    pub max_acu_limit: Option<u32>,
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Check and normalize a proposed scope.
pub fn validate_scope(s: &DelegationScope) -> Result<DelegationScope, String> {
    let task = clean(&s.task);
    if task.is_empty() || task.chars().count() > MAX_TASK_CHARS {
        return Err(format!(
            "describe the task in 1 to {MAX_TASK_CHARS} characters"
        ));
    }
    let repo = s.repo.trim().to_string();
    let parts: Vec<&str> = repo.split('/').collect();
    let seg_ok = |p: &str| {
        !p.is_empty()
            && p.len() <= 100
            && p != "."
            && p != ".."
            && p.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if parts.len() != 2 || !parts.iter().all(|p| seg_ok(p)) {
        return Err("name one repository as owner/name".into());
    }
    if s.files.len() > MAX_FILES {
        return Err(format!("name at most {MAX_FILES} files"));
    }
    let mut files = Vec::with_capacity(s.files.len());
    for f in &s.files {
        let f = f.trim();
        let bad = f.is_empty()
            || f.chars().count() > MAX_PATH_CHARS
            || f.starts_with('/')
            || f.contains('\\')
            || f.chars().any(char::is_control)
            || f.split('/').any(|seg| seg == ".." || seg.is_empty());
        if bad {
            return Err(format!(
                "{f:?} is not a file path inside the repository (relative, no ..)"
            ));
        }
        if !files.iter().any(|x: &String| x == f) {
            files.push(f.to_string());
        }
    }
    let limit = s.max_acu_limit.unwrap_or(DEFAULT_MAX_ACU_LIMIT);
    if !(1..=MAX_ACU_LIMIT).contains(&limit) {
        return Err(format!(
            "set a Devin spend ceiling of 1 to {MAX_ACU_LIMIT} ACUs for this task"
        ));
    }
    let context = clean(&s.context);
    if context.chars().count() > MAX_CONTEXT_CHARS {
        return Err(format!(
            "keep the context under {MAX_CONTEXT_CHARS} characters"
        ));
    }
    Ok(DelegationScope {
        task,
        repo,
        files,
        context,
        max_acu_limit: Some(limit),
    })
}

/// The exact prompt sent for a scope. Deterministic: the card shows this text.
pub fn assemble_prompt(s: &DelegationScope) -> String {
    let mut p = format!(
        "Task from a Citrate Core member (approved by them for this session only):\n{}\n\nRepository: {}\n",
        s.task, s.repo
    );
    if !s.files.is_empty() {
        p.push_str("Files to focus on:\n");
        for f in &s.files {
            p.push_str("- ");
            p.push_str(f);
            p.push('\n');
        }
    }
    if !s.context.is_empty() {
        p.push_str("\nContext:\n");
        p.push_str(&s.context);
        p.push('\n');
    }
    p.push_str(
        "\nWork only in this repository. Open a pull request with your changes and do not merge it.",
    );
    p
}

/// A hash over everything the approval covers: the repository, the exact prompt and the spend
/// ceiling (so nothing can raise the ceiling after the card).
pub fn scope_hash(s: &DelegationScope, prompt: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"citrate-core/cognition-delegation/v2\0");
    for part in [s.repo.as_str(), prompt] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    let limit = s.max_acu_limit.map_or(0, u64::from);
    h.update(limit.to_be_bytes());
    hex::encode(h.finalize())
}

/// The `POST /sessions` body: built only from what was approved.
pub fn session_body(p: &Pending) -> Result<Value, ProxyError> {
    let title: String = p.scope.task.chars().take(80).collect();
    // Never sent without a ceiling (the approval always carries one; this is the last check).
    let limit = p
        .scope
        .max_acu_limit
        .filter(|l| (1..=MAX_ACU_LIMIT).contains(l))
        .ok_or_else(|| {
            ProxyError::BadRequest(
                "a delegation needs a Devin spend ceiling; nothing was sent".into(),
            )
        })?;
    Ok(json!({
        "prompt": p.prompt,
        "repos": [p.scope.repo],
        "title": format!("Citrate Core: {title}"),
        "tags": [SESSION_TAG],
        "max_acu_limit": limit,
    }))
}

// ---------------------------------------------------------------------------
// The book
// ---------------------------------------------------------------------------

/// An approval the member has been shown and not answered yet.
#[derive(Debug, Clone)]
pub struct Pending {
    pub id: String,
    pub scope: DelegationScope,
    pub prompt: String,
    pub hash: String,
    pub minted_ms: u64,
}

/// A delegation that was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub id: String,
    pub org_id: String,
    pub devin_id: String,
    pub url: Option<String>,
    pub repo: String,
    pub started_ms: u64,
}

/// What the approval card shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DelegationView {
    pub delegation_id: String,
    pub repo: String,
    pub files: Vec<String>,
    pub task: String,
    pub context: String,
    /// The exact text sent.
    pub prompt: String,
    pub scope_hash: String,
    pub expires_at_ms: u64,
    pub destination: String,
    /// The Devin spend ceiling for this task (ACUs).
    pub max_acu_limit: u32,
}

#[derive(Default)]
pub struct DelegationBook {
    pending: Vec<Pending>,
    started: Vec<Started>,
}

impl DelegationBook {
    /// Check the proposal and keep it for the member's answer.
    pub fn prepare(
        &mut self,
        scope: &DelegationScope,
        now_ms: u64,
        id: String,
    ) -> Result<DelegationView, String> {
        let scope = validate_scope(scope)?;
        let prompt = assemble_prompt(&scope);
        let hash = scope_hash(&scope, &prompt);
        self.pending
            .retain(|p| now_ms.saturating_sub(p.minted_ms) < APPROVAL_TTL_MS);
        if self.pending.len() >= MAX_PENDING {
            self.pending.remove(0);
        }
        let view = DelegationView {
            delegation_id: id.clone(),
            repo: scope.repo.clone(),
            files: scope.files.clone(),
            task: scope.task.clone(),
            context: scope.context.clone(),
            prompt: prompt.clone(),
            scope_hash: hash.clone(),
            expires_at_ms: now_ms.saturating_add(APPROVAL_TTL_MS),
            destination: "Cognition (Devin) · api.devin.ai".to_string(),
            max_acu_limit: scope.max_acu_limit.unwrap_or(DEFAULT_MAX_ACU_LIMIT),
        };
        self.pending.push(Pending {
            id,
            scope,
            prompt,
            hash,
            minted_ms: now_ms,
        });
        Ok(view)
    }

    /// The member approved `id` with the hash the card showed: hand out that stored scope once.
    pub fn take_approved(&mut self, id: &str, hash: &str, now_ms: u64) -> Result<Pending, String> {
        let i =
            self.pending.iter().position(|p| p.id == id).ok_or(
                "that delegation was already used, declined or is unknown; nothing was sent",
            )?;
        let p = self.pending.remove(i);
        if now_ms.saturating_sub(p.minted_ms) >= APPROVAL_TTL_MS || now_ms < p.minted_ms {
            return Err("that approval expired; nothing was sent".into());
        }
        if p.hash != hash {
            return Err(
                "the approval does not match the task that was shown; nothing was sent".into(),
            );
        }
        Ok(p)
    }

    /// The member declined (or the card closed): forget the proposal.
    pub fn decline(&mut self, id: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|p| p.id != id);
        before != self.pending.len()
    }

    pub fn record_started(&mut self, s: Started) {
        if self.started.len() >= MAX_STARTED {
            self.started.remove(0);
        }
        self.started.push(s);
    }

    pub fn started(&self, id: &str) -> Option<Started> {
        self.started.iter().find(|s| s.id == id).cloned()
    }
}

// ---------------------------------------------------------------------------
// Results (untrusted)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestView {
    pub url: String,
    pub state: Option<String>,
}

/// A delegated session as Devin reports it. Every text field is third-party output.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub delegation_id: String,
    pub repo: String,
    pub session_url: Option<String>,
    pub status: String,
    pub status_detail: Option<String>,
    pub pull_requests: Vec<PullRequestView>,
    /// Devin's own messages, oldest first (at most [`MAX_MESSAGES`], each clipped).
    pub messages: Vec<String>,
    /// The session's structured output as JSON text, when it has one (clipped).
    pub structured_output: Option<String>,
    pub acus_consumed: Option<f64>,
    /// Always true: the webview fences all of the above before the model sees it.
    pub untrusted: bool,
}

fn clip(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t} … (clipped)")
    } else {
        t
    }
}

/// A Devin session URL is shown only when it is on Devin's own web app.
pub fn safe_session_url(u: Option<&str>) -> Option<String> {
    let u = u?;
    let parsed = url::Url::parse(u).ok()?;
    let host = parsed.host_str()?;
    (parsed.scheme() == "https" && (host == "app.devin.ai" || host.ends_with(".devin.ai")))
        .then(|| u.to_string())
}

/// Build the view from `GET …/sessions/{id}` and `GET …/messages`.
pub fn session_view(s: &Started, session: &Value, messages: Option<&Value>) -> SessionView {
    let status = session
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let status_detail = session
        .get("status_detail")
        .and_then(Value::as_str)
        .map(str::to_string);
    let pull_requests = session
        .get("pull_requests")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let url = p.get("pr_url").and_then(Value::as_str)?;
            let parsed = url::Url::parse(url).ok()?;
            (parsed.scheme() == "https").then(|| PullRequestView {
                url: clip(url, 500),
                state: p
                    .get("pr_state")
                    .and_then(Value::as_str)
                    .map(|x| clip(x, 40)),
            })
        })
        .take(20)
        .collect();
    let mut msgs: Vec<String> = messages
        .and_then(|m| m.get("items"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("source").and_then(Value::as_str) == Some("devin"))
        .filter_map(|m| m.get("message").and_then(Value::as_str))
        .map(|m| clip(m, MAX_MESSAGE_CHARS))
        .collect();
    if msgs.len() > MAX_MESSAGES {
        msgs.drain(..msgs.len() - MAX_MESSAGES);
    }
    let structured_output = session
        .get("structured_output")
        .filter(|v| !v.is_null())
        .map(|v| {
            let t = v.to_string();
            if t.len() > MAX_STRUCTURED_BYTES {
                clip(&t, MAX_STRUCTURED_BYTES)
            } else {
                t
            }
        });
    SessionView {
        delegation_id: s.id.clone(),
        repo: s.repo.clone(),
        session_url: safe_session_url(
            session
                .get("url")
                .and_then(Value::as_str)
                .or(s.url.as_deref()),
        ),
        status,
        status_detail,
        pull_requests,
        messages: msgs,
        structured_output,
        acus_consumed: session.get("acus_consumed").and_then(Value::as_f64),
        untrusted: true,
    }
}

// ---------------------------------------------------------------------------
// The calls (through the proxy)
// ---------------------------------------------------------------------------

/// The usage-log session label of one delegation.
fn log_session(id: &str) -> String {
    format!("dg-{}", id.trim_start_matches("dg-"))
}

/// Send one approved delegation: `GET /self` for the organization, then `POST …/sessions` with
/// the approved payload. Both are logged calls on the proxy.
pub fn start(proxy: &ModelProxy, p: &Pending, now_ms: u64) -> Result<Started, ProxyError> {
    // The payload (with its ceiling) is complete before anything is sent.
    let body = session_body(p)?;
    let session = log_session(&p.id);
    let org = proxy.logged_op(
        &ProviderId::Cognition,
        DELEGATE_MODEL,
        &session,
        false,
        |ctx| org_id_from_self(&get_json(ctx, "/self")?),
    )?;
    let path = format!("/organizations/{org}/sessions");
    let created = proxy.logged_op(
        &ProviderId::Cognition,
        DELEGATE_MODEL,
        &session,
        false,
        |ctx| send_json(ctx, Method::Post, &path, Some(body)),
    )?;
    let devin_id = created
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| valid_api_id(s))
        .ok_or(ProxyError::Provider(ProviderError::BadResponse(
            "Devin did not return a session id",
        )))?
        .to_string();
    Ok(Started {
        id: p.id.clone(),
        org_id: org,
        devin_id,
        url: safe_session_url(created.get("url").and_then(Value::as_str)),
        repo: p.scope.repo.clone(),
        started_ms: now_ms,
    })
}

/// Read a started delegation's state and Devin's messages (two logged calls).
pub fn status(proxy: &ModelProxy, s: &Started) -> Result<SessionView, ProxyError> {
    let session = log_session(&s.id);
    let base = format!("/organizations/{}/sessions/{}", s.org_id, s.devin_id);
    let info = proxy.logged_op(
        &ProviderId::Cognition,
        DELEGATE_MODEL,
        &session,
        false,
        |ctx| get_json(ctx, &base),
    )?;
    let messages = read_messages(proxy, &session, &base);
    Ok(session_view(s, &info, messages.as_ref()))
}

/// Pages through a session's messages (oldest first, [`MESSAGE_PAGE`] per page, at most
/// [`MAX_MESSAGE_PAGES`] pages) and returns them as one `{items}` answer ending with the latest.
/// A cursor that is not a plain token stops the paging (it would not fit the pinned path); a read
/// that fails returns what was read, or `None` when nothing was.
fn read_messages(proxy: &ModelProxy, session: &str, base: &str) -> Option<Value> {
    let mut items: Vec<Value> = Vec::new();
    let mut after: Option<String> = None;
    let mut any = false;
    for _ in 0..MAX_MESSAGE_PAGES {
        let path = match &after {
            None => format!("{base}/messages?first={MESSAGE_PAGE}"),
            Some(c) => format!("{base}/messages?first={MESSAGE_PAGE}&after={c}"),
        };
        let Ok(page) = proxy.logged_op(
            &ProviderId::Cognition,
            DELEGATE_MODEL,
            session,
            false,
            |ctx| get_json(ctx, &path),
        ) else {
            break;
        };
        any = true;
        items.extend(
            page.get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        // Keep memory bounded: only the latest messages are ever shown.
        if items.len() > MAX_MESSAGES * 4 {
            items.drain(..items.len() - MAX_MESSAGES * 4);
        }
        let next = page
            .get("end_cursor")
            .and_then(Value::as_str)
            .filter(|c| {
                crate::provider_egress::path_allowed(&["?after={cursor}"], &format!("?after={c}"))
            })
            .map(str::to_string);
        match (page.get("has_next_page").and_then(Value::as_bool), next) {
            (Some(true), Some(c)) => after = Some(c),
            _ => break,
        }
    }
    any.then(|| json!({ "items": items }))
}

/// The HIC record of one approved delegation (the decision and whether it was sent).
pub fn hic_event(
    p: &Pending,
    outcome: &Result<Started, ProxyError>,
) -> crate::hic_records::HicEvent {
    use crate::hic_records::{fit, HicEvent};
    let (o, detail) = match outcome {
        Ok(s) => ("completed", format!("Devin session {} started", s.devin_id)),
        Err(e) => ("failed", e.to_string()),
    };
    HicEvent {
        kind: "agent.tool_approval".into(),
        decision: "approved".into(),
        subject: fit(
            &format!(
                "devin_delegate: {} (scope {}, at most {} ACUs)",
                p.scope.repo,
                &p.hash[..16],
                p.scope.max_acu_limit.unwrap_or(0)
            ),
            300,
        ),
        reason: "the member approved this exact task for a Cognition (Devin) session (HIC-1)"
            .into(),
        outcome: Some(o.into()),
        outcome_detail: Some(fit(&detail, 300)),
        evidence: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tauri state and commands
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct DelegationState(pub std::sync::Mutex<DelegationBook>);

fn book<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<std::sync::MutexGuard<'_, DelegationBook>, String> {
    use tauri::Manager;
    let st = app
        .try_state::<DelegationState>()
        .ok_or("internal: delegation state unavailable")?;
    // The guard borrows managed state, which lives as long as the app.
    let st: &DelegationState = st.inner();
    Ok(st.0.lock().unwrap_or_else(|e| e.into_inner()))
}

fn proxy_for<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<std::sync::Arc<ModelProxy>, String> {
    use tauri::Manager;
    app.try_state::<crate::providers_ipc::ModelProxyState>()
        .ok_or("internal: model proxy state unavailable")?
        .get(app)
}

/// **delegation_prepare** — Hermes proposed a delegation; the approval card is opening. Core
/// checks the scope and returns the exact prompt it would send. Nothing is sent. Refused under
/// Local model only, and when no Devin key is connected.
#[tauri::command]
pub async fn delegation_prepare(
    app: tauri::AppHandle,
    scope: DelegationScope,
) -> Result<DelegationView, String> {
    let route_dir = crate::chat_route::route_dir(&app)?;
    crate::blocking::off_main(move || {
        crate::chat_route::remote_chat_allowed(&route_dir)?;
        if !proxy_for(&app)?.key_configured(&ProviderId::Cognition) {
            return Err(
                "No Devin key is connected, so nothing can be delegated. Connect Cognition in Settings › Models."
                    .into(),
            );
        }
        let id = format!("dg-{:016x}", rand::random::<u64>());
        book(&app)?.prepare(&scope, crate::model_proxy::now_ms(), id)
    })
    .await
}

/// **delegation_decline** — the card closed without approval.
#[tauri::command]
pub async fn delegation_decline(
    app: tauri::AppHandle,
    delegation_id: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        book(&app)?.decline(&delegation_id);
        Ok(())
    })
    .await
}

/// What an approved delegation returns.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartedView {
    pub delegation_id: String,
    pub repo: String,
    pub session_url: Option<String>,
}

/// **delegation_approve** — the member approved the card for `delegation_id` with the hash it
/// showed. Core sends exactly the stored payload (never anything from this call), records the
/// decision in the HIC outbox and the call in the usage log.
#[tauri::command]
pub async fn delegation_approve(
    app: tauri::AppHandle,
    delegation_id: String,
    scope_hash: String,
) -> Result<StartedView, String> {
    let route_dir = crate::chat_route::route_dir(&app)?;
    crate::blocking::off_main(move || {
        crate::chat_route::remote_chat_allowed(&route_dir)?;
        let outbox = crate::hic_records::outbox_for_app(&app)?;
        outbox.check_writable()?;
        let now = crate::model_proxy::now_ms();
        let pending = book(&app)?.take_approved(&delegation_id, &scope_hash, now)?;
        let proxy = proxy_for(&app)?;
        let outcome = start(&proxy, &pending, now);
        if let Err(e) = crate::hic_records::record_for_app(&app, hic_event(&pending, &outcome)) {
            eprintln!(
                "citrate-core: delegation {} was not recorded in the decision records: {e}",
                pending.id
            );
        }
        let started = outcome.map_err(|e| e.to_string())?;
        let view = StartedView {
            delegation_id: started.id.clone(),
            repo: started.repo.clone(),
            session_url: started.url.clone(),
        };
        book(&app)?.record_started(started);
        Ok(view)
    })
    .await
}

/// **delegation_status** — a started delegation's state and Devin's messages, live. Every text
/// field is untrusted third-party output.
#[tauri::command]
pub async fn delegation_status(
    app: tauri::AppHandle,
    delegation_id: String,
) -> Result<SessionView, String> {
    let route_dir = crate::chat_route::route_dir(&app)?;
    crate::blocking::off_main(move || {
        crate::chat_route::remote_chat_allowed(&route_dir)?;
        let s = book(&app)?
            .started(&delegation_id)
            .ok_or("no delegation with that id was started in this app session")?;
        let proxy = proxy_for(&app)?;
        status(&proxy, &s).map_err(|e| e.to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("cognition_tests.rs");
}
