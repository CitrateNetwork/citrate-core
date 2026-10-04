//! HUP-S6.5 (core half): `faucet_request`: deploy-gas top-ups from the Citrate faucet, routed
//! through core so no key ever lives in a sidecar.
//!
//! Design: `docs/adr/ADR-2026-10-01-faucet-for-deploy-gas.md` (**proposed**; owner questions
//! O-1 to O-4 are open). Everything here is **off by default** and changes nothing for a member
//! until they turn it on in Settings → Budgets, which is the HIC-1 grant of an HIC-2 budget
//! (ADR D4.3, D5). The values this build uses are conservative placeholders pending owner
//! sign-off ([`PENDING_OWNER_SIGN_OFF`]).
//!
//! ## What happens on a request (ADR D3, D4)
//! 1. The recipient is the member's own wallet address, read by core. No caller chooses it.
//! 2. The request is tied to a deploy the member has started: the deploy gate must hold a READY
//!    record for the init code hash (HUP-S6.4). Nothing is requested speculatively.
//! 3. Need = the deploy's gas limit ([`DEPLOY_GAS_LIMIT`], the same limit `contract_deploy`
//!    puts on the transaction) × the live gas price. If the balance already covers it, nothing
//!    is requested.
//! 4. The member's budget allows one faucet request per 24 h ([`MEMBER_WINDOW_MS`]), shared by
//!    every caller (the app, Hermes, an MCP client). A refusal from the faucet is recorded with
//!    its next eligible time and never retried in a loop (ADR D4.4).
//! 5. Core sends one unsigned `POST {faucet}/faucet {"address": …}`. The faucet signs and pays
//!    the drip with its own operations key. Nothing here signs anything.
//!
//! ## Data sources (Rule 7)
//! - Balance and gas price: `eth_getBalance` and `eth_gasPrice` on the public 40204 RPC
//!   (`citrate_core_kit::rpc::CITRATE_RPC_URL`).
//! - Faucet state: `GET {faucet}/ready` (readiness, citrate-chain faucet with HUP-S6.5), falling
//!   back to `GET {faucet}/health` for an older faucet; `GET {faucet}/eligibility?address=`.
//! - The drip: `POST {faucet}/faucet`. Its JSON reply (`success`, `tx_hash`, `message`, and on a
//!   refusal `code`, `limit`, `next_eligible_at`) is interpreted by [`interpret_reply`].
//! - The member's choices and history: `<app data>/faucet-budget.json` (0600).
//! - Decision records (ADR D4.3): every grant, revoke and faucet call becomes one record in
//!   core's HIC outbox (`crate::hic_records`), which the nightly anchor batches. Kinds:
//!   `faucet.budget_granted`, `faucet.budget_revoked` (the member's HIC-1 decisions) and
//!   `faucet.topup` (`approved` when the member asked from the app, `auto_within_budget` (HIC-2)
//!   when Hermes or an MCP client asked inside the member's budget).
//!
//! ## Honest states
//! Disabled, wallet changed since the grant, no deploy started, balance already enough, waiting
//! for the next eligible time, the faucet wants a CAPTCHA (the member solves it in an in-app
//! window on the faucet's own page), refused (with the faucet's reason), unreachable. Each one is
//! shown as what it is; none pretends the drip happened.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The faucet the app talks to when nothing overrides it.
pub const DEFAULT_FAUCET_URL: &str = "https://faucet.citrate.ai";
/// Developer/operator override (an https URL, or http on loopback only).
pub const FAUCET_URL_ENV: &str = "CITRATE_FAUCET_URL";
/// The member's budget and request history, in the app data dir.
pub const STATE_FILE_NAME: &str = "faucet-budget.json";
/// One faucet request per member per this window (ADR D4.2). Placeholder pending owner sign-off.
pub const MEMBER_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// Requests allowed per window (ADR D4.3: "cap: one per 24 h"). Placeholder pending owner sign-off.
pub const MAX_PER_WINDOW: u32 = 1;
/// The gas limit `contract_deploy` puts on a deploy (one source of truth).
pub const DEPLOY_GAS_LIMIT: u64 = crate::contract_deploy::DEFAULT_DEPLOY_GAS;
/// What the faucet sends per drip today (informational; the faucet decides, ADR O-1).
pub const DRIP_WEI: u128 = 10_000_000_000_000_000_000;
/// History kept in the state file.
pub const MAX_LEDGER: usize = 200;
/// Longest faucet message kept or shown.
const MAX_MESSAGE_CHARS: usize = 300;
/// Largest faucet reply read.
const MAX_BODY_BYTES: u64 = 64 * 1024;
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// The in-app challenge window's label. No capability names it, so the faucet page it shows has
/// no route to any app command (enforced by a test).
pub const CHALLENGE_WINDOW_LABEL: &str = "faucet-challenge";
/// The CAPTCHA provider's frame origin. The faucet page (citrate-chain `faucet/src/desktop.rs`,
/// `TURNSTILE_ORIGIN`) embeds the challenge as an iframe from here, and the webview asks the
/// navigation handler about every frame, so the challenge window must let exactly this origin
/// load or the CAPTCHA never renders.
pub const CHALLENGE_FRAME_ORIGIN: &str = "https://challenges.cloudflare.com";

/// Decisions this build makes with placeholders, pending owner sign-off (ADR O-1..O-4).
pub const PENDING_OWNER_SIGN_OFF: &[&str] = &[
    "The faucet ADR (ADR-2026-10-01-faucet-for-deploy-gas) is proposed, not accepted. The in-app faucet stays off until the member turns it on. Pending owner sign-off.",
    "O-1: the drip stays the faucet's 10 SALT; no smaller gas-only drip. Pending owner sign-off.",
    "O-2: per-member limits are enforced in the app (one request per 24 h per wallet). The faucet-side membership check exists but is off until the operator turns it on. Pending owner sign-off.",
    "O-3: when the faucet asks for a CAPTCHA, the member solves it in an in-app window on the faucet's own page. Pending owner sign-off.",
    "O-4: the switch lives in Settings, Budgets, and is off by default; turning it on is the member's grant. Pending owner sign-off.",
];

