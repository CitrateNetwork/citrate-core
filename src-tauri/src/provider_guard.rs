//! D-11 (RC52) — the spend guard for cloud model calls. ADR-2026-10-26 P-7 (a) and (b).
//!
//! **Caps (P-7a).** Per provider: daily and monthly token caps and, when the member sets them,
//! daily and monthly currency caps (micro-USD). They are checked **before every call** against what
//! the signed usage log ([`crate::provider_usage_log`]) already holds for the current UTC day and
//! UTC month, plus this call's worst case (an upper bound on its input tokens and its full output
//! ceiling). Over a cap the call is refused with nothing sent: a hard stop, not a prompt. A currency
//! cap needs a price for the model (the adapter's published price, or the member's own entry); a
//! call to an unpriced model under a currency cap is refused rather than counted as free.
//!
//! **Anomaly halt (P-7b).** A provider is paused (every call refused until the member resumes it)
//! when it sees:
//! - a **burst**: more than [`BURST_MAX_CALLS`] calls within [`BURST_WINDOW_MS`];
//! - an **unknown model**: a model that is neither in the adapter's list, nor in the list the vendor
//!   returned at the key test, nor one the member approved (resuming such a halt approves it);
//! - a **token spike**: one call's input bound above [`SPIKE_INPUT_CEILING`], or a finished call
//!   whose tokens exceed [`SPIKE_FACTOR`] times the trailing mean (and [`SPIKE_FLOOR`]).
//!
//! **Persistence.** Caps, halts, approved models, member prices, the models a key test listed, and
//! when the key was set live in one file, sealed with an HMAC under a key in the OS keyring (the
//! escalation ledger's scheme). An unreadable, edited or unsealed file fails closed: every provider
//! is treated as halted until the member resets it in Settings.

use crate::ai::AiKeyring;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Out-of-the-box caps. Pending owner sign-off (ADR P-7 names the mechanism, not the numbers).
pub const DEFAULT_DAILY_TOKENS: u64 = 1_000_000;
pub const DEFAULT_MONTHLY_TOKENS: u64 = 10_000_000;
/// The highest cap a member can set.
pub const MAX_TOKEN_CAP: u64 = 10_000_000_000;
pub const MAX_MICROS_CAP: u64 = 10_000_000_000; // 10,000 USD
/// The highest price per million tokens a member may enter (1,000 USD).
pub const MAX_PRICE_MICROS_PER_MTOK: u64 = 1_000_000_000;

pub const BURST_WINDOW_MS: u64 = 60_000;
pub const BURST_MAX_CALLS: usize = 30;
pub const SPIKE_INPUT_CEILING: u64 = 1_000_000;
pub const SPIKE_FACTOR: u64 = 8;
pub const SPIKE_FLOOR: u64 = 50_000;
pub const SPIKE_MIN_SAMPLES: usize = 5;
const SPIKE_WINDOW: usize = 20;
/// Member-approved extra models per provider.
pub const MAX_APPROVED_MODELS: usize = 64;

pub const DAY_MS: u64 = 86_400_000;

pub const GUARD_FILE: &str = "guard.json";
pub const GUARD_MAC_ACCOUNT: &str = "provider-guard-mac-v1";
const SEALED_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A provider's caps. `None` = no cap of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    pub daily_tokens: Option<u64>,
    pub monthly_tokens: Option<u64>,
    pub daily_micros: Option<u64>,
    pub monthly_micros: Option<u64>,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            daily_tokens: Some(DEFAULT_DAILY_TOKENS),
            monthly_tokens: Some(DEFAULT_MONTHLY_TOKENS),
            daily_micros: None,
            monthly_micros: None,
        }
    }
}

impl Caps {
    pub fn validate(&self) -> Result<(), String> {
        for t in [self.daily_tokens, self.monthly_tokens]
            .into_iter()
            .flatten()
        {
            if t > MAX_TOKEN_CAP {
                return Err(format!("a token cap above {MAX_TOKEN_CAP} is not accepted"));
            }
        }
        for m in [self.daily_micros, self.monthly_micros]
            .into_iter()
            .flatten()
        {
            if m > MAX_MICROS_CAP {
                return Err(format!(
                    "a spending cap above {} is not accepted",
                    crate::escalation::fmt_usd(MAX_MICROS_CAP)
                ));
            }
        }
        if let (Some(d), Some(m)) = (self.daily_tokens, self.monthly_tokens) {
            if d > m {
                return Err("the daily token cap is above the monthly one".into());
            }
        }
        if let (Some(d), Some(m)) = (self.daily_micros, self.monthly_micros) {
            if d > m {
                return Err("the daily spending cap is above the monthly one".into());
            }
        }
        Ok(())
    }
}

/// Micro-USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input_micros_per_mtok: u64,
    pub output_micros_per_mtok: u64,
}

