//! HUP-S2.3: live Sign-In with Ethereum: core's side of the managed browser's sign-in bridge.
//!
//! ADR-2026-09-30-rule3-budgetable-signatures (accepted), D2 and D6. Hermes's managed browser
//! (citrate-agent-runtime `agent-browser`, on only with `CITRATE_HERMES_BROWSER=1`) gives the
//! page a provider whose `eth_requestAccounts` and `personal_sign` wait in the sidecar. This module
//! collects such a request, decides it in core, and hands the answer back:
//!
//! 1. **Read the request from the sidecar, never from the webview.** The webview only names the
//!    request id. The message, the asking page and what the sessions have read come from the
//!    sidecar's `GET /browser/sign-in` over core's own bearer channel. The webview cannot supply
//!    or alter taint, `hic`, the message or the origin (this closes the "caller-supplied taint"
//!    gap the S2.3 review recorded).
//! 2. **Attest the origin itself (D2 #1).** Core reads the managed browser's loopback DevTools
//!    endpoint (`/json/list`) directly and takes the top-level origin of the tab Hermes drives. It
//!    must equal the origin Chrome gave the asking context, or the request is not attested (a page
//!    that navigated in between gets a card). Attach mode (the member's own Chrome) is never
//!    attested as managed (D2 #3).
//! 3. **Decide in the ceremony.** `eth_requestAccounts`: the address is shared only with a site
//!    that has a live budget for the active wallet; otherwise the page is told no. `personal_sign`:
//!    `SignatureCeremony::request_siwe_budgeted` signs inside a live budget or returns a pending
//!    HIC-1 card. A message that is not UTF-8 text becomes an ordinary card.
//! 4. **Deliver.** The signature (or the refusal) goes back over `POST /browser/sign-in/answer`.
//!    A card is delivered after the member approves it (`web_signing_approve`) or declined
//!    (`web_signing_reject`).
//!
//! Every web-signing decision record is then copied into the local decision records the nightly
//! anchor covers (`POST /records/web-signing`, US-2.3 AC3); see [`export_records`].
//!
//! Residual (recorded in docs/WEB_SIGNING_BUDGETS.md): which frame raised the request comes from
//! Chrome's execution-context events as the sidecar reports them; core verifies the tab's
//! top-level origin and that it matches the asking context's origin, but it does not hold its own
//! DevTools session on the frame tree.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use citrate_core_kit::ceremony::{
    BudgetedOutcome, CeremonyError, CeremonyView, IntentKind, SignatureCeremony, SignatureIntent,
    SiweSignRequest,
};
use citrate_core_kit::custody::CustodyVault;
use citrate_core_kit::web_budget::{
    BrowserMode, BudgetGate, DecisionRecord, FrameKind, OriginAttestation, RecordKind,
    RecordStatus, TaskTaint, DEFAULT_PRINCIPAL,
};
use serde::{Deserialize, Serialize};

/// Longest message accepted from the bridge (core budgets at most 2048 bytes).
pub const MAX_MESSAGE_BYTES: usize = 4096;
/// At most this many sign-in cards wait for the member at once.
pub const MAX_PENDING_CARDS: usize = 8;
/// How long core waits for the browser's DevTools list.
pub const DEVTOOLS_TIMEOUT: Duration = Duration::from_secs(3);
/// Records sent per call to the decision-records route.
pub const EXPORT_BATCH: usize = 100;
/// The Tauri event the "Signed for you" notice listens to.
pub const AUTO_SIGNED_EVENT: &str = "web-budget://auto-signed";

/// EIP-1193 codes sent to the page.
pub const CODE_USER_REJECTED: i64 = 4001;
pub const CODE_UNAUTHORIZED: i64 = 4100;

// ---------------------------------------------------------------------------------------------
// Ports (production: the Hermes manager and the loopback DevTools endpoint)

/// The sidecar's bearer-authed control channel.
pub trait SidecarLink {
    /// `(status, body)` of a GET.
    fn get(&self, path: &str) -> Result<(u16, String), String>;
    /// `(status, body)` of a JSON POST.
    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String>;
}