// ---------------------------------------------------------------------------------------------
// state file

/// The member's grant (ADR D4.3). Present = on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaucetBudget {
    /// The wallet the grant was made for (lowercase `0x` address). A different active wallet
    /// does not inherit it.
    pub wallet: String,
    pub granted_at_ms: u64,
    pub window_ms: u64,
    pub max_per_window: u32,
}

/// What came of one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The faucet sent the drip (it returned a transaction hash).
    Sent,
    /// The faucet refused because of a cooldown or cap; it said when to come back.
    RateLimited,
    /// The faucet wants a CAPTCHA; nothing was sent.
    ChallengeRequired,
    /// The faucet refused for another reason (its message is kept).
    Refused,
    /// The faucet could not be reached or answered with an HTTP error.
    Unreachable,
    /// The faucet's answer does not say whether the drip went out. Counted against the window.
    Unknown,
}

impl Outcome {
    /// Outcomes that use up the member's window: the drip went out, or may have.
    pub fn consumes_window(self) -> bool {
        matches!(self, Outcome::Sent | Outcome::Unknown)
    }
}

/// One request the app made to the faucet (the decision record the Budgets panel shows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub at_ms: u64,
    pub wallet: String,
    /// Who asked: `local-user`, `hermes`, or `mcp:<label>`.
    pub origin: String,
    pub initcode_hash: Option<String>,
    pub need_wei: Option<String>,
    pub balance_wei: Option<String>,
    pub outcome: Outcome,
    pub tx_hash: Option<String>,
    pub message: String,
    pub next_eligible_at_ms: Option<u64>,
}

/// The persisted file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FaucetBook {
    pub budget: Option<FaucetBudget>,
    pub ledger: Vec<LedgerEntry>,
}

impl FaucetBook {
    fn append_entry(&mut self, e: LedgerEntry) {
        self.ledger.push(e);
        if self.ledger.len() > MAX_LEDGER {
            let drop = self.ledger.len() - MAX_LEDGER;
            self.ledger.drain(..drop);
        }
    }
}

/// The state file, serialized by one lock so two callers can never both pass the window check.
pub struct FaucetStore {
    path: Option<PathBuf>,
    lock: Mutex<()>,
}

impl FaucetStore {
    /// `path` is `None` when the app data dir is unavailable: the faucet then stays off.
    pub fn new(path: Option<PathBuf>) -> Self {
        FaucetStore {
            path,
            lock: Mutex::new(()),
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Read the file. Missing: empty (off). Unreadable or corrupt: an error, so the caller can
    /// say so instead of silently resetting the member's history.
    pub fn load(&self) -> Result<FaucetBook, String> {
        let Some(p) = self.path.as_ref() else {
            return Ok(FaucetBook::default());
        };
        match std::fs::read(p) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("the faucet settings file is damaged ({e})")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FaucetBook::default()),
            Err(e) => Err(format!("the faucet settings file cannot be read ({e})")),
        }
    }

    fn save(&self, book: &FaucetBook) -> Result<(), String> {
        let p = self
            .path
            .as_ref()
            .ok_or("the app data folder is unavailable, so the faucet cannot be turned on")?;
        let body = serde_json::to_vec_pretty(book).map_err(|e| e.to_string())?;
        crate::node_mcp_token::write_private(p, &body)
    }
}

/// Managed state.
pub struct FaucetState(pub FaucetStore);

/// Build the managed state from the app handle.
pub fn build_faucet_state(app: &tauri::AppHandle) -> FaucetState {
    use tauri::Manager;
    FaucetState(FaucetStore::new(
        app.path()
            .app_data_dir()
            .ok()
            .map(|d| d.join(STATE_FILE_NAME)),
    ))
}

// ---------------------------------------------------------------------------------------------
// pure rules

/// A lowercase `0x` 20-byte address.
pub fn normalize_address(s: &str) -> Result<String, String> {
    let h = s
        .trim()
        .strip_prefix("0x")
        .ok_or("expected a 0x-prefixed 20-byte address")?;
    if h.len() != 40 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("expected a 0x-prefixed 20-byte address".into());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// A `0x` 32-byte hash, lowercased.
pub fn normalize_hash(s: &str) -> Result<String, String> {
    let h = s
        .trim()
        .strip_prefix("0x")
        .ok_or("initcode_hash must be a 0x-prefixed 32-byte hash")?;
    if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("initcode_hash must be a 0x-prefixed 32-byte hash".into());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// The faucet base URL: `CITRATE_FAUCET_URL` when it is https (or http on loopback), else the
/// default. A trailing slash is dropped.
pub fn faucet_base_url(env: Option<&str>) -> String {
    let candidate = env.map(str::trim).filter(|s| !s.is_empty());
    match candidate {
        Some(u) if is_allowed_base(u) => u.trim_end_matches('/').to_string(),
        _ => DEFAULT_FAUCET_URL.to_string(),
    }
}

fn is_allowed_base(u: &str) -> bool {
    let Ok(url) = url::Url::parse(u) else {
        return false;
    };
    let host_ok = url.host_str().is_some_and(|h| !h.is_empty());
    let path_ok = url.path() == "/" || url.path().is_empty();
    let clean = url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => matches!(
            url.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        ),
        _ => false,
    };
    host_ok && path_ok && clean && scheme_ok
}

/// Need = gas limit × gas price (wei), saturating.
pub fn need_wei(gas_limit: u64, gas_price_wei: u128) -> u128 {
    u128::from(gas_limit).saturating_mul(gas_price_wei)
}

/// What the rules say before any faucet call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum Gate {
    /// The member has not turned the in-app faucet on.
    Disabled,
    /// The grant was made for another wallet.
    WalletChanged { granted_for: String },
    /// No READY deploy for that init code: nothing is requested speculatively.
    NoPendingDeploy,
    /// The balance already covers the deploy.
    NotNeeded {
        balance_wei: String,
        need_wei: String,
    },
    /// Inside the member's window, or the faucet said to come back later.
    Waiting {
        next_eligible_at_ms: u64,
        reason: String,
    },
    /// Ask the faucet.
    Go,
}

