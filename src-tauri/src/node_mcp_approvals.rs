//! HUP-S4.2 — the approval inbox for write requests made over the citrate-node MCP server.
//!
//! An MCP client can only ASK. Every write lands here as a pending request the member reviews in
//! Settings. A signature request carries the id of a SignatureCeremony that was opened when the
//! request arrived; approving it runs that ceremony (the one signing path). An action request
//! (join a cluster, share a file, create or revoke an invite) runs only after the member approves.
//!
//! Properties this module guarantees (each pinned by a test):
//! - a request is decided at most once (`begin_decision` moves it out of `pending` first);
//! - a client can read only its own requests (ownership is the connect-token id);
//! - pending requests expire after [`REQUEST_TTL_MS`] and can no longer be approved;
//! - revoking a token closes that token's pending requests;
//! - at most [`MAX_PENDING`] requests wait at once, so a client cannot flood the member.

use serde::Serialize;
use serde_json::Value;

/// How long a request waits for the member before it expires (15 minutes).
pub const REQUEST_TTL_MS: u64 = 15 * 60 * 1000;
/// The most requests that may be pending at once, across all clients.
pub const MAX_PENDING: usize = 16;
/// How many requests (pending + decided) are kept for display and polling.
pub const MAX_KEPT: usize = 64;

/// A non-signing change an MCP client asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum McpAction {
    ClusterJoin { group: String },
    ClusterShare { group: String, cid: String },
    InviteCreate { group: String, for_handle: String },
    InviteRevoke { group: String, invite_id: String },
}

impl McpAction {
    /// The human summary shown in the approval card.
    pub fn summary(&self) -> String {
        match self {
            McpAction::ClusterJoin { group } => format!("Join this node to the cluster of group {group}"),
            McpAction::ClusterShare { group, cid } => {
                format!("Share file {cid} with the cluster of group {group}")
            }
            McpAction::InviteCreate { group, for_handle } => format!(
                "Create a one-time invite to group {group} for {for_handle}. The agent will receive the invite link."
            ),
            McpAction::InviteRevoke { group, invite_id } => {
                format!("Revoke invite {invite_id} for group {group}")
            }
        }
    }
}

/// What the request is.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestKind {
    /// A transaction waiting in the SignatureCeremony. `ceremony` is the ceremony's decoded view
    /// (action, cost, destination, true origin, raw-ack flag) exactly as the ceremony produced it.
    Signature {
        ceremony_id: String,
        ceremony: Value,
    },
    /// A non-signing change, run by core after approval.
    Action { action: McpAction },
}

/// Where a request is in its life.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RequestState {
    Pending,
    /// The member approved and core is running it.
    Running,
    Approved {
        result: Value,
    },
    Rejected {
        reason: String,
    },
    Failed {
        error: String,
    },
    Expired,
}

/// One request.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRequest {
    pub id: String,
    /// The connect-token id that made the request (ownership for `request_status`).
    pub token_id: String,
    /// The origin shown to the member (token label + client name), verbatim.
    pub origin: String,
    pub summary: String,
    #[serde(flatten)]
    pub kind: RequestKind,
    #[serde(flatten)]
    pub state: RequestState,
    pub created_ms: u64,
    pub decided_ms: Option<u64>,
}

/// Something the caller must do after an inbox transition (close a ceremony that will never be
/// approved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeremonyToClose(pub String);

/// The inbox.
#[derive(Default)]
pub struct ApprovalInbox {
    inner: std::sync::Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    requests: Vec<McpRequest>,
}

impl ApprovalInbox {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Expire stale pending requests. Returns the ceremonies the caller must reject.
    fn expire_locked(inner: &mut Inner, now_ms: u64) -> Vec<CeremonyToClose> {
        let mut close = Vec::new();
        for r in inner.requests.iter_mut() {
            if r.state == RequestState::Pending
                && now_ms.saturating_sub(r.created_ms) >= REQUEST_TTL_MS
            {
                r.state = RequestState::Expired;
                r.decided_ms = Some(now_ms);
                if let RequestKind::Signature { ceremony_id, .. } = &r.kind {
                    close.push(CeremonyToClose(ceremony_id.clone()));
                }
            }
        }
        close
    }

    /// Drop the oldest DECIDED requests beyond [`MAX_KEPT`] (pending ones are never dropped).
    fn trim_locked(inner: &mut Inner) {
        while inner.requests.len() > MAX_KEPT {
            match inner
                .requests
                .iter()
                .position(|r| !matches!(r.state, RequestState::Pending | RequestState::Running))
            {
                Some(i) => {
                    inner.requests.remove(i);
                }
                None => break,
            }
        }
    }

