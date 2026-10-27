//! D-11 (RC52) — the spend guard for cloud model calls. ADR-2026-10-26 P-7 (a) and (b).
//!
//! **Caps (P-7a).** Per provider: daily and monthly token caps and daily and monthly currency caps
//! (micro-USD), on by default ([`Caps::default`]: 1M / 10M tokens, $5 / $50). They are checked **before every call** against what
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

/// Out-of-the-box caps per provider, a hard stop. Owner sign-off 2026-10-26 (ADR P-7a, fed#394):
/// 1M tokens a day, 10M a month, $5 a day, $50 a month.
pub const DEFAULT_DAILY_TOKENS: u64 = 1_000_000;
pub const DEFAULT_MONTHLY_TOKENS: u64 = 10_000_000;
pub const DEFAULT_DAILY_MICROS: u64 = 5_000_000;
pub const DEFAULT_MONTHLY_MICROS: u64 = 50_000_000;
/// Per-vendor overrides of the default currency caps: `(provider id, daily, monthly)` micro-USD.
/// Owner decision amending P-7a: xAI reserves its full context window (about $6 per call), so its
/// day is $20; its month stays $50. Token caps are the same for every provider.
pub const DEFAULT_CURRENCY_CAPS: &[(&str, u64, u64)] = &[("xai", 20_000_000, 50_000_000)];
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
            daily_micros: Some(DEFAULT_DAILY_MICROS),
            monthly_micros: Some(DEFAULT_MONTHLY_MICROS),
        }
    }
}

impl Caps {
    /// The out-of-the-box caps for `provider`.
    pub fn default_for(provider: &str) -> Caps {
        let base = Caps::default();
        match DEFAULT_CURRENCY_CAPS
            .iter()
            .find(|(p, _, _)| *p == provider)
        {
            Some(&(_, daily, monthly)) => Caps {
                daily_micros: Some(daily),
                monthly_micros: Some(monthly),
                ..base
            },
            None => base,
        }
    }

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

impl ProviderGuard {
    /// A provider seen for the first time: its vendor's default caps.
    pub fn new_for(provider: &str) -> ProviderGuard {
        ProviderGuard {
            caps: Caps::default_for(provider),
            ..ProviderGuard::default()
        }
    }
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
    let (y, m, _) = civil_of(ms);
    (y, m)
}

/// UTC (year, month, day of month) of a millisecond timestamp.
pub fn civil_of(ms: u64) -> (i64, u32, u32) {
    // Civil-from-days (Howard Hinnant), UTC.
    let z = (ms / DAY_MS) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
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
    /// The keyring holding the anchor (the file's generation). `None` only in unit tests that
    /// never save.
    keyring: Option<std::sync::Arc<dyn AiKeyring>>,
    /// The generation of the file this book was loaded from (or last saved).
    generation: u64,
}

impl GuardBook {
    pub fn get(&self, provider: &str) -> ProviderGuard {
        let mut g = self
            .providers
            .get(provider)
            .cloned()
            .unwrap_or_else(|| ProviderGuard::new_for(provider));
        if self.unreadable && g.halt.is_none() {
            g.halt = Some(Halt {
                reason: HaltReason::GuardUnreadable,
                at_ms: 0,
            });
        }
        g
    }

    fn entry(&mut self, provider: &str) -> &mut ProviderGuard {
        self.providers
            .entry(provider.to_string())
            .or_insert_with(|| ProviderGuard::new_for(provider))
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
        let g = self.get(&plan.provider);
        if let Some(h) = g.halt {
            return Err(Refusal::Halted(h));
        }
        if !self.model_known(&plan.provider, &plan.model, adapter_models) {
            let h = self.halt(
                &plan.provider,
                HaltReason::UnknownModel {
                    model: plan.model.clone(),
                },
                now_ms,
            );
            return Err(Refusal::Halted(h));
        }
        if plan.reserve_in > SPIKE_INPUT_CEILING {
            let h = self.halt(
                &plan.provider,
                HaltReason::TokenSpike {
                    tokens: plan.reserve_in,
                    threshold: SPIKE_INPUT_CEILING,
                },
                now_ms,
            );
            return Err(Refusal::Halted(h));
        }
        self.burst_check(&plan.provider, now_ms)?;
        let caps = g.caps;
        let call_tokens = plan.reserve_tokens();
        for (which, used, cap) in [
            ("daily token", today.tokens, caps.daily_tokens),
            ("monthly token", month.tokens, caps.monthly_tokens),
        ] {
            if let Some(cap) = cap {
                if used.saturating_add(call_tokens) > cap {
                    return Err(Refusal::CapExceeded {
                        which,
                        used,
                        call: call_tokens,
                        cap,
                    });
                }
            }
        }
        if caps.daily_micros.is_some() || caps.monthly_micros.is_some() {
            let call_micros = plan.reserve_micros().ok_or(Refusal::PriceUnknown)?;
            for (which, used, cap) in [
                ("daily spend", today.micros, caps.daily_micros),
                ("monthly spend", month.micros, caps.monthly_micros),
            ] {
                if let Some(cap) = cap {
                    if used.saturating_add(call_micros) > cap {
                        return Err(Refusal::CapExceeded {
                            which,
                            used,
                            call: call_micros,
                            cap,
                        });
                    }
                }
            }
        }
        self.entry(&plan.provider).recent_calls.push_back(now_ms);
        Ok(())
    }

