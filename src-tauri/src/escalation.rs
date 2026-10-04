//! HUP-S1.5 — the escalation router, core half (US-1.5: "escalate when needed, at a price I see").
//!
//! Hermes can hand a hard planning step to a bigger model. Two routes exist:
//!
//! 1. **Member endpoints** (this build). The member adds an OpenAI-compatible endpoint in
//!    Settings › Escalation: a label, the base URL, a model, the price card from their provider,
//!    and an API key. The key is sealed in the **OS keyring** here and never written to a file,
//!    never returned to the webview, and never stored by the sidecar: core reads it for one request
//!    and passes it to the sidecar inside that request (`POST /escalations`), which drops it.
//! 2. **Registry models** (InferenceRouter + x402, ADR-2026-09-30 Rule-3 D3): see
//!    `escalation_registry.rs`. Every registry payment is an HIC-1 ceremony; the route is off until
//!    the address book pins an InferenceRouter and an x402 asset is allowlisted (owner decision O-1).
//!
//! ## The spend budget (TLA+ `formal/SpendBudget.tla`)
//!
//! - **SpendWithinCap.** Budgeted spend for the current UTC day (`committed + reserved`) never
//!   exceeds the member's daily cap. The worst-case price is reserved **write-ahead** (persisted)
//!   before the sidecar is called; settlement charges the provider's reported usage at the member's
//!   price, never more than the reservation.
//! - **NoEscalationWithoutShownPrice.** A run names a quote core issued, and must echo the quoted
//!   price exactly. A quote runs at most once and expires after [`QUOTE_TTL_MS`].
//! - **ResetOnlyAtPeriodBoundary.** Committed spend drops only when the UTC day advances. A clock
//!   that moves backwards never resets it, and a restart reloads it from disk.
//! - **OverBudgetNeedsHic1 / TaintNeedsHic1.** When the price does not fit what is left today, or
//!   the agent's context holds untrusted content, the run needs the member's explicit confirmation
//!   (HIC-1). A confirmed escalation is the member's own decision for that one request and is
//!   recorded separately; it never counts against (or past) the cap.
//! - **EgressOptInOnly.** Requests only go to endpoints the member added. Removing an endpoint
//!   deletes its key and voids its quotes.
//!
//! ## Owner decisions (pending owner sign-off)
//!
//! [`DEFAULT_DAILY_CAP_MICROS`] is **0**, so out of the box every escalation asks. The ceiling a
//! member may set ([`MAX_DAILY_CAP_MICROS`]) and the price ceiling are conservative placeholders.
//!
//! ## Honest limits
//!
//! Prices are what the member typed from their provider's pricing page; the app cannot verify them,
//! and the provider's own bill is authoritative. The input token count is an upper bound (UTF-8
//! bytes plus a per-message allowance), so quotes are ceilings. The ledger file is plain JSON in the
//! app data directory (no MAC); an unreadable file fails closed to asking.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::ai::AiKeyring;

/// One UTC day, the budget period.
pub const DAY_MS: u64 = 86_400_000;
/// Daily cap out of the box: zero, so every escalation asks. Pending owner sign-off.
pub const DEFAULT_DAILY_CAP_MICROS: u64 = 0;
/// The highest daily cap a member can set: 100 USD (in micro-USD). Pending owner sign-off.
pub const MAX_DAILY_CAP_MICROS: u64 = 100_000_000;
/// The highest price per million tokens accepted on a price card: 1,000 USD. Pending owner sign-off.
pub const MAX_PRICE_MICROS_PER_MTOK: u64 = 1_000_000_000;
/// A quote is valid for five minutes.
pub const QUOTE_TTL_MS: u64 = 5 * 60 * 1000;
/// Outstanding quotes kept (oldest dropped first).
pub const MAX_QUOTES: usize = 32;
/// Member endpoints.
pub const MAX_ENDPOINTS: usize = 8;
/// Spend records kept for Settings and the activity monitor.
pub const HISTORY_CAP: usize = 200;
/// Matches the sidecar's limits (citrate-agent-escalation).
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_ESCALATION_TOKENS: u32 = 8192;
pub const DEFAULT_ESCALATION_TOKENS: u32 = 2048;
/// Tokens added per message for the chat template (same as the sidecar).
pub const PER_MESSAGE_OVERHEAD_TOKENS: u64 = 16;

pub const LEDGER_FILE: &str = "ledger.json";
pub const ENDPOINTS_FILE: &str = "endpoints.json";
const DIR_NAME: &str = "escalation";
const KEY_ACCOUNT_PREFIX: &str = "escalation-endpoint:";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscError {
    Invalid(String),
    UnknownEndpoint,
    UnknownQuote,
    QuoteExpired,
    /// The run did not echo the price the quote showed.
    PriceNotShown {
        quoted: u64,
        shown: u64,
    },
    /// The member must confirm (HIC-1): over budget, untrusted context, or an unreadable ledger.
    NeedsConfirmation {
        cost_micros: u64,
        remaining_micros: u64,
        reason: String,
    },
    Storage(String),
}