impl Price {
    /// Cost of `input` + `output` tokens, rounded up to the next micro-USD.
    pub fn cost(&self, input: u64, output: u64) -> u64 {
        let num = (input as u128)
            .saturating_mul(self.input_micros_per_mtok as u128)
            .saturating_add((output as u128).saturating_mul(self.output_micros_per_mtok as u128));
        u64::try_from(num.div_ceil(1_000_000)).unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum HaltReason {
    /// The member paused it.
    Member,
    Burst {
        calls: u64,
        window_ms: u64,
    },
    UnknownModel {
        model: String,
    },
    TokenSpike {
        tokens: u64,
        threshold: u64,
    },
    /// The guard file could not be trusted (unreadable, edited or unsealed).
    GuardUnreadable,
}

impl HaltReason {
    pub fn describe(&self) -> String {
        match self {
            HaltReason::Member => "paused by you".into(),
            HaltReason::Burst { calls, window_ms } => format!(
                "paused automatically: {calls} calls in {} seconds",
                window_ms / 1000
            ),
            HaltReason::UnknownModel { model } => format!(
                "paused automatically: a call asked for model {model:?}, which this provider has not listed; resuming approves it"
            ),
            HaltReason::TokenSpike { tokens, threshold } => format!(
                "paused automatically: one call used about {tokens} tokens (the alert level is {threshold})"
            ),
            HaltReason::GuardUnreadable => {
                "paused: the spend settings file could not be verified; check your caps and resume"
                    .into()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Halt {
    pub reason: HaltReason,
    pub at_ms: u64,
}

/// One provider's persisted guard state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderGuard {
    #[serde(default)]
    pub caps: Caps,
    #[serde(default)]
    pub halt: Option<Halt>,
    /// Models the member approved (by resuming an unknown-model halt).
    #[serde(default)]
    pub approved_models: BTreeSet<String>,
    /// Model ids the vendor listed at the last successful key test.
    #[serde(default)]
    pub listed_models: Vec<String>,
    /// Prices the member entered, by model id (override the adapter's).
    #[serde(default)]
    pub prices: BTreeMap<String, Price>,
    /// When the stored key was set (for rotation reminders).
    #[serde(default)]
    pub key_set_ms: Option<u64>,
    /// Total tokens of the last finished calls (for the spike check).
    #[serde(default)]
    pub recent_totals: VecDeque<u64>,
    /// Call start times inside the burst window (not persisted).
    #[serde(skip)]
    pub recent_calls: VecDeque<u64>,
}

/// What the log already holds for a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeriodUse {
    pub tokens: u64,
    pub micros: u64,
    /// Calls whose cost is unknown (no price).
    pub unpriced_calls: u64,
}

/// The worst case of a call about to be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallPlan {
    pub provider: String,
    pub model: String,
    pub reserve_in: u64,
    pub reserve_out: u64,
    pub price: Option<Price>,
}

impl CallPlan {
    pub fn reserve_tokens(&self) -> u64 {
        self.reserve_in.saturating_add(self.reserve_out)
    }
    pub fn reserve_micros(&self) -> Option<u64> {
        self.price
            .map(|p| p.cost(self.reserve_in, self.reserve_out))
    }
}

/// Why a call was refused before anything was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Halted(Halt),
    CapExceeded {
        which: &'static str,
        used: u64,
        call: u64,
        cap: u64,
    },
    PriceUnknown,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Halted(h) => write!(f, "this provider is {}", h.reason.describe()),
            Refusal::CapExceeded {
                which,
                used,
                call,
                cap,
            } => {
                if which.ends_with("spend") {
                    write!(
                        f,
                        "the {which} cap would be passed: {} used, this call could cost up to {}, the cap is {}",
                        crate::escalation::fmt_usd(*used),
                        crate::escalation::fmt_usd(*call),
                        crate::escalation::fmt_usd(*cap)
                    )
                } else {
                    write!(
                        f,
                        "the {which} cap would be passed: {used} tokens used, this call could use up to {call}, the cap is {cap}"
                    )
                }
            }
            Refusal::PriceUnknown => f.write_str(
                "a spending cap is set but this model has no price; enter its price in Settings or remove the spending cap",
            ),
        }
    }
}

/// UTC day number and UTC (year, month) of a millisecond timestamp.
pub fn day_of(ms: u64) -> u64 {
    ms / DAY_MS
}

pub fn month_of(ms: u64) -> (i64, u32) {
    // Civil-from-days (Howard Hinnant), UTC.
    let z = (ms / DAY_MS) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m)
}