    /// Halt (and refuse) when this call would make a burst. Does not record the call.
    fn burst_check(&mut self, provider: &str, now_ms: u64) -> Result<(), Refusal> {
        let e = self.entry(provider);
        while e
            .recent_calls
            .front()
            .is_some_and(|t| now_ms.saturating_sub(*t) >= BURST_WINDOW_MS || *t > now_ms)
        {
            e.recent_calls.pop_front();
        }
        if e.recent_calls.len() >= BURST_MAX_CALLS {
            let calls = e.recent_calls.len() as u64 + 1;
            let h = self.halt(
                provider,
                HaltReason::Burst {
                    calls,
                    window_ms: BURST_WINDOW_MS,
                },
                now_ms,
            );
            return Err(Refusal::Halted(h));
        }
        Ok(())
    }

    /// The guard for a live key test: refused while halted (when `respect_halt`), and counted
    /// toward the burst halt like a model call.
    pub fn admit_key_test(
        &mut self,
        provider: &str,
        respect_halt: bool,
        now_ms: u64,
    ) -> Result<(), Refusal> {
        if respect_halt {
            if let Some(h) = self.get(provider).halt {
                return Err(Refusal::Halted(h));
            }
        }
        self.burst_check(provider, now_ms)?;
        self.entry(provider).recent_calls.push_back(now_ms);
        Ok(())
    }