impl std::fmt::Display for EscError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscError::Invalid(m) => write!(f, "{m}"),
            EscError::UnknownEndpoint => f.write_str("that escalation endpoint is not set up"),
            EscError::UnknownQuote => f.write_str(
                "that price quote was already used or is unknown; ask for a new quote",
            ),
            EscError::QuoteExpired => f.write_str("that price quote expired; ask for a new quote"),
            EscError::PriceNotShown { quoted, shown } => write!(
                f,
                "the price shown ({}) is not the quoted price ({}); nothing was sent",
                fmt_usd(*shown),
                fmt_usd(*quoted)
            ),
            EscError::NeedsConfirmation {
                cost_micros,
                remaining_micros,
                reason,
            } => write!(
                f,
                "NEEDS_CONFIRMATION: {reason}. This escalation costs up to {} and {} is left in today's budget.",
                fmt_usd(*cost_micros),
                fmt_usd(*remaining_micros)
            ),
            EscError::Storage(m) => write!(f, "escalation storage: {m}"),
        }
    }
}

impl std::error::Error for EscError {}

/// `$1.234567` style, trimmed to at least two decimals.
pub fn fmt_usd(micros: u64) -> String {
    let whole = micros / 1_000_000;
    let frac = format!("{:06}", micros % 1_000_000);
    let trimmed = frac.trim_end_matches('0');
    let frac = if trimmed.len() < 2 {
        &frac[..2]
    } else {
        trimmed
    };
    format!("${whole}.{frac}")
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// What the member enters in Settings (the key travels separately).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointInput {
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub input_micros_per_mtok: u64,
    pub output_micros_per_mtok: u64,
}

/// A stored endpoint. No key: the key lives in the OS keyring under [`key_account`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub input_micros_per_mtok: u64,
    pub output_micros_per_mtok: u64,
    pub created_ms: u64,
}

impl Endpoint {
    /// What a price card names as the destination: the label and the host.
    pub fn destination(&self) -> String {
        format!("{} · {}", self.label, host_of(&self.base_url))
    }
}

/// The host (and port) of a URL validated by [`validate_base_url`].
pub fn host_of(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or("").to_string()
}

/// The keyring account for an endpoint's key.
pub fn key_account(endpoint_id: &str) -> String {
    format!("{KEY_ACCOUNT_PREFIX}{endpoint_id}")
}

/// `https://host[:port]/path`, or `http://` to a loopback host only. No userinfo, query, fragment,
/// whitespace or control characters. Same rule as the sidecar.
pub fn validate_base_url(url: &str) -> Result<(), String> {
    if url.len() > 2048 || url.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err("the endpoint URL has characters that are not allowed".into());
    }
    if url.contains('?') || url.contains('#') {
        return Err("the endpoint URL may not carry a query or fragment".into());
    }
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("the endpoint must be https:// (or http:// on this computer)".into());
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err("the endpoint URL has no host".into());
    }
    if authority.contains('@') {
        return Err("the endpoint URL may not carry a user name or password".into());
    }
    if !https {
        let host = if let Some(h) = authority.strip_prefix('[') {
            h.split(']').next().unwrap_or("")
        } else {
            authority
                .rsplit_once(':')
                .map(|(h, _)| h)
                .unwrap_or(authority)
        };
        let loopback = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .map(|ip| ip.is_loopback())
                .unwrap_or(false);
        if !loopback {
            return Err("plain http is only allowed to this computer (loopback)".into());
        }
    }
    Ok(())
}

pub fn validate_endpoint_input(i: &EndpointInput) -> Result<(), String> {
    let label = i.label.trim();
    if label.is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) {
        return Err("give the endpoint a name of 1 to 80 characters".into());
    }
    validate_base_url(i.base_url.trim())?;
    let model = i.model.trim();
    if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err("enter the model name the endpoint expects".into());
    }
    if i.input_micros_per_mtok > MAX_PRICE_MICROS_PER_MTOK
        || i.output_micros_per_mtok > MAX_PRICE_MICROS_PER_MTOK
    {
        return Err(format!(
            "a price above {} per million tokens is not accepted",
            fmt_usd(MAX_PRICE_MICROS_PER_MTOK)
        ));
    }
    Ok(())
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 512 && key.bytes().all(|b| b.is_ascii_graphic())
}

/// The non-secret endpoint view the webview gets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointView {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub input_micros_per_mtok: u64,
    pub output_micros_per_mtok: u64,
    pub destination: String,
}

impl From<&Endpoint> for EndpointView {
    fn from(e: &Endpoint) -> Self {
        EndpointView {
            id: e.id.clone(),
            label: e.label.clone(),
            base_url: e.base_url.clone(),
            model: e.model.clone(),
            input_micros_per_mtok: e.input_micros_per_mtok,
            output_micros_per_mtok: e.output_micros_per_mtok,
            destination: e.destination(),
        }
    }
}

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Within today's budget, no confirmation (HIC-2).
    Budget,
    /// The member confirmed this one request (HIC-1). Not counted against the cap.
    Confirmed,
}