impl SidecarLink for crate::hermes::HermesManager {
    fn get(&self, path: &str) -> Result<(u16, String), String> {
        self.control_get_path(path)
            .map(|r| (r.status, r.body))
            .map_err(|e| e.to_string())
    }
    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        self.control_post_path(path, body)
            .map(|r| (r.status, r.body))
            .map_err(|e| e.to_string())
    }
}

/// Core's own read of the managed browser's DevTools target list.
pub trait DevtoolsReader {
    fn targets(&self, port: u16) -> Result<Vec<DevtoolsTarget>, String>;
}

/// One entry of `/json/list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DevtoolsTarget {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
}

/// `GET http://127.0.0.1:<port>/json/list`, loopback only, no redirects, bounded.
pub struct LoopbackDevtools;

impl DevtoolsReader for LoopbackDevtools {
    fn targets(&self, port: u16) -> Result<Vec<DevtoolsTarget>, String> {
        if port < 1024 {
            return Err("the browser's DevTools port is not a loopback user port".to_string());
        }
        let resp = ureq::get(&format!("http://127.0.0.1:{port}/json/list"))
            .config()
            .timeout_global(Some(DEVTOOLS_TIMEOUT))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("the browser's DevTools endpoint did not answer: {e}"))?;
        if resp.status().as_u16() != 200 {
            return Err("the browser's DevTools endpoint refused the read".to_string());
        }
        let body = resp
            .into_body()
            .with_config()
            .limit(512 * 1024)
            .read_to_string()
            .map_err(|e| format!("the browser's DevTools list could not be read: {e}"))?;
        serde_json::from_str(&body)
            .map_err(|_| "the browser's DevTools list is not what Chrome sends".to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// The sidecar's view

/// The taint the sidecar computed from its live sessions (never from the webview).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaintWire {
    Clean,
    Sources { sources: Vec<String> },
    Unknown,
}

impl TaintWire {
    pub fn to_task_taint(&self) -> TaskTaint {
        match self {
            TaintWire::Clean => TaskTaint::Clean,
            TaintWire::Sources { sources } if sources.is_empty() => TaskTaint::Unknown,
            TaintWire::Sources { sources } => TaskTaint::Sources(sources.clone()),
            TaintWire::Unknown => TaskTaint::Unknown,
        }
    }
}

/// One waiting request, as the sidecar reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRequest {
    pub id: String,
    /// `accounts` or `personal_sign`.
    pub kind: String,
    pub raise_origin: String,
    pub top_frame: bool,
    #[serde(default)]
    pub message_hex: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

/// `GET /browser/sign-in`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSnapshot {
    pub mode: String,
    #[serde(default)]
    pub devtools_port: Option<u16>,
    #[serde(default)]
    pub target_id: Option<String>,
    pub requests: Vec<BridgeRequest>,
    pub taint: TaintWire,
}

fn reason_of(body: &str, status: u16) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| format!("the sidecar answered {status}"))
}

pub fn read_snapshot(link: &dyn SidecarLink) -> Result<BridgeSnapshot, String> {
    let (status, body) = link.get("/browser/sign-in")?;
    if status != 200 {
        return Err(reason_of(&body, status));
    }
    serde_json::from_str(&body).map_err(|_| "the sidecar's sign-in list is malformed".to_string())
}

fn web_origin(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let o = u.origin();
    o.is_tuple().then(|| o.ascii_serialization())
}

/// D2 #1-3: the tab's top-level origin as core reads it from the browser itself, checked against
/// the origin Chrome gave the asking context. `Err` (with the reason) means not attested.
pub fn attest(
    snap: &BridgeSnapshot,
    req: &BridgeRequest,
    devtools: &dyn DevtoolsReader,
) -> Result<OriginAttestation, String> {
    if snap.mode != "managed" {
        return Err("budgets apply only in Hermes's managed browser".to_string());
    }
    let port = snap
        .devtools_port
        .ok_or("the managed browser did not report its DevTools port")?;
    let target = snap
        .target_id
        .as_deref()
        .filter(|t| !t.is_empty() && t.len() <= 128)
        .ok_or("the managed browser did not report its tab")?;
    let list = devtools.targets(port)?;
    let tab = list
        .iter()
        .find(|t| t.id == target && t.kind == "page")
        .ok_or("Hermes's tab was not found in the browser")?;
    let origin = web_origin(&tab.url).ok_or("the tab is not on a web page")?;
    if origin != req.raise_origin {
        return Err(
            "the page changed before Citrate Core could check which site asked".to_string(),
        );
    }
    Ok(OriginAttestation {
        origin,
        frame: if req.top_frame {
            FrameKind::Top
        } else {
            FrameKind::Sub
        },
        mode: BrowserMode::Managed,
    })
}