/// When the member may ask again, from the history (None = now).
pub fn next_eligible_ms(book: &FaucetBook, wallet: &str, now_ms: u64) -> Option<(u64, String)> {
    let window = book
        .budget
        .as_ref()
        .map(|b| b.window_ms)
        .unwrap_or(MEMBER_WINDOW_MS);
    let max = book
        .budget
        .as_ref()
        .map(|b| b.max_per_window)
        .unwrap_or(MAX_PER_WINDOW)
        .max(1) as usize;
    let mine: Vec<&LedgerEntry> = book.ledger.iter().filter(|e| e.wallet == wallet).collect();
    let used: Vec<u64> = mine
        .iter()
        .filter(|e| e.outcome.consumes_window() && now_ms.saturating_sub(e.at_ms) < window)
        .map(|e| e.at_ms)
        .collect();
    let mut best: Option<(u64, String)> = None;
    if used.len() >= max {
        let oldest = used.iter().copied().min().unwrap_or(now_ms);
        best = Some((
            oldest.saturating_add(window),
            "Your faucet budget allows one top-up per 24 hours.".to_string(),
        ));
    }
    if let Some(t) = mine
        .iter()
        .filter_map(|e| e.next_eligible_at_ms)
        .filter(|t| *t > now_ms)
        .max()
    {
        if best.as_ref().is_none_or(|(b, _)| t > *b) {
            best = Some((t, "The faucet asked to wait until then.".to_string()));
        }
    }
    best
}

/// The rules (ADR D4), in order.
pub fn gate(
    book: &FaucetBook,
    wallet: &str,
    deploy_ready: bool,
    balance_wei: u128,
    need: u128,
    now_ms: u64,
) -> Gate {
    let Some(budget) = book.budget.as_ref() else {
        return Gate::Disabled;
    };
    if budget.wallet != wallet {
        return Gate::WalletChanged {
            granted_for: budget.wallet.clone(),
        };
    }
    if !deploy_ready {
        return Gate::NoPendingDeploy;
    }
    if balance_wei >= need {
        return Gate::NotNeeded {
            balance_wei: balance_wei.to_string(),
            need_wei: need.to_string(),
        };
    }
    if let Some((t, reason)) = next_eligible_ms(book, wallet, now_ms) {
        return Gate::Waiting {
            next_eligible_at_ms: t,
            reason,
        };
    }
    Gate::Go
}

/// One interpreted faucet reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpreted {
    pub outcome: Outcome,
    pub tx_hash: Option<String>,
    pub message: String,
    pub next_eligible_at_ms: Option<u64>,
}