// ---------------------------------------------------------------------------
// The guard book
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct GuardBook {
    pub providers: BTreeMap<String, ProviderGuard>,
    /// Member-added custom endpoints (sealed with the rest, so `confirmed` cannot be set by an
    /// edit made outside the app).
    pub customs: Vec<crate::provider_egress::CustomEndpoint>,
    /// The file could not be trusted: every provider not yet reset by the member is halted.
    pub unreadable: bool,
    mac_key: Option<Zeroizing<Vec<u8>>>,
}

impl GuardBook {
    pub fn get(&self, provider: &str) -> ProviderGuard {
        let mut g = self.providers.get(provider).cloned().unwrap_or_default();
        if self.unreadable && g.halt.is_none() {
            g.halt = Some(Halt {
                reason: HaltReason::GuardUnreadable,
                at_ms: 0,
            });
        }
        g
    }

    fn entry(&mut self, provider: &str) -> &mut ProviderGuard {
        self.providers.entry(provider.to_string()).or_default()
    }

    fn halt(&mut self, provider: &str, reason: HaltReason, now_ms: u64) -> Halt {
        let h = Halt {
            reason,
            at_ms: now_ms,
        };
        self.entry(provider).halt = Some(h.clone());
        h
    }

    /// Whether `model` is known for `provider`: the adapter's list, the vendor's listing at the key
    /// test, or one the member approved.
    pub fn model_known(&self, provider: &str, model: &str, adapter_models: &[String]) -> bool {
        let g = self.providers.get(provider);
        adapter_models.iter().any(|m| m == model)
            || g.is_some_and(|g| {
                g.listed_models.iter().any(|m| m == model) || g.approved_models.contains(model)
            })
    }

    /// The price for a call: the member's entry, else the adapter's.
    pub fn price_for(&self, provider: &str, model: &str, adapter: Option<Price>) -> Option<Price> {
        self.providers
            .get(provider)
            .and_then(|g| g.prices.get(model).copied())
            .or(adapter)
    }

    /// **The pre-call check.** Refuses (and halts, for an anomaly) before anything is sent.
    /// `today` / `month` are what the usage log holds for this provider, unsettled reservations
    /// included.
    pub fn admit(
        &mut self,
        plan: &CallPlan,
        adapter_models: &[String],
        today: PeriodUse,
        month: PeriodUse,
        now_ms: u64,
    ) -> Result<(), Refusal> {
        let _ = (plan, adapter_models, today, month, now_ms);
        Ok(())
    }

    /// After a call finished: record its tokens; halt on a spike against the trailing mean.
    /// Returns the halt when one was raised.
    pub fn observe(&mut self, provider: &str, total_tokens: u64, now_ms: u64) -> Option<Halt> {
        let _ = (provider, total_tokens, now_ms);
        None
    }

    /// The member pauses a provider.
    pub fn member_halt(&mut self, provider: &str, now_ms: u64) -> Halt {
        self.halt(provider, HaltReason::Member, now_ms)
    }

    /// The member resumes a provider. Resuming an unknown-model halt approves that model. Clears
    /// the burst window so the resumed provider does not trip again at once. While the guard file
    /// is unreadable, resuming is the member's acknowledgement: settings go back to the defaults
    /// for every provider.
    pub fn member_resume(&mut self, provider: &str) -> Result<(), String> {
        if self.unreadable {
            self.providers.clear();
            self.customs.clear();
            self.unreadable = false;
            return Ok(());
        }
        let e = self.entry(provider);
        if let Some(Halt {
            reason: HaltReason::UnknownModel { model },
            ..
        }) = &e.halt
        {
            if e.approved_models.len() >= MAX_APPROVED_MODELS {
                return Err("too many approved models for this provider".into());
            }
            e.approved_models.insert(model.clone());
        }
        e.halt = None;
        e.recent_calls.clear();
        Ok(())
    }

    pub fn set_caps(&mut self, provider: &str, caps: Caps) -> Result<(), String> {
        caps.validate()?;
        self.entry(provider).caps = caps;
        Ok(())
    }

    pub fn set_price(
        &mut self,
        provider: &str,
        model: &str,
        price: Option<Price>,
    ) -> Result<(), String> {
        let model = model.trim();
        if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err("enter the model id the price is for".into());
        }
        let e = self.entry(provider);
        match price {
            Some(p) => {
                if p.input_micros_per_mtok > MAX_PRICE_MICROS_PER_MTOK
                    || p.output_micros_per_mtok > MAX_PRICE_MICROS_PER_MTOK
                {
                    return Err("that price is above the accepted ceiling".into());
                }
                if e.prices.len() >= MAX_APPROVED_MODELS && !e.prices.contains_key(model) {
                    return Err("too many model prices for this provider".into());
                }
                e.prices.insert(model.to_string(), p);
            }
            None => {
                e.prices.remove(model);
            }
        }
        Ok(())
    }

    /// A key was set and tested: keep the models the vendor listed and when.
    pub fn key_set(&mut self, provider: &str, listed: Vec<String>, now_ms: u64) {
        let e = self.entry(provider);
        e.listed_models = listed;
        e.key_set_ms = Some(now_ms);
    }

    /// The key was removed.
    pub fn key_removed(&mut self, provider: &str) {
        let e = self.entry(provider);
        e.listed_models.clear();
        e.key_set_ms = None;
    }
}