// ---------------------------------------------------------------------------------------------
// Pending cards (ceremony id -> bridge request id)

/// Which bridge request each pending sign-in card answers.
#[derive(Default)]
pub struct SignInState {
    cards: Mutex<BTreeMap<String, String>>,
}

impl SignInState {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        self.cards.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn remember(&self, ceremony_id: &str, request_id: &str) {
        let mut g = self.lock();
        while g.len() >= MAX_PENDING_CARDS {
            let Some(first) = g.keys().next().cloned() else {
                break;
            };
            g.remove(&first);
        }
        g.insert(ceremony_id.to_string(), request_id.to_string());
    }
    fn take(&self, ceremony_id: &str) -> Option<String> {
        self.lock().remove(ceremony_id)
    }
    pub fn is_sign_in_card(&self, ceremony_id: &str) -> bool {
        self.lock().contains_key(ceremony_id)
    }
}

// ---------------------------------------------------------------------------------------------
// Handling one request

/// What happened to a request (crosses the bridge to the main window).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SignInOutcome {
    /// Signed inside the member's budget and delivered to the page.
    AutoSigned {
        origin: String,
        remaining: u32,
        #[serde(rename = "recordId")]
        record_id: u64,
        #[serde(rename = "budgetId")]
        budget_id: u64,
        delivered: bool,
    },
    /// Waiting for the member at the Signature Ceremony.
    Pending {
        ceremony: CeremonyView,
        reason: String,
    },
    /// The member's address was shared with a budgeted site.
    AddressShared { origin: String },
    /// The page was told no (with the reason the member sees).
    Refused { reason: String },
}

fn answer(link: &dyn SidecarLink, body: serde_json::Value) -> Result<(), String> {
    let (status, text) = link.post("/browser/sign-in/answer", &body.to_string())?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(reason_of(&text, status))
    }
}

fn refuse(link: &dyn SidecarLink, id: &str, code: i64, message: &str) -> Result<(), String> {
    answer(
        link,
        serde_json::json!({"id": id, "refused": {"code": code, "message": message}}),
    )
}

fn decode_hex(h: &str) -> Option<Vec<u8>> {
    let h = h.strip_prefix("0x").unwrap_or(h);
    if !h.len().is_multiple_of(2) || h.len() / 2 > MAX_MESSAGE_BYTES {
        return None;
    }
    hex::decode(h).ok()
}

/// Everything core needs to decide one request.
pub struct SignInCtx<'a> {
    pub link: &'a dyn SidecarLink,
    pub devtools: &'a dyn DevtoolsReader,
    pub ceremony: &'a SignatureCeremony,
    pub vault: &'a CustodyVault,
    pub gate: &'a BudgetGate,
    pub state: &'a SignInState,
    pub clock: &'a dyn Fn() -> u64,
}