/// A write-ahead reservation for an escalation in flight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    pub escalation_id: String,
    pub endpoint_id: String,
    pub destination: String,
    pub amount_micros: u64,
    pub period: u64,
    pub mode: Mode,
    pub started_ms: u64,
}

/// One finished escalation, for Settings and the activity monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendRecord {
    pub escalation_id: String,
    pub endpoint_id: String,
    pub destination: String,
    pub quoted_micros: u64,
    pub charged_micros: u64,
    pub mode: Mode,
    /// "answered" | "not_sent" | "failed".
    pub outcome: String,
    pub usage_reported: bool,
    pub exceeded_quote: bool,
    pub at_ms: u64,
}

/// The daily spend budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub cap_micros: u64,
    /// The UTC day index (`unix_ms / DAY_MS`) the counters belong to. Never decreases.
    pub period: u64,
    /// Settled budget spend this period.
    pub committed_micros: u64,
    /// Budget reservations in flight this period.
    pub reserved_micros: u64,
    /// Member-confirmed (HIC-1) spend this period, outside the cap.
    pub confirmed_micros: u64,
    pub outstanding: BTreeMap<String, Reservation>,
    pub history: VecDeque<SpendRecord>,
    /// The ledger file could not be read: nothing runs without confirmation.
    #[serde(skip)]
    pub unreadable: bool,
}

impl Ledger {
    pub fn new(cap_micros: u64, now_ms: u64) -> Ledger {
        Ledger {
            cap_micros,
            period: now_ms / DAY_MS,
            committed_micros: 0,
            reserved_micros: 0,
            confirmed_micros: 0,
            outstanding: BTreeMap::new(),
            history: VecDeque::new(),
            unreadable: false,
        }
    }

    /// Budget spend counted against the cap this period.
    pub fn used(&self) -> u64 {
        self.committed_micros.saturating_add(self.reserved_micros)
    }

    pub fn remaining(&self) -> u64 {
        self.cap_micros.saturating_sub(self.used())
    }

    /// Advance to the period of `now_ms` if it is later. Only a later UTC day resets the counters.
    pub fn roll(&mut self, now_ms: u64) {
        let day = now_ms / DAY_MS;
        if day > self.period {
            self.period = day;
            self.committed_micros = 0;
            self.reserved_micros = 0;
            self.confirmed_micros = 0;
        }
    }

    /// Change the daily cap. It may not go below what today already used (that would put spend
    /// over the cap), nor above [`MAX_DAILY_CAP_MICROS`].
    pub fn set_cap(&mut self, cap_micros: u64, now_ms: u64) -> Result<(), EscError> {
        self.roll(now_ms);
        if cap_micros > MAX_DAILY_CAP_MICROS {
            return Err(EscError::Invalid(format!(
                "the daily cap can be at most {}",
                fmt_usd(MAX_DAILY_CAP_MICROS)
            )));
        }
        if cap_micros < self.used() {
            return Err(EscError::Invalid(format!(
                "today's escalations already used {}; set at least that, or lower it tomorrow",
                fmt_usd(self.used())
            )));
        }
        self.cap_micros = cap_micros;
        Ok(())
    }

    fn push_history(&mut self, rec: SpendRecord) {
        self.history.push_back(rec);
        while self.history.len() > HISTORY_CAP {
            self.history.pop_front();
        }
    }
}

// ---------------------------------------------------------------------------
// Quotes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Quote {
    pub endpoint: Endpoint,
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub cost_micros: u64,
    pub expires_ms: u64,
}

/// The price card the webview shows before anything runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteView {
    pub quote_id: String,
    pub endpoint_id: String,
    pub destination: String,
    pub model: String,
    /// The most this request can cost (micro-USD).
    pub cost_micros: u64,
    pub cost_label: String,
    pub within_budget: bool,
    pub remaining_micros: u64,
    pub cap_micros: u64,
    pub max_tokens: u32,
    pub prompt_bytes: usize,
    pub expires_ms: u64,
}

/// Upper bound on input tokens: UTF-8 bytes plus a per-message allowance.
pub fn input_token_bound(texts: &[&str]) -> u64 {
    texts.iter().fold(0u64, |acc, t| {
        acc.saturating_add(t.len() as u64)
            .saturating_add(PER_MESSAGE_OVERHEAD_TOKENS)
    })
}

/// `ceil((in * in_price + out * out_price) / 1e6)`, `None` on overflow.
pub fn price_upper_bound(e: &Endpoint, input_tokens: u64, max_output_tokens: u64) -> Option<u64> {
    let a = u128::from(input_tokens).checked_mul(u128::from(e.input_micros_per_mtok))?;
    let b = u128::from(max_output_tokens).checked_mul(u128::from(e.output_micros_per_mtok))?;
    u64::try_from(a.checked_add(b)?.div_ceil(1_000_000)).ok()
}