    /// Whether another request may be queued now (checked BEFORE a ceremony is opened, so a full
    /// inbox never leaves an orphan ceremony). Also expires stale requests.
    pub fn has_room(&self, now_ms: u64) -> (bool, Vec<CeremonyToClose>) {
        let mut inner = self.lock();
        let close = Self::expire_locked(&mut inner, now_ms);
        let pending = inner
            .requests
            .iter()
            .filter(|r| r.state == RequestState::Pending)
            .count();
        (pending < MAX_PENDING, close)
    }

    /// Queue a request. Fails when [`MAX_PENDING`] are already waiting.
    pub fn submit(
        &self,
        token_id: &str,
        origin: &str,
        kind: RequestKind,
        summary: String,
        now_ms: u64,
    ) -> Result<(McpRequest, Vec<CeremonyToClose>), String> {
        let mut inner = self.lock();
        let close = Self::expire_locked(&mut inner, now_ms);
        let pending = inner
            .requests
            .iter()
            .filter(|r| r.state == RequestState::Pending)
            .count();
        if pending >= MAX_PENDING {
            return Err(format!(
                "{MAX_PENDING} requests are already waiting for the member. Ask them to review the requests in Citrate Core first."
            ));
        }
        inner.next += 1;
        let req = McpRequest {
            id: format!("mcpr-{}", inner.next),
            token_id: token_id.to_string(),
            origin: origin.to_string(),
            summary,
            kind,
            state: RequestState::Pending,
            created_ms: now_ms,
            decided_ms: None,
        };
        inner.requests.push(req.clone());
        Self::trim_locked(&mut inner);
        Ok((req, close))
    }

    /// A client's view of its own request. `None` when the id is unknown OR belongs to another
    /// token (the two are indistinguishable to the caller).
    pub fn status_for(
        &self,
        id: &str,
        token_id: &str,
        now_ms: u64,
    ) -> (Option<McpRequest>, Vec<CeremonyToClose>) {
        let mut inner = self.lock();
        let close = Self::expire_locked(&mut inner, now_ms);
        let found = inner
            .requests
            .iter()
            .find(|r| r.id == id && r.token_id == token_id)
            .cloned();
        (found, close)
    }

    /// Every kept request, newest first (the Settings panel).
    pub fn list(&self, now_ms: u64) -> (Vec<McpRequest>, Vec<CeremonyToClose>) {
        let mut inner = self.lock();
        let close = Self::expire_locked(&mut inner, now_ms);
        let mut v = inner.requests.clone();
        v.reverse();
        (v, close)
    }

    /// Start deciding request `id`: it must be pending (not expired). Moves it to `Running` (on
    /// approve) or `Rejected` (on reject) atomically, so a second decision on the same id fails.
    pub fn begin_decision(
        &self,
        id: &str,
        approve: bool,
        now_ms: u64,
    ) -> Result<(McpRequest, Vec<CeremonyToClose>), String> {
        let mut inner = self.lock();
        let close = Self::expire_locked(&mut inner, now_ms);
        let r = inner
            .requests
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("no request {id}"))?;
        if r.state != RequestState::Pending {
            return Err(format!("request {id} is no longer waiting for a decision"));
        }
        r.state = if approve {
            RequestState::Running
        } else {
            RequestState::Rejected {
                reason: "rejected by the member".to_string(),
            }
        };
        r.decided_ms = Some(now_ms);
        Ok((r.clone(), close))
    }

    /// Record the outcome of an approved request that was `Running`.
    pub fn finish(
        &self,
        id: &str,
        outcome: Result<Value, String>,
        now_ms: u64,
    ) -> Option<McpRequest> {
        let mut inner = self.lock();
        let r = inner.requests.iter_mut().find(|r| r.id == id)?;
        if r.state != RequestState::Running {
            return None;
        }
        r.state = match outcome {
            Ok(result) => RequestState::Approved { result },
            Err(error) => RequestState::Failed { error },
        };
        r.decided_ms = Some(now_ms);
        let out = r.clone();
        Self::trim_locked(&mut inner);
        Some(out)
    }

    /// Close every pending request made with token `token_id` (the token was revoked).
    pub fn revoke_token(&self, token_id: &str, now_ms: u64) -> Vec<CeremonyToClose> {
        let mut inner = self.lock();
        let mut close = Self::expire_locked(&mut inner, now_ms);
        for r in inner.requests.iter_mut() {
            if r.token_id == token_id && r.state == RequestState::Pending {
                r.state = RequestState::Rejected {
                    reason: "the connect token was revoked".to_string(),
                };
                r.decided_ms = Some(now_ms);
                if let RequestKind::Signature { ceremony_id, .. } = &r.kind {
                    close.push(CeremonyToClose(ceremony_id.clone()));
                }
            }
        }
        close
    }
}