    /// After a call finished: record its tokens; halt on a spike against the trailing mean.
    /// Returns the halt when one was raised.
    pub fn observe(&mut self, provider: &str, total_tokens: u64, now_ms: u64) -> Option<Halt> {
        let e = self.entry(provider);
        let n = e.recent_totals.len();
        let spike = if n >= SPIKE_MIN_SAMPLES {
            let mean = e.recent_totals.iter().sum::<u64>() / n as u64;
            let threshold = mean.saturating_mul(SPIKE_FACTOR).max(SPIKE_FLOOR);
            (total_tokens > threshold).then_some(threshold)
        } else {
            None
        };
        e.recent_totals.push_back(total_tokens);
        while e.recent_totals.len() > SPIKE_WINDOW {
            e.recent_totals.pop_front();
        }
        spike.map(|threshold| {
            self.halt(
                provider,
                HaltReason::TokenSpike {
                    tokens: total_tokens,
                    threshold,
                },
                now_ms,
            )
        })
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

    /// The stored key passed a live test: refresh the listed models. The date the key was set is
    /// kept (it drives the rotation reminder).
    pub fn key_tested(&mut self, provider: &str, listed: Vec<String>) {
        self.entry(provider).listed_models = listed;
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
    /// Bumped on every save and mirrored in the keyring anchor: a missing file, or an older one,
    /// is detected.
    #[serde(default)]
    generation: u64,
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

/// The highest generation of any guard file in `dir`: the live `guard.json` and the copies kept
/// aside as `guard.unreadable-*.json`. Read without checking the seal (a higher number only makes
/// more files fail to load, never fewer). Used when a log reset has to re-create a missing anchor,
/// so that no guard file saved before the reset can load as current.
pub(crate) fn highest_generation_on_disk(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n == GUARD_FILE || (n.starts_with("guard.unreadable-") && n.ends_with(".json"))
        })
        .filter_map(|e| std::fs::read(e.path()).ok())
        .filter_map(|b| serde_json::from_slice::<Sealed>(&b).ok())
        .filter_map(|s| serde_json::from_str::<GuardFile>(&s.body).ok())
        .map(|f| f.generation)
        .max()
        .unwrap_or(0)
}

/// Replace the guard's seal key with a new random one: every guard file sealed before fails its
/// seal. Used by a log reset that had to re-create a missing anchor.
pub(crate) fn rotate_mac_key(keyring: &dyn AiKeyring) -> Result<(), ()> {
    let k = Zeroizing::new(rand::random::<[u8; 32]>().to_vec());
    keyring
        .set(GUARD_MAC_ACCOUNT, &hex::encode(k.as_slice()))
        .map_err(|_| ())
}

pub fn guard_path(dir: &Path) -> PathBuf {
    dir.join(GUARD_FILE)
}

/// Load the guard. A fresh install (no file, no anchor): defaults. Fails closed (`unreadable`, every
/// provider halted until the member resumes) when the keyring cannot be read, or the file is
/// unreadable, edited or unsealed, or missing while the anchor says one was saved, or older than
/// the anchored generation (a rolled-back copy). A file *newer* than the anchor is a crash between
/// the two writes and is accepted (it cannot be sealed without the key).
pub fn load(dir: &Path, keyring: std::sync::Arc<dyn AiKeyring>, now_ms: u64) -> GuardBook {
    use hmac::Mac as _;
    let failed = |key: Option<Zeroizing<Vec<u8>>>,
                  keyring: std::sync::Arc<dyn AiKeyring>,
                  generation| GuardBook {
        unreadable: true,
        mac_key: key,
        keyring: Some(keyring),
        generation,
        ..GuardBook::default()
    };
    let (Ok(key), Ok(anchor)) = (
        mac_key(keyring.as_ref(), GUARD_MAC_ACCOUNT),
        crate::provider_usage_log::read_anchor_opt(keyring.as_ref()),
    ) else {
        return failed(None, keyring, 0);
    };
    let path = guard_path(dir);
    // A saved guard file always has an anchor next to it; one without it is not trusted.
    let Some(anchor) = anchor else {
        return if path.exists() {
            failed(Some(key), keyring, 0)
        } else {
            GuardBook {
                mac_key: Some(key),
                keyring: Some(keyring),
                ..GuardBook::default()
            }
        };
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && anchor.guard_gen == 0 => {
            return GuardBook {
                mac_key: Some(key),
                keyring: Some(keyring),
                ..GuardBook::default()
            }
        }
        Err(_) => return failed(Some(key), keyring, anchor.guard_gen),
    };
    let parsed = serde_json::from_slice::<Sealed>(&bytes).ok().and_then(|s| {
        let tag = hex::decode(&s.mac).ok()?;
        mac_of(&key, &s.body)?.verify_slice(&tag).ok()?;
        (s.v == SEALED_VERSION).then_some(())?;
        serde_json::from_str::<GuardFile>(&s.body).ok()
    });
    match parsed {
        Some(f) if f.generation >= anchor.guard_gen => GuardBook {
            providers: f.providers,
            customs: f.customs,
            unreadable: false,
            mac_key: Some(key),
            keyring: Some(keyring),
            generation: f.generation,
        },
        _ => {
            let _ = std::fs::rename(&path, dir.join(format!("guard.unreadable-{now_ms}.json")));
            failed(Some(key), keyring, anchor.guard_gen)
        }
    }
}

/// Persist the guard, sealed, with the next generation; then move the anchor to it.
pub fn save(dir: &Path, g: &mut GuardBook) -> Result<(), String> {
    use hmac::Mac as _;
    if g.unreadable {
        return Err("the spend settings could not be verified; resume a provider first".into());
    }
    let key = g
        .mac_key
        .as_ref()
        .ok_or("the spend settings key is unavailable")?;
    let keyring = g
        .keyring
        .clone()
        .ok_or("the spend settings key is unavailable")?;
    // Checked before the file is written: with the anchor missing nothing is saved, so the halt
    // stays until the member resets the usage log (which restores the anchor).
    let anchored = crate::provider_usage_log::read_anchor_opt(keyring.as_ref())
        .map_err(|_| "the keychain could not be read".to_string())?
        .ok_or("start a new usage log first: its record of past use is missing")?
        .guard_gen;
    let generation = g.generation.max(anchored).saturating_add(1);
    let body = serde_json::to_string(&GuardFile {
        generation,
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
    // The directory is owner-only; the file is replaced through a temp name unique to this write.
    citrate_core_kit::fsutil::ensure_private_dir(dir).map_err(|e| e.kind().to_string())?;
    crate::atomic_file::replace(&guard_path(dir), &sealed).map_err(|e| e.kind().to_string())?;
    g.generation = generation;
    crate::provider_usage_log::update_anchor(keyring.as_ref(), |a| a.guard_gen = generation)
        .map_err(|_| "the spend settings anchor could not be written".to_string())
}

#[cfg(test)]
mod tests {
    include!("provider_guard_tests.rs");
}
