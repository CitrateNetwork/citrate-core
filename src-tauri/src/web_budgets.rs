//! HUP-S2.3 — Settings → Budgets and the budgeted Sign-In with Ethereum entry point.
//!
//! ADR-2026-09-30-rule3-budgetable-signatures (accepted). The store and the one budgeted signer
//! path live in the kit (`citrate_core_kit::web_budget`, `SignatureCeremony::request_siwe_budgeted`);
//! this module is the app's command surface over them:
//!
//! | command | what it does |
//! |---|---|
//! | `web_budget_status` | budgets, decision records, store health, placeholder defaults, ceilings |
//! | `web_budget_grant` | the member grants a per-origin sign-in budget (HIC-1, no signature, O-4) |
//! | `web_budget_revoke` / `web_budget_revoke_all` | immediate revocation under the budget lock |
//! | `web_budget_reset` | after an integrity failure: keep the old file aside, start empty |
//! | `web_signing_request` | a SIWE request: auto-signed inside a budget, otherwise an HIC-1 card |
//!
//! **Honest state today.** Auto-signing needs core to attest the page's top-frame origin over its
//! own session with the managed browser (D2 #1). That browser is HUP-S5.1 and does not exist yet,
//! so [`attest_origin`] returns `None` and every `web_signing_request` becomes an ordinary HIC-1
//! card that says why. Budgets can be granted, viewed and revoked now; they start to apply when
//! the attestation source lands, with no other change.
//!
//! **Defaults change nothing.** There are no budgets until the member grants one. The values the
//! grant form proposes are placeholders pending owner sign-off (see `web_budget` constants).

use std::path::PathBuf;
use std::sync::OnceLock;

use citrate_core_kit::ceremony::{BudgetedOutcome, SignatureCeremony, SiweSignRequest};
use citrate_core_kit::custody::{CustodyVault, OsKeyring};
use citrate_core_kit::web_budget::{
    self, BudgetGate, BudgetSnapshot, OriginAttestation, TaskTaint, WebSigningBudget,
    DEFAULT_PRINCIPAL,
};
use serde::{Deserialize, Serialize};