/// Text from the faucet, made safe to show: no control or bidi characters, bounded.
pub fn clean_message(s: &str) -> String {
    s.chars()
        .filter(|c| !crate::node_mcp_tools::is_unsafe_display_char(*c))
        .take(MAX_MESSAGE_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// `"… 23h 5m remaining"` (an older faucet's only signal) → seconds.
fn remaining_from_text(msg: &str) -> Option<u64> {
    let before = msg.split("remaining").next()?;
    let mut hours = None;
    let mut minutes = None;
    for tok in before.split_whitespace() {
        if let Some(h) = tok.strip_suffix('h') {
            hours = h.parse::<u64>().ok().or(hours);
        } else if let Some(m) = tok.strip_suffix('m') {
            minutes = m.parse::<u64>().ok().or(minutes);
        }
    }
    let (h, m) = (hours?, minutes.unwrap_or(0));
    Some(h.saturating_mul(3600).saturating_add(m.saturating_mul(60)))
}

fn is_tx_hash(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Read a `POST /faucet` reply (status + body) into an outcome. Unknown shapes are never read as
/// success.
pub fn interpret_reply(status: u16, body: &str, now_ms: u64) -> Interpreted {
    let unreachable = |message: String| Interpreted {
        outcome: Outcome::Unreachable,
        tx_hash: None,
        message,
        next_eligible_at_ms: None,
    };
    if !(200..300).contains(&status) {
        return unreachable(format!("The faucet answered with HTTP {status}."));
    }
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return unreachable("The faucet's answer was not readable.".to_string());
    };
    let message = clean_message(v.get("message").and_then(Value::as_str).unwrap_or(""));
    let code = v.get("code").and_then(Value::as_str).unwrap_or("");
    if v.get("success").and_then(Value::as_bool) == Some(true) {
        return match v.get("tx_hash").and_then(Value::as_str) {
            Some(h) if is_tx_hash(h) => Interpreted {
                outcome: Outcome::Sent,
                tx_hash: Some(h.to_ascii_lowercase()),
                message,
                next_eligible_at_ms: None,
            },
            _ => Interpreted {
                outcome: Outcome::Unknown,
                tx_hash: None,
                message: "The faucet reported success without a transaction hash.".to_string(),
                next_eligible_at_ms: None,
            },
        };
    }
    let rate_limited =
        code == "rate_limited" || (code.is_empty() && message.starts_with("Rate limited"));
    if rate_limited {
        let next = v
            .get("next_eligible_at")
            .and_then(Value::as_u64)
            .map(|s| s.saturating_mul(1000))
            .or_else(|| {
                v.get("retry_after_secs")
                    .and_then(Value::as_u64)
                    .or_else(|| remaining_from_text(&message))
                    .map(|s| now_ms.saturating_add(s.saturating_mul(1000)))
            });
        return Interpreted {
            outcome: Outcome::RateLimited,
            tx_hash: None,
            message,
            next_eligible_at_ms: next,
        };
    }
    let challenge = matches!(code, "captcha_required" | "captcha_failed")
        || (code.is_empty() && message.starts_with("CAPTCHA required"));
    if challenge {
        return Interpreted {
            outcome: Outcome::ChallengeRequired,
            tx_hash: None,
            message,
            next_eligible_at_ms: None,
        };
    }
    let unknown =
        code == "unknown_rpc_response" || (code.is_empty() && message == "Unknown RPC response");
    if unknown {
        return Interpreted {
            outcome: Outcome::Unknown,
            tx_hash: None,
            message: "The faucet could not tell whether the drip went out. It is counted against today's budget.".to_string(),
            next_eligible_at_ms: None,
        };
    }
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Interpreted {
            outcome: Outcome::Refused,
            tx_hash: None,
            message: if message.is_empty() {
                "The faucet refused the request without a reason.".to_string()
            } else {
                message
            },
            next_eligible_at_ms: None,
        };
    }
    unreachable("The faucet's answer was not in the expected form.".to_string())
}

// ---------------------------------------------------------------------------------------------
// decision records (ADR D4.3)

/// Where faucet decisions are recorded: core's HIC outbox in the app, the same outbox type on a
/// scratch folder in tests.
pub trait DecisionSink {
    /// Whether a record could be written now. Checked before anything happens (fail closed).
    fn check_writable(&self) -> Result<(), String>;
    /// Write one record; returns its id.
    fn record(&self, ev: crate::hic_records::HicEvent) -> Result<u64, String>;
}

impl DecisionSink for crate::hic_records::HicOutbox {
    fn check_writable(&self) -> Result<(), String> {
        crate::hic_records::HicOutbox::check_writable(self)
    }
    fn record(&self, ev: crate::hic_records::HicEvent) -> Result<u64, String> {
        self.append(ev, crate::hic_records::now_ms())
            .map(|r| r.record_id)
    }
}

/// The member turned the in-app faucet on (HIC-1).
pub fn budget_granted_event(b: &FaucetBudget) -> crate::hic_records::HicEvent {
    crate::hic_records::HicEvent {
        kind: "faucet.budget_granted".to_string(),
        decision: "approved".to_string(),
        subject: crate::hic_records::fit(
            &format!(
                "faucet budget for {}: {} top-up per {} h, deploy gas only",
                b.wallet,
                b.max_per_window,
                b.window_ms / 3_600_000
            ),
            300,
        ),
        reason: "the member turned the in-app faucet on in Settings, Budgets (HIC-1)".to_string(),
        outcome: Some("completed".to_string()),
        outcome_detail: Some("saved to the faucet settings file".to_string()),
        evidence: Vec::new(),
    }
}

/// The member turned the in-app faucet off (HIC-1).
pub fn budget_revoked_event(wallet: &str) -> crate::hic_records::HicEvent {
    crate::hic_records::HicEvent {
        kind: "faucet.budget_revoked".to_string(),
        decision: "approved".to_string(),
        subject: crate::hic_records::fit(&format!("faucet budget for {wallet}"), 300),
        reason: "the member turned the in-app faucet off in Settings, Budgets".to_string(),
        outcome: Some("completed".to_string()),
        outcome_detail: Some("removed from the faucet settings file".to_string()),
        evidence: Vec::new(),
    }
}

/// One faucet call. The member's own click is `approved` (HIC-1); a call from Hermes or an MCP
/// client runs inside the member's budget, `auto_within_budget` (HIC-2).
pub fn topup_event(e: &LedgerEntry) -> crate::hic_records::HicEvent {
    let by_member = e.origin == "local-user";
    let outcome = match e.outcome {
        Outcome::Sent => "completed",
        Outcome::Unknown => "outcome_unknown",
        Outcome::RateLimited
        | Outcome::ChallengeRequired
        | Outcome::Refused
        | Outcome::Unreachable => "failed",
    };
    let label = match e.outcome {
        Outcome::Sent => "sent",
        Outcome::RateLimited => "rate_limited",
        Outcome::ChallengeRequired => "challenge_required",
        Outcome::Refused => "refused",
        Outcome::Unreachable => "unreachable",
        Outcome::Unknown => "unknown",
    };
    let mut detail = format!("{label}: {}", e.message);
    if let Some(t) = e.next_eligible_at_ms {
        detail = format!("{detail} (next eligible at {t} ms)");
    }
    let evidence = e
        .tx_hash
        .as_ref()
        .map(|h| {
            vec![crate::hic_records::HicEvidence {
                kind: "tx".to_string(),
                uri: format!("eip155:40204/tx/{h}"),
                digest: Some(h.clone()),
            }]
        })
        .unwrap_or_default();
    crate::hic_records::HicEvent {
        kind: "faucet.topup".to_string(),
        decision: if by_member {
            "approved"
        } else {
            "auto_within_budget"
        }
        .to_string(),
        subject: crate::hic_records::fit(
            &format!(
                "deploy-gas top-up for {} (init code {}, need {} wei, balance {} wei)",
                e.wallet,
                e.initcode_hash.as_deref().unwrap_or("unknown"),
                e.need_wei.as_deref().unwrap_or("unknown"),
                e.balance_wei.as_deref().unwrap_or("unknown"),
            ),
            300,
        ),
        reason: crate::hic_records::fit(
            &if by_member {
                "the member asked the faucet from the app (HIC-1)".to_string()
            } else {
                format!(
                    "asked by {} inside the member's faucet budget (HIC-2)",
                    e.origin
                )
            },
            400,
        ),
        outcome: Some(outcome.to_string()),
        outcome_detail: Some(crate::hic_records::fit(&detail, 300)),
        evidence,
    }
}

// ---------------------------------------------------------------------------------------------
// I/O seams

/// The HTTP calls this module makes; tests script them, production uses `ureq`.
pub trait FaucetHttp {
    fn get(&self, url: &str) -> Result<(u16, String), String>;
    fn post_json(&self, url: &str, body: &Value) -> Result<(u16, String), String>;
}

/// Production HTTP: blocking `ureq` (rustls), no redirects, bounded body, non-2xx as a status.
pub struct UreqFaucet;

impl UreqFaucet {
    fn read(resp: ureq::http::Response<ureq::Body>) -> Result<(u16, String), String> {
        let status = resp.status().as_u16();
        let body = resp
            .into_body()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|e| format!("the faucet's answer could not be read ({e})"))?;
        Ok((status, body))
    }
}