// ---------------------------------------------------------------------------
// The book: endpoints + ledger + quotes
// ---------------------------------------------------------------------------

/// An authorized escalation, ready for the sidecar. `Debug` shows no prompt text.
pub struct Authorized {
    pub escalation_id: String,
    pub mode: Mode,
    pub endpoint: Endpoint,
    pub prompt: String,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub reserved_micros: u64,
}

impl std::fmt::Debug for Authorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authorized")
            .field("escalation_id", &self.escalation_id)
            .field("mode", &self.mode)
            .field("endpoint", &self.endpoint.id)
            .field("reserved_micros", &self.reserved_micros)
            .finish_non_exhaustive()
    }
}

/// How an escalation ended, for settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Answered {
        charged_micros: u64,
        usage_reported: bool,
        exceeded_quote: bool,
    },
    /// Refused before anything left this computer: nothing is charged.
    NotSent,
    /// Failed after the request may have reached the provider: the full reservation stays charged.
    MaybeSent,
}

pub struct Book {
    pub endpoints: Vec<Endpoint>,
    pub ledger: Ledger,
    pub quotes: BTreeMap<String, Quote>,
}

impl Book {
    pub fn new(ledger: Ledger) -> Book {
        Book {
            endpoints: Vec::new(),
            ledger,
            quotes: BTreeMap::new(),
        }
    }