/// The budget file, in the app data dir.
pub const BUDGET_FILE_NAME: &str = "web-signing-budgets.json";
/// Why no origin can be attested yet. Shown in Settings → Budgets.
pub const ATTESTATION_UNAVAILABLE_REASON: &str = "Automatic sign-in needs the managed browser, which is not in this build yet. Until then every sign-in request asks you, even for sites with a budget.";
/// The caller's claimed origin is display text only; it is truncated to this many chars.
pub const MAX_CLAIMED_ORIGIN_CHARS: usize = 200;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Managed state: the budget file path and the store, opened on first use (off the main thread,
/// because opening reads the OS keychain).
pub struct WebBudgetState {
    path: Option<PathBuf>,
    gate: OnceLock<BudgetGate>,
}

impl WebBudgetState {
    /// `path` is `None` when the app data dir is unavailable; the store then reports itself failed.
    pub fn new(path: Option<PathBuf>) -> Self {
        WebBudgetState {
            path,
            gate: OnceLock::new(),
        }
    }

    fn gate(&self) -> &BudgetGate {
        self.gate.get_or_init(|| {
            let path = self.path.clone().unwrap_or_else(|| {
                PathBuf::from("/nonexistent/citrate-core").join(BUDGET_FILE_NAME)
            });
            BudgetGate::open(
                path,
                Box::new(OsKeyring::with_service(crate::CUSTODY_KEYRING_SERVICE)),
                now_ms(),
            )
        })
    }
}

/// Build the managed state from the app handle.
pub fn build_web_budget_state(app: &tauri::AppHandle) -> WebBudgetState {
    use tauri::Manager;
    WebBudgetState::new(
        app.path()
            .app_data_dir()
            .ok()
            .map(|d| d.join(BUDGET_FILE_NAME)),
    )
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// D2 #1: the top-frame origin as core reads it over its OWN session with the managed browser.
/// The managed browser (HUP-S5.1) is not built yet, so nothing can be attested and this returns
/// `None` for every tab. The caller's claimed origin is never used here.
pub fn attest_origin(_tab_id: Option<&str>) -> Option<OriginAttestation> {
    None
}

/// The taint a main-window `web_signing_request` carries.
///
/// The caller's list can only ADD taint. A caller that says "nothing untrusted" is not believed:
/// any script in the main webview can call this command, so a clean task has to be established by
/// core itself (the loop principal's own taint record, HUP-S2.7), which is not wired to this
/// command. Until it is, a request from the window is never clean and so never auto-signs, even
/// after origin attestation (HUP-S5.1) lands. This must stay true before attestation is enabled.
pub fn window_taint(sources: Option<Vec<String>>) -> TaskTaint {
    match sources {
        Some(v) if !v.is_empty() => TaskTaint::Sources(v),
        _ => TaskTaint::Unknown,
    }
}

/// Whole days to ms, inside the O-2 ceiling (1 to 30 days).
pub fn ttl_days_to_ms(days: u32) -> Result<u64, String> {
    let ms = u64::from(days) * DAY_MS;
    if days == 0 || ms > web_budget::MAX_TTL_MS_CEILING {
        return Err("a sign-in budget lasts 1 to 30 days".into());
    }
    Ok(ms)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttestationStatus {
    pub available: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantDefaults {
    pub max_count: u32,
    pub ttl_days: u32,
    /// The two values above are placeholders until the owner signs off on defaults.
    pub pending_owner_signoff: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrantCeilings {
    pub max_count: u32,
    pub ttl_days: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimits {
    pub min_gap_seconds: u64,
    pub window_max: u32,
    pub window_hours: u64,
}

/// Everything Settings → Budgets renders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebBudgetStatus {
    pub snapshot: BudgetSnapshot,
    pub attestation: AttestationStatus,
    pub defaults: GrantDefaults,
    pub ceilings: GrantCeilings,
    pub rate: RateLimits,
    /// The active wallet (lower-case), if one is unlocked.
    pub wallet_address: Option<String>,
    /// Epoch ms at read time, so the UI counts down from the same clock.
    pub now_ms: u64,
}

pub fn status_of(gate: &BudgetGate, now: u64, wallet: Option<&str>) -> WebBudgetStatus {
    WebBudgetStatus {
        snapshot: gate.snapshot(now, wallet),
        attestation: AttestationStatus {
            available: attest_origin(None).is_some(),
            reason: ATTESTATION_UNAVAILABLE_REASON.to_string(),
        },
        defaults: GrantDefaults {
            max_count: web_budget::PLACEHOLDER_DEFAULT_MAX_COUNT,
            ttl_days: web_budget::PLACEHOLDER_DEFAULT_TTL_DAYS,
            pending_owner_signoff: web_budget::DEFAULTS_PENDING_OWNER_SIGNOFF,
        },
        ceilings: GrantCeilings {
            max_count: web_budget::MAX_COUNT_CEILING,
            ttl_days: (web_budget::MAX_TTL_MS_CEILING / DAY_MS) as u32,
        },
        rate: RateLimits {
            min_gap_seconds: web_budget::SIWE_MIN_GAP_MS / 1000,
            window_max: web_budget::SIWE_WINDOW_MAX,
            window_hours: web_budget::SIWE_WINDOW_MS / (60 * 60 * 1000),
        },
        wallet_address: wallet.map(str::to_ascii_lowercase),
        now_ms: now,
    }
}

pub fn grant_inner(
    gate: &BudgetGate,
    wallet: Option<&str>,
    origin: &str,
    max_count: u32,
    ttl_days: u32,
    now: u64,
) -> Result<WebSigningBudget, String> {
    let wallet = wallet.ok_or_else(|| {
        "unlock or create your wallet first; a budget is tied to one wallet".to_string()
    })?;
    let ttl = ttl_days_to_ms(ttl_days)?;
    gate.grant(origin, DEFAULT_PRINCIPAL, max_count, ttl, wallet, now)
        .map_err(|e| e.to_string())
}

/// The arguments of `web_signing_request`. There is no origin attestation here on purpose: core
/// attests the origin itself (D2 #1); `claimed_origin` is display text for the card only.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SigningRequestArgs {
    pub message: String,
    pub tab_id: Option<String>,
    pub claimed_origin: String,
    /// Untrusted sources the current task has read. These can only add taint: omitted or empty is
    /// treated as unknown (tainted), see [`window_taint`].
    pub taint_sources: Option<Vec<String>>,
    /// The sidecar marked the call `hic: "required"`. Can only raise the bar, never lower it.
    #[serde(default)]
    pub hic_required: bool,
}

pub fn request_inner(
    ceremony: &SignatureCeremony,
    vault: &CustodyVault,
    gate: &BudgetGate,
    args: SigningRequestArgs,
    clock: &dyn Fn() -> u64,
) -> BudgetedOutcome {
    let attestation = attest_origin(args.tab_id.as_deref());
    request_attested(ceremony, vault, gate, args, attestation, clock)
}

/// The window command's path once core has (or has not) attested the origin. Split out so the
/// trust rules on the caller's arguments are testable with an attestation in hand.
pub fn request_attested(
    ceremony: &SignatureCeremony,
    vault: &CustodyVault,
    gate: &BudgetGate,
    args: SigningRequestArgs,
    attestation: Option<OriginAttestation>,
    clock: &dyn Fn() -> u64,
) -> BudgetedOutcome {
    let claimed: String = args
        .claimed_origin
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CLAIMED_ORIGIN_CHARS)
        .collect();
    let req = SiweSignRequest {
        message: args.message,
        attestation,
        taint: window_taint(args.taint_sources),
        hic_required: args.hic_required,
        principal: DEFAULT_PRINCIPAL.to_string(),
        claimed_origin: claimed,
    };
    ceremony.request_siwe_budgeted(vault, gate, req, clock)
}

fn active_wallet(vault: &CustodyVault) -> Option<String> {
    citrate_core_kit::wallet::address(vault)
        .ok()
        .map(|w| w.address.to_ascii_lowercase())
}

fn state<'a, T: Send + Sync + 'static>(
    app_h: &'a tauri::AppHandle,
) -> Result<tauri::State<'a, T>, String> {
    tauri::Manager::try_state::<T>(app_h)
        .ok_or_else(|| "internal: managed state unavailable".to_string())
}

/// **Command — web_budget_status.** Read-only.
#[tauri::command]
pub async fn web_budget_status(app_h: tauri::AppHandle) -> Result<WebBudgetStatus, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let cu = state::<crate::custody::CustodyState>(&app_h)?;
        let wallet = active_wallet(&cu.0);
        Ok(status_of(st.gate(), now_ms(), wallet.as_deref()))
    })
    .await
}

