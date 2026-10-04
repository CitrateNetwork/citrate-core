//! HUP-S2.3: Settings → Budgets and the budgeted Sign-In with Ethereum entry point.
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
//! | `web_signing_request` | decide one sign-in request waiting in the managed browser ([`crate::web_signin`]) |
//! | `web_signing_approve` / `web_signing_reject` | the member's decision on a sign-in card |
//!
//! **Where it applies.** Only in Hermes's managed browser (off unless the sidecar runs with
//! `CITRATE_HERMES_BROWSER=1`). The webview names a request id and nothing else: the message, the
//! asking page and the session taint come from the sidecar over core's own channel, and the origin
//! is attested by core's own read of the browser ([`crate::web_signin::attest`]). After every
//! decision the records are copied into the local decision records the nightly anchor covers.
//!
//! **Defaults change nothing.** There are no budgets until the member grants one. The values the
//! grant form proposes are placeholders pending owner sign-off (see `web_budget` constants).

use std::path::PathBuf;
use std::sync::OnceLock;

use citrate_core_kit::custody::{CustodyVault, OsKeyring};
use citrate_core_kit::web_budget::{
    self, BudgetGate, BudgetSnapshot, WebSigningBudget, DEFAULT_PRINCIPAL,
};
use serde::Serialize;

use crate::web_signin::{self, SignInOutcome, SignInState};

/// The budget file, in the app data dir.
pub const BUDGET_FILE_NAME: &str = "web-signing-budgets.json";
/// Where automatic sign-in applies. Shown in Settings → Budgets.
pub const ATTESTATION_SCOPE: &str = "Automatic sign-in works only in Hermes's managed browser, which is off unless Hermes's browser is turned on. Everywhere else, and for any site without an active budget, every sign-in asks you.";
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Managed state: the budget file path and the store, opened on first use (off the main thread,
/// because opening reads the OS keychain).
pub struct WebBudgetState {
    path: Option<PathBuf>,
    gate: OnceLock<BudgetGate>,
    /// Which bridge request each pending sign-in card answers.
    signin: SignInState,
}

impl WebBudgetState {
    /// `path` is `None` when the app data dir is unavailable; the store then reports itself failed.
    pub fn new(path: Option<PathBuf>) -> Self {
        WebBudgetState {
            path,
            gate: OnceLock::new(),
            signin: SignInState::default(),
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
            available: true,
            reason: ATTESTATION_SCOPE.to_string(),
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

/// A sign-in request id as the sidecar mints it (`signin-<n>-<m>`).
pub fn valid_request_id(id: &str) -> Result<(), String> {
    let ok = id.len() <= 64
        && id.strip_prefix("signin-").is_some_and(|rest| {
            !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit() || b == b'-')
        });
    if ok {
        Ok(())
    } else {
        Err("that is not a sign-in request id".to_string())
    }
}

/// Copy the budget records into the decision records the nightly anchor covers. Best effort:
/// Hermes may not be running; the next decision or the nightly pass tries again.
pub fn export_for_app(app_h: &tauri::AppHandle) {
    let Ok(st) = state::<WebBudgetState>(app_h) else {
        return;
    };
    let Ok(m) = crate::hermes::manager_for(app_h) else {
        return;
    };
    if let Err(e) = web_signin::export_records(m, st.gate()) {
        eprintln!("citrate-core: web-signing records not exported yet: {e}");
    }
}

/// What the "Signed for you" notice shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoSignedNotice {
    pub origin: String,
    pub budget_id: u64,
    pub record_id: u64,
    pub remaining: u32,
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
        let out = grant_inner(
            st.gate(),
            wallet.as_deref(),
            &origin,
            max_count,
            ttl_days,
            now_ms(),
        );
        if out.is_ok() {
            export_for_app(&app_h);
        }
        out
    })
    .await
}

/// **Command — web_budget_revoke.** Immediate: takes the budget lock and saves the tombstone.
#[tauri::command]
pub async fn web_budget_revoke(app_h: tauri::AppHandle, id: u64) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let out = st.gate().revoke(id, now_ms()).map_err(|e| e.to_string());
        export_for_app(&app_h);
        out
    })
    .await
}

/// **Command — web_budget_revoke_all.** Revokes every budget at once.
#[tauri::command]
pub async fn web_budget_revoke_all(app_h: tauri::AppHandle) -> Result<u32, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let out = st
            .gate()
            .revoke_all("the member revoked all budgets in Settings", now_ms())
            .map_err(|e| e.to_string());
        export_for_app(&app_h);
        out
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
        let out = st
            .gate()
            .reset_after_integrity_failure(now_ms())
            .map_err(|e| e.to_string());
        if out.is_ok() {
            export_for_app(&app_h);
        }
        out
    })
    .await
}

/// **Command: web_signing_request.** Decide one sign-in request waiting in the managed browser.
/// The webview names the request only; everything else is read by core itself (see
/// [`crate::web_signin`]). Inside a live budget for a core-attested origin it is signed,
/// recorded, delivered and announced (the "Signed for you" notice); otherwise it becomes a pending
/// HIC-1 card with the reason, or the page is told no.
#[tauri::command]
pub async fn web_signing_request(
    app_h: tauri::AppHandle,
    request_id: String,
) -> Result<SignInOutcome, String> {
    crate::blocking::off_main(move || {
        valid_request_id(&request_id)?;
        let st = state::<WebBudgetState>(&app_h)?;
        let cu = state::<crate::custody::CustodyState>(&app_h)?;
        let ce = state::<crate::ceremony::CeremonyState>(&app_h)?;
        let m = crate::hermes::manager_for(&app_h)?;
        let out = web_signin::handle(
            &web_signin::SignInCtx {
                link: m,
                devtools: &web_signin::LoopbackDevtools,
                ceremony: &ce.0,
                vault: &cu.0,
                gate: st.gate(),
                state: &st.signin,
                clock: &now_ms,
            },
            &request_id,
        )?;
        if let SignInOutcome::AutoSigned {
            origin,
            remaining,
            record_id,
            budget_id,
            ..
        } = &out
        {
            use tauri::Emitter;
            let _ = app_h.emit_to(
                "main",
                web_signin::AUTO_SIGNED_EVENT,
                AutoSignedNotice {
                    origin: origin.clone(),
                    budget_id: *budget_id,
                    record_id: *record_id,
                    remaining: *remaining,
                },
            );
            export_for_app(&app_h);
        }
        Ok(out)
    })
    .await
}

/// **Command: web_signing_approve.** The member approved a sign-in card at the Signature
/// Ceremony: sign it and deliver it to the page. Returns whether the page received it.
#[tauri::command]
pub async fn web_signing_approve(
    app_h: tauri::AppHandle,
    id: String,
    raw_ack: bool,
) -> Result<bool, String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let cu = state::<crate::custody::CustodyState>(&app_h)?;
        let ce = state::<crate::ceremony::CeremonyState>(&app_h)?;
        let m = crate::hermes::manager_for(&app_h)?;
        web_signin::approve(m, &ce.0, &cu.0, &st.signin, &id, raw_ack)
    })
    .await
}

/// **Command: web_signing_reject.** The member declined a sign-in card: nothing is signed.
#[tauri::command]
pub async fn web_signing_reject(app_h: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        let st = state::<WebBudgetState>(&app_h)?;
        let ce = state::<crate::ceremony::CeremonyState>(&app_h)?;
        let m = crate::hermes::manager_for(&app_h)?;
        web_signin::reject(m, &ce.0, &st.signin, &id)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("web_budgets_tests.rs");
}