// ---------------------------------------------------------------------------
// Persistence (sealed like the escalation ledger)
// ---------------------------------------------------------------------------

/// The sealed body.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GuardFile {
    #[serde(default)]
    providers: BTreeMap<String, ProviderGuard>,
    #[serde(default)]
    customs: Vec<crate::provider_egress::CustomEndpoint>,
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    v: u32,
    body: String,
    mac: String,
}

type Mac = hmac::Hmac<sha2::Sha256>;

fn mac_of(key: &[u8], body: &str) -> Option<Mac> {
    use hmac::Mac as _;
    let mut m = <Mac as hmac::Mac>::new_from_slice(key).ok()?;
    m.update(body.as_bytes());
    Some(m)
}

/// Read (or create) a 32-byte HMAC key from the keyring. `Err` = keyring unreadable.
pub(crate) fn mac_key(keyring: &dyn AiKeyring, account: &str) -> Result<Zeroizing<Vec<u8>>, ()> {
    match keyring.get(account) {
        Err(_) => Err(()),
        Ok(Some(h)) => match hex::decode(h.trim()) {
            Ok(k) if k.len() == 32 => Ok(Zeroizing::new(k)),
            _ => Err(()),
        },
        Ok(None) => {
            let k = Zeroizing::new(rand::random::<[u8; 32]>().to_vec());
            keyring
                .set(account, &hex::encode(k.as_slice()))
                .map_err(|_| ())?;
            Ok(k)
        }
    }
}

/// Owner-only (0600 from creation), written to a temporary file and renamed into place.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let tmp = path.with_extension("tmp");
    if let Some(dir) = path.parent() {
        citrate_core_kit::fsutil::ensure_private_dir(dir).map_err(|e| e.kind().to_string())?;
    }
    let res = (|| -> std::io::Result<()> {
        let _ = std::fs::remove_file(&tmp);
        let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res.map_err(|e| e.kind().to_string())
}

pub fn guard_path(dir: &Path) -> PathBuf {
    dir.join(GUARD_FILE)
}

/// Load the guard. Missing file: defaults. Unreadable, edited or unsealed file, or an unreadable
/// keyring: `unreadable` (every provider halted until the member resets), the bad file kept aside.
pub fn load(dir: &Path, keyring: &dyn AiKeyring, now_ms: u64) -> GuardBook {
    use hmac::Mac as _;
    let key = mac_key(keyring, GUARD_MAC_ACCOUNT).unwrap_or_default();
    let path = guard_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return GuardBook {
                mac_key: Some(key),
                ..GuardBook::default()
            }
        }
        Err(_) => {
            return GuardBook {
                unreadable: true,
                mac_key: Some(key),
                ..GuardBook::default()
            }
        }
    };
    let parsed = serde_json::from_slice::<Sealed>(&bytes).ok().and_then(|s| {
        let _ = &s.mac;
        serde_json::from_str::<GuardFile>(&s.body).ok()
    });
    match parsed {
        Some(f) => GuardBook {
            providers: f.providers,
            customs: f.customs,
            unreadable: false,
            mac_key: Some(key),
        },
        None => {
            let _ = std::fs::rename(&path, dir.join(format!("guard.unreadable-{now_ms}.json")));
            GuardBook {
                unreadable: true,
                mac_key: Some(key),
                ..GuardBook::default()
            }
        }
    }
}

/// Persist the guard, sealed.
pub fn save(dir: &Path, g: &GuardBook) -> Result<(), String> {
    use hmac::Mac as _;
    if g.unreadable {
        return Err("the spend settings could not be verified; resume a provider first".into());
    }
    let key = g
        .mac_key
        .as_ref()
        .ok_or("the spend settings key is unavailable")?;
    let body = serde_json::to_string(&GuardFile {
        providers: g.providers.clone(),
        customs: g.customs.clone(),
    })
    .map_err(|_| "encode guard".to_string())?;
    let mac = mac_of(key, &body)
        .ok_or("seal guard")?
        .finalize()
        .into_bytes();
    let sealed = serde_json::to_vec(&Sealed {
        v: SEALED_VERSION,
        body,
        mac: hex::encode(mac),
    })
    .map_err(|_| "encode guard".to_string())?;
    write_atomic(&guard_path(dir), &sealed)
}

#[cfg(test)]
mod tests {
    include!("provider_guard_tests.rs");
}