impl FaucetHttp for UreqFaucet {
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        let resp = ureq::get(url)
            .config()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .call()
            .map_err(|e| format!("the faucet is unreachable ({e})"))?;
        Self::read(resp)
    }

    fn post_json(&self, url: &str, body: &Value) -> Result<(u16, String), String> {
        let resp = ureq::post(url)
            .config()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .send_json(body)
            .map_err(|e| format!("the faucet is unreachable ({e})"))?;
        Self::read(resp)
    }
}

/// The two chain reads the rules need.
pub trait ChainReads {
    fn balance_wei(&self, address: &str) -> Result<u128, String>;
    fn gas_price_wei(&self) -> Result<u128, String>;
}

/// Production reads: the public 40204 RPC.
pub struct RpcChainReads;

impl ChainReads for RpcChainReads {
    fn balance_wei(&self, address: &str) -> Result<u128, String> {
        crate::rpc::RpcClient::citrate()
            .get_balance(address)
            .map_err(|e| format!("the wallet balance could not be read ({e})"))
    }
    fn gas_price_wei(&self) -> Result<u128, String> {
        crate::rpc::RpcClient::citrate()
            .gas_price()
            .map(u128::from)
            .map_err(|e| format!("the gas price could not be read ({e})"))
    }
}

// ---------------------------------------------------------------------------------------------
// health + eligibility

/// What the Budgets panel shows about the faucet itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaucetHealth {
    pub reachable: bool,
    /// `Some(true/false)` from `/ready`; `None` when the faucet does not report readiness.
    pub ready: Option<bool>,
    pub detail: String,
}

/// Probe `/ready`, falling back to `/health` for a faucet without readiness.
pub fn probe_health(http: &dyn FaucetHttp, base: &str) -> FaucetHealth {
    match http.get(&format!("{base}/ready")) {
        Ok((200, body)) | Ok((503, body)) => {
            let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let ready = v.get("ready").and_then(Value::as_bool);
            let reason = v
                .get("reason")
                .and_then(Value::as_str)
                .map(clean_message)
                .unwrap_or_default();
            match ready {
                Some(true) => FaucetHealth {
                    reachable: true,
                    ready: Some(true),
                    detail: "The faucet is up and can send a drip.".to_string(),
                },
                Some(false) => FaucetHealth {
                    reachable: true,
                    ready: Some(false),
                    detail: if reason.is_empty() {
                        "The faucet is up but cannot send a drip right now.".to_string()
                    } else {
                        format!("The faucet is up but cannot send a drip right now: {reason}.")
                    },
                },
                None => FaucetHealth {
                    reachable: true,
                    ready: None,
                    detail: "The faucet answered, but not in the expected form.".to_string(),
                },
            }
        }
        Ok((404, _)) => match http.get(&format!("{base}/health")) {
            Ok((200, _)) => FaucetHealth {
                reachable: true,
                ready: None,
                detail: "The faucet is up. This faucet version does not report whether it can send a drip.".to_string(),
            },
            Ok((s, _)) => FaucetHealth {
                reachable: false,
                ready: None,
                detail: format!("The faucet answered with HTTP {s}."),
            },
            Err(e) => FaucetHealth {
                reachable: false,
                ready: None,
                detail: clean_message(&e),
            },
        },
        Ok((s, _)) => FaucetHealth {
            reachable: false,
            ready: None,
            detail: format!("The faucet answered with HTTP {s}."),
        },
        Err(e) => FaucetHealth {
            reachable: false,
            ready: None,
            detail: clean_message(&e),
        },
    }
}