/// **Command — web_budget_grant.** The member's HIC-1 decision in Settings → Budgets. Writes a
/// `BudgetGranted` decision record; no wallet signature (O-4).
#[tauri::command]
pub async fn web_budget_grant(
    app_h: tauri::AppHandle,
    origin: String,
    max_count: u32,
    ttl_days: u32,
) -> Result<WebSigningBudget, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let cu = state::<crate::custody::CustodyState>(&app_h)?;
        let wallet = active_wallet(&cu.0);
        grant_inner(
            st.gate(),
            wallet.as_deref(),
            &origin,
            max_count,
            ttl_days,
            now_ms(),
        )
    })
    .await
}

/// **Command — web_budget_revoke.** Immediate: takes the budget lock and saves the tombstone.
#[tauri::command]
pub async fn web_budget_revoke(app_h: tauri::AppHandle, id: u64) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        st.gate().revoke(id, now_ms()).map_err(|e| e.to_string())
    })
    .await
}

/// **Command — web_budget_revoke_all.** Revokes every budget at once.
#[tauri::command]
pub async fn web_budget_revoke_all(app_h: tauri::AppHandle) -> Result<u32, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        st.gate()
            .revoke_all("the member revoked all budgets in Settings", now_ms())
            .map_err(|e| e.to_string())
    })
    .await
}

/// **Command — web_budget_reset.** Only meaningful after an integrity failure: keeps the old file
/// aside and starts with no budgets.
#[tauri::command]
pub async fn web_budget_reset(app_h: tauri::AppHandle) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        // The kit refuses a healthy store under the budget lock (BudgetError::StoreHealthy).
        st.gate()
            .reset_after_integrity_failure(now_ms())
            .map_err(|e| e.to_string())
    })
    .await
}

/// **Command — web_signing_request.** A Sign-In with Ethereum request. Inside a live budget for
/// a core-attested origin it is signed and recorded; otherwise it becomes a pending HIC-1 card
/// (the returned ceremony id), with the reason.
#[tauri::command]
pub async fn web_signing_request(
    app_h: tauri::AppHandle,
    args: SigningRequestArgs,
) -> Result<BudgetedOutcome, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let cu = state::<crate::custody::CustodyState>(&app_h)?;
        let ce = state::<crate::ceremony::CeremonyState>(&app_h)?;
        Ok(request_inner(&ce.0, &cu.0, st.gate(), args, &now_ms))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("web_budgets_tests.rs");
}