    fn endpoint(&self, id: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.id == id)
    }

    /// Add an endpoint record (the caller seals the key first; see [`add_endpoint_with_key`]).
    pub fn add_endpoint(
        &mut self,
        i: &EndpointInput,
        id: String,
        now_ms: u64,
    ) -> Result<Endpoint, EscError> {
        validate_endpoint_input(i).map_err(EscError::Invalid)?;
        if self.endpoints.len() >= MAX_ENDPOINTS {
            return Err(EscError::Invalid(format!(
                "at most {MAX_ENDPOINTS} escalation endpoints"
            )));
        }
        let e = Endpoint {
            id,
            label: i.label.trim().to_string(),
            base_url: i.base_url.trim().trim_end_matches('/').to_string(),
            model: i.model.trim().to_string(),
            input_micros_per_mtok: i.input_micros_per_mtok,
            output_micros_per_mtok: i.output_micros_per_mtok,
            created_ms: now_ms,
        };
        self.endpoints.push(e.clone());
        Ok(e)
    }

    /// Remove an endpoint record and void its quotes.
    pub fn remove_endpoint(&mut self, id: &str) -> Result<(), EscError> {
        let before = self.endpoints.len();
        self.endpoints.retain(|e| e.id != id);
        if self.endpoints.len() == before {
            return Err(EscError::UnknownEndpoint);
        }
        self.quotes.retain(|_, q| q.endpoint.id != id);
        Ok(())
    }

    /// Price a request. Nothing is reserved and nothing runs.
    pub fn quote(
        &mut self,
        endpoint_id: &str,
        prompt: &str,
        system: Option<&str>,
        max_tokens: u32,
        now_ms: u64,
        quote_id: String,
    ) -> Result<QuoteView, EscError> {
        let e = self
            .endpoint(endpoint_id)
            .cloned()
            .ok_or(EscError::UnknownEndpoint)?;
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
            return Err(EscError::Invalid(
                "the escalation prompt must be 1 to 65536 bytes".into(),
            ));
        }
        if system.is_some_and(|s| s.len() > MAX_PROMPT_BYTES) {
            return Err(EscError::Invalid("the system prompt is too long".into()));
        }
        if max_tokens == 0 || max_tokens > MAX_ESCALATION_TOKENS {
            return Err(EscError::Invalid(format!(
                "the answer length must be 1 to {MAX_ESCALATION_TOKENS} tokens"
            )));
        }
        // Count only the messages actually sent (an empty system prompt is omitted on the wire).
        let input = match system.filter(|s| !s.is_empty()) {
            Some(s) => input_token_bound(&[s, prompt]),
            None => input_token_bound(&[prompt]),
        };
        let cost = price_upper_bound(&e, input, u64::from(max_tokens))
            .ok_or_else(|| EscError::Invalid("the price does not fit".into()))?;
        self.ledger.roll(now_ms);
        let within = !self.ledger.unreadable
            && self.ledger.used().saturating_add(cost) <= self.ledger.cap_micros;
        self.quotes.retain(|_, q| q.expires_ms >= now_ms);
        while self.quotes.len() >= MAX_QUOTES {
            let oldest = self
                .quotes
                .iter()
                .min_by_key(|(_, q)| q.expires_ms)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    self.quotes.remove(&k);
                }
                None => break,
            }
        }
        let view = QuoteView {
            quote_id: quote_id.clone(),
            endpoint_id: e.id.clone(),
            destination: e.destination(),
            model: e.model.clone(),
            cost_micros: cost,
            cost_label: fmt_usd(cost),
            within_budget: within,
            remaining_micros: self.ledger.remaining(),
            cap_micros: self.ledger.cap_micros,
            max_tokens,
            prompt_bytes: prompt.len(),
            expires_ms: now_ms.saturating_add(QUOTE_TTL_MS),
        };
        self.quotes.insert(
            quote_id.clone(),
            Quote {
                endpoint: e,
                prompt: prompt.to_string(),
                system: system.map(str::to_string),
                max_tokens,
                cost_micros: cost,
                expires_ms: view.expires_ms,
            },
        );
        Ok(view)
    }

    /// Authorize a shown quote: reserve it against the budget (HIC-2), or, with the member's
    /// confirmation, run it as a one-off (HIC-1). The caller persists the ledger before any egress.
    pub fn authorize(
        &mut self,
        quote_id: &str,
        shown_cost_micros: u64,
        confirmed: bool,
        tainted: bool,
        now_ms: u64,
        escalation_id: String,
    ) -> Result<Authorized, EscError> {
        let q = self.quotes.remove(quote_id).ok_or(EscError::UnknownQuote)?;
        if now_ms > q.expires_ms {
            return Err(EscError::QuoteExpired);
        }
        if shown_cost_micros != q.cost_micros {
            return Err(EscError::PriceNotShown {
                quoted: q.cost_micros,
                shown: shown_cost_micros,
            });
        }
        if self.endpoint(&q.endpoint.id).is_none() {
            return Err(EscError::UnknownEndpoint);
        }
        self.ledger.roll(now_ms);
        let fits = self.ledger.used().saturating_add(q.cost_micros) <= self.ledger.cap_micros;
        let mode = if !tainted && !self.ledger.unreadable && fits {
            Mode::Budget
        } else if confirmed {
            Mode::Confirmed
        } else {
            let reason = if tainted {
                "the agent has read untrusted content in this task, so it asks before spending"
            } else if self.ledger.unreadable {
                "the spend ledger could not be read, so every escalation asks"
            } else {
                "this would go over today's escalation budget"
            };
            let err = EscError::NeedsConfirmation {
                cost_micros: q.cost_micros,
                remaining_micros: self.ledger.remaining(),
                reason: reason.to_string(),
            };
            // Keep the quote so the member can confirm the same price.
            self.quotes.insert(quote_id.to_string(), q);
            return Err(err);
        };
        if mode == Mode::Budget {
            self.ledger.reserved_micros = self.ledger.reserved_micros.saturating_add(q.cost_micros);
        }
        self.ledger.outstanding.insert(
            escalation_id.clone(),
            Reservation {
                escalation_id: escalation_id.clone(),
                endpoint_id: q.endpoint.id.clone(),
                destination: q.endpoint.destination(),
                amount_micros: q.cost_micros,
                period: self.ledger.period,
                mode,
                started_ms: now_ms,
            },
        );
        Ok(Authorized {
            escalation_id,
            mode,
            endpoint: q.endpoint,
            prompt: q.prompt,
            system: q.system,
            max_tokens: q.max_tokens,
            reserved_micros: q.cost_micros,
        })
    }

    /// Settle an escalation. Charges at most its reservation. A reservation from an earlier period
    /// is recorded but never touches the current period's counters.
    pub fn settle(&mut self, escalation_id: &str, s: Settlement, now_ms: u64) -> SpendRecord {
        self.ledger.roll(now_ms);
        let Some(r) = self.ledger.outstanding.remove(escalation_id) else {
            return SpendRecord {
                escalation_id: escalation_id.to_string(),
                endpoint_id: String::new(),
                destination: String::new(),
                quoted_micros: 0,
                charged_micros: 0,
                mode: Mode::Confirmed,
                outcome: "unknown".into(),
                usage_reported: false,
                exceeded_quote: false,
                at_ms: now_ms,
            };
        };
        let (charged, outcome, usage_reported, exceeded) = match s {
            Settlement::Answered {
                charged_micros,
                usage_reported,
                exceeded_quote,
            } => (
                charged_micros.min(r.amount_micros),
                "answered",
                usage_reported,
                exceeded_quote || charged_micros > r.amount_micros,
            ),
            Settlement::NotSent => (0, "not_sent", false, false),
            Settlement::MaybeSent => (r.amount_micros, "failed", false, false),
        };
        if r.period == self.ledger.period {
            match r.mode {
                Mode::Budget => {
                    self.ledger.reserved_micros =
                        self.ledger.reserved_micros.saturating_sub(r.amount_micros);
                    self.ledger.committed_micros =
                        self.ledger.committed_micros.saturating_add(charged);
                }
                Mode::Confirmed => {
                    self.ledger.confirmed_micros =
                        self.ledger.confirmed_micros.saturating_add(charged);
                }
            }
        }
        let rec = SpendRecord {
            escalation_id: r.escalation_id,
            endpoint_id: r.endpoint_id,
            destination: r.destination,
            quoted_micros: r.amount_micros,
            charged_micros: charged,
            mode: r.mode,
            outcome: outcome.into(),
            usage_reported,
            exceeded_quote: exceeded,
            at_ms: now_ms,
        };
        self.ledger.push_history(rec.clone());
        rec
    }
}