/// The faucet's own view of when `wallet` may ask again (`None` = unknown: older faucet or
/// unreachable). `Some(None)` = eligible now.
pub fn faucet_eligibility(
    http: &dyn FaucetHttp,
    base: &str,
    wallet: &str,
    now_ms: u64,
) -> Option<Option<u64>> {
    let (status, body) = http
        .get(&format!("{base}/eligibility?address={wallet}"))
        .ok()?;
    if status != 200 {
        return None;
    }
    let v: Value = serde_json::from_str(&body).ok()?;
    match v.get("eligible").and_then(Value::as_bool)? {
        true => Some(None),
        false => {
            let at = v
                .get("next_eligible_at")
                .and_then(Value::as_u64)
                .map(|s| s.saturating_mul(1000))
                .or_else(|| {
                    v.get("retry_after_secs")
                        .and_then(Value::as_u64)
                        .map(|s| now_ms.saturating_add(s.saturating_mul(1000)))
                })?;
            Some(Some(at))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the request

/// What a request returns to the app, Hermes, or an MCP client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaucetResult {
    /// The gate state, or `requested` when the faucet was asked.
    pub state: String,
    pub gate: Gate,
    pub outcome: Option<Outcome>,
    pub tx_hash: Option<String>,
    pub next_eligible_at_ms: Option<u64>,
    pub balance_wei: Option<String>,
    pub need_wei: Option<String>,
    /// Plain words for the member (and for Hermes to repeat).
    pub message: String,
    /// The faucet's page, for funding by hand or solving a CAPTCHA.
    pub faucet_page: String,
}

fn gate_state(g: &Gate) -> &'static str {
    match g {
        Gate::Disabled => "disabled",
        Gate::WalletChanged { .. } => "wallet_changed",
        Gate::NoPendingDeploy => "no_pending_deploy",
        Gate::NotNeeded { .. } => "not_needed",
        Gate::Waiting { .. } => "waiting",
        Gate::Go => "requested",
    }
}

fn gate_message(g: &Gate, page: &str) -> String {
    match g {
        Gate::Disabled => format!(
            "The in-app faucet is not turned on. The member can turn it on in Settings, Budgets, or fund the deploy from their own SALT or the faucet page ({page})."
        ),
        Gate::WalletChanged { .. } => "The faucet budget was granted for a different wallet. The member must grant it again for this wallet in Settings, Budgets.".to_string(),
        Gate::NoPendingDeploy => "No deploy is ready for that init code, so no top-up is requested. Run the deploy gate first.".to_string(),
        Gate::NotNeeded { .. } => "The wallet already holds enough SALT for this deploy. No top-up is needed.".to_string(),
        Gate::Waiting { reason, .. } => format!("{reason} No request was sent; fund the deploy yourself or wait."),
        Gate::Go => String::new(),
    }
}

fn outcome_message(i: &Interpreted, page: &str) -> String {
    match i.outcome {
        Outcome::Sent => "The faucet sent a top-up for deploy gas. It arrives once the transaction is in a block.".to_string(),
        Outcome::RateLimited => format!("The faucet refused for now: {}. No retry is made.", i.message),
        Outcome::ChallengeRequired => format!(
            "The faucet asks for a CAPTCHA. Open the faucet's page in the app to solve it ({page}); nothing was sent."
        ),
        Outcome::Refused => format!("The faucet refused: {}", i.message),
        Outcome::Unreachable => format!(
            "The faucet could not be reached ({}). Fund the deploy from your own SALT, or try the faucet page later.",
            i.message
        ),
        Outcome::Unknown => i.message.clone(),
    }
}

/// The inputs of one request.
pub struct RequestCtx<'a> {
    pub wallet: &'a str,
    pub origin: &'a str,
    pub initcode_hash: &'a str,
    pub deploy_ready: bool,
    pub base: &'a str,
    pub now_ms: u64,
}

/// Run one request end to end: rules, at most one faucet call, a ledger entry. The store lock is
/// held throughout, so a second caller sees the first one's entry.
pub fn request(
    store: &FaucetStore,
    decisions: &dyn DecisionSink,
    http: &dyn FaucetHttp,
    chain: &dyn ChainReads,
    ctx: &RequestCtx<'_>,
) -> Result<FaucetResult, String> {
    let wallet = normalize_address(ctx.wallet)?;
    let initcode_hash = normalize_hash(ctx.initcode_hash)?;
    let page = format!("{}/?address={wallet}", ctx.base);
    let _g = store.guard();
    let mut book = store.load()?;
    // Cheap rules first: an off switch or a changed wallet needs no RPC.
    let early = gate(&book, &wallet, ctx.deploy_ready, 0, 1, ctx.now_ms);
    if matches!(
        early,
        Gate::Disabled | Gate::WalletChanged { .. } | Gate::NoPendingDeploy
    ) {
        return Ok(FaucetResult {
            state: gate_state(&early).to_string(),
            message: gate_message(&early, &page),
            gate: early,
            outcome: None,
            tx_hash: None,
            next_eligible_at_ms: None,
            balance_wei: None,
            need_wei: None,
            faucet_page: page,
        });
    }
    let balance = chain.balance_wei(&wallet)?;
    let need = need_wei(DEPLOY_GAS_LIMIT, chain.gas_price_wei()?);
    let g = gate(&book, &wallet, ctx.deploy_ready, balance, need, ctx.now_ms);
    if g != Gate::Go {
        let next = match &g {
            Gate::Waiting {
                next_eligible_at_ms,
                ..
            } => Some(*next_eligible_at_ms),
            _ => None,
        };
        return Ok(FaucetResult {
            state: gate_state(&g).to_string(),
            message: gate_message(&g, &page),
            gate: g,
            outcome: None,
            tx_hash: None,
            next_eligible_at_ms: next,
            balance_wei: Some(balance.to_string()),
            need_wei: Some(need.to_string()),
            faucet_page: page,
        });
    }
    // Fail closed: no faucet call whose decision record could not be written (ADR D4.3).
    decisions.check_writable().map_err(|e| {
        format!("the faucet was not asked: the decision record cannot be written ({e})")
    })?;
    let reply = http.post_json(
        &format!("{}/faucet", ctx.base),
        &json!({ "address": wallet }),
    );
    let interpreted = match reply {
        Ok((status, body)) => interpret_reply(status, &body, ctx.now_ms),
        Err(e) => Interpreted {
            outcome: Outcome::Unreachable,
            tx_hash: None,
            message: clean_message(&e),
            next_eligible_at_ms: None,
        },
    };
    let entry = LedgerEntry {
        at_ms: ctx.now_ms,
        wallet: wallet.clone(),
        origin: clean_message(ctx.origin),
        initcode_hash: Some(initcode_hash),
        need_wei: Some(need.to_string()),
        balance_wei: Some(balance.to_string()),
        outcome: interpreted.outcome,
        tx_hash: interpreted.tx_hash.clone(),
        message: interpreted.message.clone(),
        next_eligible_at_ms: interpreted.next_eligible_at_ms,
    };
    let ev = topup_event(&entry);
    book.append_entry(entry);
    store.save(&book)?;
    // The call already happened, so a failed record write cannot undo it: say so plainly.
    let mut message = outcome_message(&interpreted, &page);
    if let Err(e) = decisions.record(ev) {
        message = format!("{message} The decision record could not be written ({e}).");
    }
    Ok(FaucetResult {
        state: gate_state(&Gate::Go).to_string(),
        message,
        gate: Gate::Go,
        outcome: Some(interpreted.outcome),
        tx_hash: interpreted.tx_hash,
        next_eligible_at_ms: interpreted.next_eligible_at_ms,
        balance_wei: Some(balance.to_string()),
        need_wei: Some(need.to_string()),
        faucet_page: page,
    })
}