/// Decide the bridge request `request_id` (see the module docs).
pub fn handle(cx: &SignInCtx<'_>, request_id: &str) -> Result<SignInOutcome, String> {
    let snap = read_snapshot(cx.link)?;
    let req = snap
        .requests
        .iter()
        .find(|r| r.id == request_id)
        .cloned()
        .ok_or("that sign-in request is no longer waiting")?;
    let attestation = attest(&snap, &req, cx.devtools);
    let wallet = citrate_core_kit::wallet::address(cx.vault)
        .ok()
        .map(|w| w.address);
    match req.kind.as_str() {
        "accounts" => {
            let shared = attestation.as_ref().ok().and_then(|a| {
                if a.frame != FrameKind::Top {
                    return None;
                }
                cx.gate
                    .live_budget_for(
                        &a.origin,
                        DEFAULT_PRINCIPAL,
                        wallet.as_deref(),
                        (cx.clock)(),
                    )
                    .map(|_| a.origin.clone())
            });
            let address = wallet.as_deref().and_then(citrate_core_kit::siwe::to_eip55);
            match (shared, address) {
                (Some(origin), Some(addr)) => {
                    answer(
                        cx.link,
                        serde_json::json!({"id": req.id, "accounts": [addr]}),
                    )?;
                    Ok(SignInOutcome::AddressShared { origin })
                }
                _ => {
                    let reason = "Citrate shares your address only with a site you gave a sign-in budget in Settings, Budgets, and only in Hermes's managed browser.".to_string();
                    refuse(cx.link, &req.id, CODE_UNAUTHORIZED, &reason)?;
                    Ok(SignInOutcome::Refused { reason })
                }
            }
        }
        "personal_sign" => {
            let Some(bytes) = req.message_hex.as_deref().and_then(decode_hex) else {
                let reason = "the message is missing or too long".to_string();
                refuse(cx.link, &req.id, CODE_USER_REJECTED, &reason)?;
                return Ok(SignInOutcome::Refused { reason });
            };
            let claimed: String = req
                .raise_origin
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            let outcome = match String::from_utf8(bytes.clone()) {
                Ok(message) => cx.ceremony.request_siwe_budgeted(
                    cx.vault,
                    cx.gate,
                    SiweSignRequest {
                        message,
                        attestation: attestation.as_ref().ok().cloned(),
                        taint: snap.taint.to_task_taint(),
                        // The request came from a page, not from a tool call; the taint above
                        // carries what the sessions read.
                        hic_required: false,
                        principal: DEFAULT_PRINCIPAL.to_string(),
                        claimed_origin: claimed.clone(),
                    },
                    cx.clock,
                ),
                Err(_) => BudgetedOutcome::Pending {
                    ceremony: cx.ceremony.request(SignatureIntent {
                        origin: match &attestation {
                            Ok(a) => a.origin.clone(),
                            Err(_) => format!("{claimed} (site not verified by Citrate Core)"),
                        },
                        kind: IntentKind::PersonalSign,
                        chain_id: citrate_core_kit::rpc::CITRATE_CHAIN_ID,
                        raw: hex::encode(&bytes),
                    }),
                    reason: "this is not a sign-in message, so it needs your approval".to_string(),
                },
            };
            match outcome {
                BudgetedOutcome::AutoSigned {
                    signature,
                    record_id,
                    budget_id,
                    remaining,
                    origin,
                } => {
                    let delivered = answer(
                        cx.link,
                        serde_json::json!({"id": req.id, "signature": format!("0x{}", signature.sig_hex)}),
                    )
                    .is_ok();
                    Ok(SignInOutcome::AutoSigned {
                        origin,
                        remaining,
                        record_id,
                        budget_id,
                        delivered,
                    })
                }
                BudgetedOutcome::Pending { ceremony, reason } => {
                    cx.state.remember(&ceremony.id, &req.id);
                    Ok(SignInOutcome::Pending { ceremony, reason })
                }
            }
        }
        _ => {
            let reason = "this request is not supported".to_string();
            refuse(cx.link, &req.id, CODE_USER_REJECTED, &reason)?;
            Ok(SignInOutcome::Refused { reason })
        }
    }
}