/// Seal the key, then add the record. A keyring failure adds nothing.
pub fn add_endpoint_with_key(
    book: &mut Book,
    keyring: &dyn AiKeyring,
    input: &EndpointInput,
    api_key: &str,
    id: String,
    now_ms: u64,
) -> Result<EndpointView, EscError> {
    validate_endpoint_input(input).map_err(EscError::Invalid)?;
    if !valid_key(api_key) {
        return Err(EscError::Invalid(
            "the API key is empty, too long, or has spaces or control characters".into(),
        ));
    }
    if book.endpoints.len() >= MAX_ENDPOINTS {
        return Err(EscError::Invalid(format!(
            "at most {MAX_ENDPOINTS} escalation endpoints"
        )));
    }
    keyring
        .set(&key_account(&id), api_key)
        .map_err(|_| EscError::Storage("the OS keyring is unavailable".into()))?;
    match book.add_endpoint(input, id.clone(), now_ms) {
        Ok(e) => Ok(EndpointView::from(&e)),
        Err(err) => {
            let _ = keyring.delete(&key_account(&id));
            Err(err)
        }
    }
}

/// Delete the key, then the record (and its quotes).
pub fn remove_endpoint_with_key(
    book: &mut Book,
    keyring: &dyn AiKeyring,
    id: &str,
) -> Result<(), EscError> {
    if book.endpoint(id).is_none() {
        return Err(EscError::UnknownEndpoint);
    }
    keyring
        .delete(&key_account(id))
        .map_err(|_| EscError::Storage("the OS keyring is unavailable".into()))?;
    book.remove_endpoint(id)
}

// ---------------------------------------------------------------------------
// The sidecar request and answer
// ---------------------------------------------------------------------------

/// The `POST /escalations` body. It carries the key, so it lives in a wiping buffer and is never
/// logged.
pub fn sidecar_body(a: &Authorized, api_key: &str) -> Zeroizing<String> {
    Zeroizing::new(
        serde_json::json!({
            "escalationId": a.escalation_id,
            "baseUrl": a.endpoint.base_url,
            "model": a.endpoint.model,
            "apiKey": api_key,
            "system": a.system,
            "prompt": a.prompt,
            "maxTokens": a.max_tokens,
            "price": {
                "inputMicrosPerMtok": a.endpoint.input_micros_per_mtok,
                "outputMicrosPerMtok": a.endpoint.output_micros_per_mtok,
            },
            "reservedMicros": a.reserved_micros,
        })
        .to_string(),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SidecarOutcome {
    content: String,
    charged_micros: u64,
    #[serde(default)]
    usage_reported: bool,
    #[serde(default)]
    exceeded_quote: bool,
}

/// Read the sidecar's answer: the settlement plus the text, or the settlement plus an error. An
/// answer core cannot read counts as possibly sent.
pub fn interpret_sidecar(
    status: u16,
    body: &str,
) -> Result<(Settlement, String), (Settlement, String)> {
    if (200..300).contains(&status) {
        return match serde_json::from_str::<SidecarOutcome>(body) {
            Ok(o) => Ok((
                Settlement::Answered {
                    charged_micros: o.charged_micros,
                    usage_reported: o.usage_reported,
                    exceeded_quote: o.exceeded_quote,
                },
                o.content,
            )),
            Err(_) => Err((
                Settlement::MaybeSent,
                "the escalation answer could not be read".into(),
            )),
        };
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let msg: String = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("the escalation failed")
        .chars()
        .take(200)
        .collect();
    let sent = v.get("sent").and_then(|s| s.as_bool()).unwrap_or(true);
    Err((
        if sent {
            Settlement::MaybeSent
        } else {
            Settlement::NotSent
        },
        msg,
    ))
}

// ---------------------------------------------------------------------------
// Registry route status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryStatusView {
    pub enabled: bool,
    pub reason: String,
    pub missing: Vec<String>,
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), EscError> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| EscError::Storage(e.kind().to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| EscError::Storage(e.kind().to_string()))
}

/// Persist the endpoints (no keys) and the ledger (quotes are never persisted).
pub fn save_book(dir: &Path, b: &Book) -> Result<(), EscError> {
    std::fs::create_dir_all(dir).map_err(|e| EscError::Storage(e.kind().to_string()))?;
    let eps = serde_json::to_vec_pretty(&b.endpoints)
        .map_err(|_| EscError::Storage("encode endpoints".into()))?;
    write_atomic(&dir.join(ENDPOINTS_FILE), &eps)?;
    save_ledger(dir, &b.ledger)
}

/// Persist only the ledger (the write-ahead step before any egress).
pub fn save_ledger(dir: &Path, l: &Ledger) -> Result<(), EscError> {
    std::fs::create_dir_all(dir).map_err(|e| EscError::Storage(e.kind().to_string()))?;
    let led =
        serde_json::to_vec_pretty(l).map_err(|_| EscError::Storage("encode ledger".into()))?;
    write_atomic(&dir.join(LEDGER_FILE), &led)
}

/// Load the book. A missing ledger is a fresh install (the default cap). An unreadable one fails
/// closed: cap 0, `unreadable`, and the bad file is kept aside for inspection.
pub fn load_book(dir: &Path, now_ms: u64) -> Book {
    let ledger = match std::fs::read(dir.join(LEDGER_FILE)) {
        Ok(bytes) => match serde_json::from_slice::<Ledger>(&bytes) {
            Ok(mut l) => {
                l.roll(now_ms);
                l
            }
            Err(_) => {
                let _ = std::fs::rename(
                    dir.join(LEDGER_FILE),
                    dir.join(format!("ledger.unreadable-{now_ms}.json")),
                );
                let mut l = Ledger::new(0, now_ms);
                l.unreadable = true;
                l
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ledger::new(DEFAULT_DAILY_CAP_MICROS, now_ms)
        }
        Err(_) => {
            let mut l = Ledger::new(0, now_ms);
            l.unreadable = true;
            l
        }
    };
    let endpoints = std::fs::read(dir.join(ENDPOINTS_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Endpoint>>(&b).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|e| validate_base_url(&e.base_url).is_ok())
        .take(MAX_ENDPOINTS)
        .collect();
    let mut b = Book::new(ledger);
    b.endpoints = endpoints;
    b
}

// ---------------------------------------------------------------------------
// Tauri state and commands
// ---------------------------------------------------------------------------

/// Lazily loaded from the app data directory on first use.
#[derive(Default)]
pub struct EscalationState(Mutex<Option<(PathBuf, Book)>>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}-{:016x}", rand::random::<u64>())
}

fn with_book<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    f: impl FnOnce(&Path, &mut Book) -> Result<T, EscError>,
) -> Result<T, String> {
    use tauri::Manager;
    let st = app
        .try_state::<EscalationState>()
        .ok_or("internal: escalation state unavailable")?;
    let mut guard = st.0.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join(DIR_NAME);
        let book = load_book(&dir, now_ms());
        *guard = Some((dir, book));
    }
    let Some((dir, book)) = guard.as_mut() else {
        return Err("internal: escalation state unavailable".into());
    };
    f(dir, book).map_err(|e| e.to_string())
}