/// Grant the budget for `wallet` (the member's HIC-1 step; the panel confirms first).
/// Fail closed (ADR D4.3): a grant whose decision record cannot be written is undone and refused.
pub fn grant(
    store: &FaucetStore,
    decisions: &dyn DecisionSink,
    wallet: &str,
    now_ms: u64,
) -> Result<FaucetBudget, String> {
    let wallet = normalize_address(wallet)?;
    let _g = store.guard();
    let mut book = store.load()?;
    decisions.check_writable().map_err(|e| {
        format!("the faucet was not turned on: the decision record cannot be written ({e})")
    })?;
    let before = book.budget.clone();
    let b = FaucetBudget {
        wallet,
        granted_at_ms: now_ms,
        window_ms: MEMBER_WINDOW_MS,
        max_per_window: MAX_PER_WINDOW,
    };
    book.budget = Some(b.clone());
    store.save(&book)?;
    if let Err(e) = decisions.record(budget_granted_event(&b)) {
        book.budget = before;
        store.save(&book)?;
        return Err(format!(
            "the faucet was not turned on: the decision record could not be written ({e})"
        ));
    }
    Ok(b)
}

/// Revoke the budget at once. The history stays. Turning the faucet off is never blocked by the
/// decision record (conservative placeholder, pending owner sign-off): the revoke happens, and a
/// record that cannot be written is reported back.
pub fn revoke(store: &FaucetStore, decisions: &dyn DecisionSink) -> Result<(), String> {
    let _g = store.guard();
    let mut book = store.load()?;
    if let Some(b) = book.budget.take() {
        store.save(&book)?;
        decisions
            .record(budget_revoked_event(&b.wallet))
            .map_err(|e| {
                format!("the faucet is off, but the decision record could not be written ({e})")
            })?;
    }
    Ok(())
}

/// What Settings → Budgets shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaucetStatus {
    pub enabled: bool,
    pub budget: Option<FaucetBudget>,
    pub wallet: Option<String>,
    pub wallet_error: Option<String>,
    pub wallet_matches: bool,
    pub store_error: Option<String>,
    pub health: FaucetHealth,
    /// From the app's own history (`None` = may ask now).
    pub next_eligible_at_ms: Option<u64>,
    /// From the faucet's `/eligibility` (`None` = unknown).
    pub faucet_next_eligible_at_ms: Option<u64>,
    pub faucet_eligibility_known: bool,
    pub ledger: Vec<LedgerEntry>,
    pub faucet_url: String,
    pub faucet_page: String,
    pub deploy_gas_limit: u64,
    pub drip_wei: String,
    pub window_hours: u64,
    pub max_per_window: u32,
    pub pending_owner_sign_off: Vec<String>,
    pub now_ms: u64,
}

/// Assemble the status (blocking: probes the faucet).
pub fn status(
    store: &FaucetStore,
    http: &dyn FaucetHttp,
    base: &str,
    wallet: Result<String, String>,
    now_ms: u64,
) -> FaucetStatus {
    let (book, store_error) = match store.load() {
        Ok(b) => (b, None),
        Err(e) => (FaucetBook::default(), Some(e)),
    };
    let (wallet, wallet_error) = match wallet.and_then(|w| normalize_address(&w)) {
        Ok(w) => (Some(w), None),
        Err(e) => (None, Some(e)),
    };
    let wallet_matches = match (&book.budget, &wallet) {
        (Some(b), Some(w)) => b.wallet == *w,
        _ => false,
    };
    let next = wallet
        .as_deref()
        .and_then(|w| next_eligible_ms(&book, w, now_ms))
        .map(|(t, _)| t);
    let faucet_elig = wallet
        .as_deref()
        .and_then(|w| faucet_eligibility(http, base, w, now_ms));
    let health = probe_health(http, base);
    let mut ledger = book.ledger.clone();
    ledger.reverse();
    ledger.truncate(20);
    FaucetStatus {
        enabled: book.budget.is_some(),
        budget: book.budget.clone(),
        faucet_page: match &wallet {
            Some(w) => format!("{base}/?address={w}"),
            None => format!("{base}/"),
        },
        wallet,
        wallet_error,
        wallet_matches,
        store_error,
        health,
        next_eligible_at_ms: next,
        faucet_next_eligible_at_ms: faucet_elig.flatten(),
        faucet_eligibility_known: faucet_elig.is_some(),
        ledger,
        faucet_url: base.to_string(),
        deploy_gas_limit: DEPLOY_GAS_LIMIT,
        drip_wei: DRIP_WEI.to_string(),
        window_hours: MEMBER_WINDOW_MS / 3_600_000,
        max_per_window: MAX_PER_WINDOW,
        pending_owner_sign_off: PENDING_OWNER_SIGN_OFF
            .iter()
            .map(|s| s.to_string())
            .collect(),
        now_ms,
    }
}

// ---------------------------------------------------------------------------------------------
// app glue

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Core's HIC outbox, as the faucet's decision sink (records exported to Hermes after each one).
struct AppDecisions<'a>(&'a tauri::AppHandle);

impl DecisionSink for AppDecisions<'_> {
    fn check_writable(&self) -> Result<(), String> {
        crate::hic_records::outbox_for_app(self.0)?.check_writable()
    }
    fn record(&self, ev: crate::hic_records::HicEvent) -> Result<u64, String> {
        crate::hic_records::record_for_app(self.0, ev)
    }
}

fn base_url() -> String {
    faucet_base_url(std::env::var(FAUCET_URL_ENV).ok().as_deref())
}

fn wallet_of(app: &tauri::AppHandle) -> Result<String, String> {
    let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(app)
        .ok_or("the wallet is not available")?;
    crate::wallet::address_auto_unlocked(&custody.0)
        .map(|w| w.address)
        .map_err(|e| format!("the wallet is not available ({e})"))
}