/// The member approved a sign-in card: sign through the ceremony and deliver the signature to
/// the page. Returns whether the page received it (it may have navigated away).
pub fn approve(
    link: &dyn SidecarLink,
    ceremony: &SignatureCeremony,
    vault: &CustodyVault,
    state: &SignInState,
    ceremony_id: &str,
    raw_ack: bool,
) -> Result<bool, String> {
    if !state.is_sign_in_card(ceremony_id) {
        return Err("that approval is not a sign-in request".to_string());
    }
    let sig = match ceremony.approve(vault, ceremony_id, raw_ack) {
        Ok(s) => s,
        // The member can still tick the raw acknowledgement and retry: keep the card.
        Err(e @ CeremonyError::RawAckRequired) => return Err(e.to_string()),
        Err(e) => {
            if let Some(r) = state.take(ceremony_id) {
                let _ = refuse(
                    link,
                    &r,
                    CODE_USER_REJECTED,
                    "The sign-in could not be completed in Citrate.",
                );
            }
            return Err(e.to_string());
        }
    };
    let request_id = state
        .take(ceremony_id)
        .ok_or("that approval is not a sign-in request")?;
    Ok(answer(
        link,
        serde_json::json!({"id": request_id, "signature": format!("0x{}", sig.sig_hex)}),
    )
    .is_ok())
}

/// The member declined a sign-in card: nothing is signed and the page is told no.
pub fn reject(
    link: &dyn SidecarLink,
    ceremony: &SignatureCeremony,
    state: &SignInState,
    ceremony_id: &str,
) -> Result<(), String> {
    let request_id = state
        .take(ceremony_id)
        .ok_or("that approval is not a sign-in request")?;
    let _ = ceremony.reject(ceremony_id);
    // The page may be gone already; declining still holds.
    let _ = refuse(
        link,
        &request_id,
        CODE_USER_REJECTED,
        "You declined the sign-in in Citrate.",
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// US-2.3 AC3: copy the web-signing records into the decision records the nightly anchor covers

fn kind_str(k: RecordKind) -> &'static str {
    match k {
        RecordKind::BudgetGranted => "budget_granted",
        RecordKind::BudgetRevoked => "budget_revoked",
        RecordKind::AllBudgetsRevoked => "all_budgets_revoked",
        RecordKind::StoreReset => "store_reset",
        RecordKind::AutoSign => "auto_sign",
    }
}

fn status_str(s: RecordStatus) -> &'static str {
    match s {
        RecordStatus::Reserved => "reserved",
        RecordStatus::Signed => "signed",
        RecordStatus::NotSigned => "not_signed",
        RecordStatus::OutcomeUnknown => "outcome_unknown",
        RecordStatus::Final => "final",
    }
}

/// The wire form `POST /records/web-signing` takes for one record.
pub fn record_wire(r: &DecisionRecord) -> serde_json::Value {
    serde_json::json!({
        "recordId": r.record_id,
        "kind": kind_str(r.kind),
        "status": status_str(r.status),
        "origin": if r.origin.is_empty() { "(all sites)" } else { r.origin.as_str() },
        "budgetId": r.budget_id,
        "payloadDigest": r.payload_digest,
        "nonce": r.nonce,
        "statement": r.statement,
        "signerAddress": r.signer_address,
        "atMs": r.at_ms,
        "hash": r.hash,
    })
}

/// What an export pass did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportResult {
    /// This many records were copied.
    Exported(usize),
    /// The sidecar has no records folder configured (anchoring is off): nothing to do.
    NotConfigured,
}

/// Copy every closed, not yet copied record, oldest first, advancing the cursor after each
/// accepted batch. At least once: a crash between the copy and the cursor save repeats a batch;
/// the core record id in each copy identifies repeats.
pub fn export_records(link: &dyn SidecarLink, gate: &BudgetGate) -> Result<ExportResult, String> {
    let mut total = 0usize;
    loop {
        let batch = gate.records_to_export(EXPORT_BATCH);
        let Some(last) = batch.last().map(|r| r.record_id) else {
            return Ok(ExportResult::Exported(total));
        };
        let body =
            serde_json::json!({ "records": batch.iter().map(record_wire).collect::<Vec<_>>() });
        let (status, text) = link.post("/records/web-signing", &body.to_string())?;
        match status {
            200..=299 => {}
            404 => return Ok(ExportResult::NotConfigured),
            _ => return Err(reason_of(&text, status)),
        }
        gate.mark_exported(last).map_err(|e| e.to_string())?;
        total += batch.len();
    }
}

#[cfg(test)]
#[path = "web_signin_tests.rs"]
mod tests;