/// The budget as Settings shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub cap_micros: u64,
    pub used_micros: u64,
    pub remaining_micros: u64,
    pub confirmed_micros: u64,
    pub period_start_ms: u64,
    pub period_end_ms: u64,
    pub max_cap_micros: u64,
    pub unreadable: bool,
    pub history: Vec<SpendRecord>,
}

fn budget_view(l: &Ledger) -> BudgetView {
    BudgetView {
        cap_micros: l.cap_micros,
        used_micros: l.used(),
        remaining_micros: l.remaining(),
        confirmed_micros: l.confirmed_micros,
        period_start_ms: l.period.saturating_mul(DAY_MS),
        period_end_ms: l.period.saturating_add(1).saturating_mul(DAY_MS),
        max_cap_micros: MAX_DAILY_CAP_MICROS,
        unreadable: l.unreadable,
        history: l.history.iter().rev().take(20).cloned().collect(),
    }
}

/// **escalation_endpoints** — the member's endpoints (no keys).
#[tauri::command]
pub async fn escalation_endpoints(app: tauri::AppHandle) -> Result<Vec<EndpointView>, String> {
    crate::blocking::off_main(move || {
        with_book(&app, |_, b| {
            Ok(b.endpoints.iter().map(EndpointView::from).collect())
        })
    })
    .await
}

/// **escalation_endpoint_add** — add an endpoint; the key is sealed in the OS keyring.
#[tauri::command]
pub async fn escalation_endpoint_add(
    app: tauri::AppHandle,
    input: EndpointInput,
    api_key: String,
) -> Result<EndpointView, String> {
    let api_key = Zeroizing::new(api_key);
    crate::blocking::off_main(move || {
        with_book(&app, |dir, b| {
            let view = add_endpoint_with_key(
                b,
                &crate::ai::OsAiKeyring,
                &input,
                api_key.as_str(),
                new_id("ep"),
                now_ms(),
            )?;
            if let Err(e) = save_book(dir, b) {
                let _ = remove_endpoint_with_key(b, &crate::ai::OsAiKeyring, &view.id);
                return Err(e);
            }
            Ok(view)
        })
    })
    .await
}

/// **escalation_endpoint_remove** — delete the key and the endpoint.
#[tauri::command]
pub async fn escalation_endpoint_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        with_book(&app, |dir, b| {
            remove_endpoint_with_key(b, &crate::ai::OsAiKeyring, &id)?;
            save_book(dir, b)
        })
    })
    .await
}