fn deploy_ready(app: &tauri::AppHandle, initcode_hash: &str) -> bool {
    let Ok(h) = normalize_hash(initcode_hash) else {
        return false;
    };
    tauri::Manager::try_state::<crate::deploy_gate::DeployGateState>(app)
        .and_then(|g| g.0.get(&h))
        .is_some_and(|r| r.verdict == crate::deploy_gate::Verdict::Ready)
}

/// One request for the member's wallet, from the app, Hermes or an MCP client. Blocking.
pub fn request_for_app(
    app: &tauri::AppHandle,
    initcode_hash: &str,
    origin: &str,
) -> Result<FaucetResult, String> {
    let st = tauri::Manager::try_state::<FaucetState>(app)
        .ok_or("internal: faucet state unavailable")?;
    let wallet = wallet_of(app)?;
    let ctx = RequestCtx {
        wallet: &wallet,
        origin,
        initcode_hash,
        deploy_ready: deploy_ready(app, initcode_hash),
        base: &base_url(),
        now_ms: now_ms(),
    };
    request(&st.0, &AppDecisions(app), &UreqFaucet, &RpcChainReads, &ctx)
}

/// Blocking body of [`faucet_status`].
pub fn faucet_status_sync(app: tauri::AppHandle) -> Result<FaucetStatus, String> {
    let st = tauri::Manager::try_state::<FaucetState>(&app)
        .ok_or("internal: faucet state unavailable")?;
    Ok(status(
        &st.0,
        &UreqFaucet,
        &base_url(),
        wallet_of(&app),
        now_ms(),
    ))
}

/// **Command.** The faucet section of Settings → Budgets.
#[tauri::command]
pub async fn faucet_status(app: tauri::AppHandle) -> Result<FaucetStatus, String> {
    crate::blocking::off_main(move || faucet_status_sync(app)).await
}

/// Blocking body of [`faucet_grant`].
pub fn faucet_grant_sync(app: tauri::AppHandle) -> Result<FaucetBudget, String> {
    let st = tauri::Manager::try_state::<FaucetState>(&app)
        .ok_or("internal: faucet state unavailable")?;
    let wallet = wallet_of(&app)?;
    grant(&st.0, &AppDecisions(&app), &wallet, now_ms())
}

/// **Command.** The member turns the in-app faucet on for their current wallet (the HIC-1 grant;
/// the panel asks for explicit confirmation first). Main window only; no agent route.
#[tauri::command]
pub async fn faucet_grant(app: tauri::AppHandle) -> Result<FaucetBudget, String> {
    crate::blocking::off_main(move || faucet_grant_sync(app)).await
}

/// Blocking body of [`faucet_revoke`].
pub fn faucet_revoke_sync(app: tauri::AppHandle) -> Result<(), String> {
    let st = tauri::Manager::try_state::<FaucetState>(&app)
        .ok_or("internal: faucet state unavailable")?;
    revoke(&st.0, &AppDecisions(&app))
}

/// **Command.** Turn the in-app faucet off at once.
#[tauri::command]
pub async fn faucet_revoke(app: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || faucet_revoke_sync(app)).await
}

/// **Command.** Ask for a deploy-gas top-up for the member's wallet, tied to the READY deploy
/// with this init code hash. Under the member's budget; never signs.
#[tauri::command]
pub async fn faucet_request(
    app: tauri::AppHandle,
    initcode_hash: String,
) -> Result<FaucetResult, String> {
    crate::blocking::off_main(move || request_for_app(&app, &initcode_hash, "local-user")).await
}

/// Is `url` on the faucet's own origin (the challenge window may not navigate anywhere else)?
pub fn same_origin(url: &url::Url, base: &str) -> bool {
    match url::Url::parse(base) {
        Ok(b) => url.origin() == b.origin(),
        Err(_) => false,
    }
}

/// What the challenge window may load. The webview asks about every frame, not only the top one,
/// so this is: the faucet's own origin, the CAPTCHA provider's frame origin
/// ([`CHALLENGE_FRAME_ORIGIN`], exact), and the empty documents a page uses to start an iframe
/// (`about:blank`, `about:srcdoc`). Nothing else. The window has no capability either way.
pub fn challenge_window_may_load(url: &url::Url, base: &str) -> bool {
    if same_origin(url, base) {
        return true;
    }
    if url.scheme() == "about" {
        return matches!(url.path(), "blank" | "srcdoc");
    }
    match url::Url::parse(CHALLENGE_FRAME_ORIGIN) {
        Ok(c) => url.origin() == c.origin(),
        Err(_) => false,
    }
}

/// **Command.** Open the faucet's own page, with the member's address filled in, in an in-app
/// window so the member can solve the faucet's CAPTCHA (ADR O-3). The window has no capability:
/// the page cannot reach any app command, and it cannot navigate off the faucet's origin.
#[tauri::command]
pub async fn faucet_open_challenge(app: tauri::AppHandle) -> Result<String, String> {
    let wallet = {
        let app = app.clone();
        crate::blocking::off_main(move || wallet_of(&app).and_then(|w| normalize_address(&w)))
            .await?
    };
    let base = base_url();
    let page = format!("{base}/?address={wallet}");
    let url =
        url::Url::parse(&page).map_err(|e| format!("the faucet address is not valid ({e})"))?;
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(CHALLENGE_WINDOW_LABEL) {
        let _ = w.close();
    }
    let nav_base = base.clone();
    tauri::WebviewWindowBuilder::new(
        &app,
        CHALLENGE_WINDOW_LABEL,
        tauri::WebviewUrl::External(url),
    )
    .title("Citrate faucet")
    .inner_size(480.0, 640.0)
    .center()
    .focused(true)
    .on_navigation(move |u| challenge_window_may_load(u, &nav_base))
    .build()
    .map_err(|e| format!("the faucet window could not open ({e})"))?;
    Ok(page)
}

#[cfg(test)]
mod tests {
    include!("faucet_tests.rs");
}