/// **escalation_budget** — today's budget and recent escalations.
#[tauri::command]
pub async fn escalation_budget(app: tauri::AppHandle) -> Result<BudgetView, String> {
    crate::blocking::off_main(move || {
        with_book(&app, |_, b| {
            b.ledger.roll(now_ms());
            Ok(budget_view(&b.ledger))
        })
    })
    .await
}

/// **escalation_budget_set** — the member sets the daily cap (micro-USD).
#[tauri::command]
pub async fn escalation_budget_set(
    app: tauri::AppHandle,
    cap_micros: u64,
) -> Result<BudgetView, String> {
    crate::blocking::off_main(move || {
        with_book(&app, |dir, b| {
            if b.ledger.unreadable {
                // A fresh ledger replaces the unreadable one only by the member's explicit act.
                b.ledger = Ledger::new(0, now_ms());
            }
            b.ledger.set_cap(cap_micros, now_ms())?;
            save_ledger(dir, &b.ledger)?;
            Ok(budget_view(&b.ledger))
        })
    })
    .await
}

/// **escalation_quote** — price a request (nothing runs).
#[tauri::command]
pub async fn escalation_quote(
    app: tauri::AppHandle,
    endpoint_id: String,
    prompt: String,
    system: Option<String>,
    max_tokens: Option<u32>,
) -> Result<QuoteView, String> {
    crate::blocking::off_main(move || {
        with_book(&app, |_, b| {
            b.quote(
                &endpoint_id,
                &prompt,
                system.as_deref(),
                max_tokens.unwrap_or(DEFAULT_ESCALATION_TOKENS),
                now_ms(),
                new_id("q"),
            )
        })
    })
    .await
}

/// The result of a run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub escalation_id: String,
    pub content: String,
    pub destination: String,
    pub mode: Mode,
    pub charged_micros: u64,
    pub charged_label: String,
    pub usage_reported: bool,
    pub exceeded_quote: bool,
    pub remaining_micros: u64,
}

/// **escalation_run** — run a shown quote. `shown_cost_micros` must equal the quote. Over budget, or
/// with untrusted context, it needs `confirmed` (the member's explicit HIC-1 decision).
#[tauri::command]
pub async fn escalation_run(
    app: tauri::AppHandle,
    quote_id: String,
    shown_cost_micros: u64,
    confirmed: bool,
    tainted: bool,
) -> Result<RunView, String> {
    crate::blocking::off_main(move || {
        escalation_run_sync(&app, &quote_id, shown_cost_micros, confirmed, tainted)
    })
    .await
}

fn escalation_run_sync<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    quote_id: &str,
    shown_cost_micros: u64,
    confirmed: bool,
    tainted: bool,
) -> Result<RunView, String> {
    // 1. Authorize and reserve, write-ahead. Nothing leaves until the reservation is on disk.
    let auth = with_book(app, |dir, b| {
        let a = b.authorize(
            quote_id,
            shown_cost_micros,
            confirmed,
            tainted,
            now_ms(),
            new_id("esc"),
        )?;
        if let Err(e) = save_ledger(dir, &b.ledger) {
            b.settle(&a.escalation_id, Settlement::NotSent, now_ms());
            return Err(e);
        }
        Ok(a)
    })?;
    // 2. Read the key for this one request.
    let key = crate::ai::OsAiKeyring
        .get(&key_account(&auth.endpoint.id))
        .ok()
        .flatten()
        .map(Zeroizing::new);
    let outcome = match key {
        None => Err((
            Settlement::NotSent,
            "the key for this endpoint is missing from the OS keyring; remove and re-add the endpoint"
                .to_string(),
        )),
        Some(key) => {
            let body = sidecar_body(&auth, key.as_str());
            drop(key);
            match crate::hermes::sidecar_escalate(app, body.as_str()) {
                Ok((status, text)) => interpret_sidecar(status, &text),
                Err(sent) => Err((
                    if sent {
                        Settlement::MaybeSent
                    } else {
                        Settlement::NotSent
                    },
                    "the Hermes agent is not running; start it, then try again".to_string(),
                )),
            }
        }
    };
    // 3. Settle and persist.
    let (settlement, result) = match outcome {
        Ok((s, text)) => (s, Ok(text)),
        Err((s, msg)) => (s, Err(msg)),
    };
    let (rec, remaining) = with_book(app, |dir, b| {
        let rec = b.settle(&auth.escalation_id, settlement, now_ms());
        save_ledger(dir, &b.ledger)?;
        Ok((rec, b.ledger.remaining()))
    })?;
    let content = result?;
    Ok(RunView {
        escalation_id: rec.escalation_id,
        content,
        destination: rec.destination,
        mode: rec.mode,
        charged_micros: rec.charged_micros,
        charged_label: fmt_usd(rec.charged_micros),
        usage_reported: rec.usage_reported,
        exceeded_quote: rec.exceeded_quote,
        remaining_micros: remaining,
    })
}

#[cfg(test)]
mod tests {
    include!("escalation_tests.rs");
}
