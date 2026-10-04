//! HUP-S7.3 + S7.5 (core half): the nightly anchor schedule, the anchor ceremony commands, the
//! daily metering report surface and the BenchmarkRegistry sharing toggle. **All off by default.**
//!
//! Data sources (Rule 7):
//! - Metering report: the Hermes sidecar's `GET /metering/daily` (records derived from the loop
//!   event stream; tokens only when the model provider reported them).
//! - Anchor batches: the sidecar's `/anchor/*` routes over the local decision records.
//! - Registry addresses: the generated 40204 address book (`src-tauri/addresses/40204.json`).
//!   `AnchorRegistry` and `BenchmarkRegistry` are optional pins in it (both deployed on 40204 and
//!   verified to have code when the book was generated). A build whose book lacks one reports it
//!   and keeps that feature off. Both features stay off until the member turns them on.
//! - Receipts: the live 40204 RPC, through the anchor ceremony.
//!
//! Signing (Rule 3): only through `citrate_core_kit::ceremony::anchor::AnchorCeremony`, with the
//! separate no-funds anchor key sealed in the OS keyring. The sidecar never holds it. A day is
//! marked anchored in the sidecar's ledger only after a mined receipt with status 1.
//!
//! Pending owner sign-off (ADR-2026-09-30-rule3-budgetable-signatures, O-5): how the anchor key
//! pays gas, how it is bound on chain as the member's anchor delegate, and whether the nightly
//! approval may run unattended (HIC-2). Until then each anchor waits for an explicit approval,
//! and the scheduler only raises the approval cards.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::ceremony::anchor::{
    anchor_address, date_of_day, ensure_anchor_key, receipt_confirms, AnchorCeremony,
    AnchorCeremonyView, AnchorGuards, AnchorReceipt, AnchorRequest, AnchorTxConfig,
};
use crate::hermes::chain::PlannedAnchor;

/// The generated address book (one source; see `addresses.rs`).
const BOOK_JSON: &str = include_str!("../addresses/40204.json");

/// How often the nightly scheduler wakes once it is running.
pub const TICK_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Decisions this build makes with conservative placeholders, pending owner sign-off.
pub const PENDING_OWNER_SIGN_OFF: &[&str] = &[
    "How the anchor key pays gas: the EIP-2771 relayer or a capped gas float (ADR O-5). Pending owner sign-off.",
    "The anchor gas caps (50 gwei, 400,000 gas) are conservative placeholders until O-5 is decided. Pending owner sign-off.",
    "How the anchor key is bound on chain as the member's anchor delegate. Pending owner sign-off.",
    "Whether the nightly anchor may be approved unattended (HIC-2). Until then each day needs an explicit approval. Pending owner sign-off.",
    "How shared benchmark aggregates reach BenchmarkRegistry (per-metric calls from the member's account, or folded into the nightly batch). Pending owner sign-off.",
];

// ---------------------------------------------------------------------------------------------
// addresses

/// An optional pin from the address book: present, `0x` + 40 hex, not the zero address.
pub fn optional_pin(book_json: &str, name: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(book_json).ok()?;
    let a = v
        .get("addresses")?
        .get(name)?
        .as_str()?
        .to_ascii_lowercase();
    let hex = a.strip_prefix("0x")?;
    let well_formed = hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    let zero = hex.bytes().all(|b| b == b'0');
    (well_formed && !zero).then_some(a)
}

/// `AnchorRegistry` on 40204, when deployed.
pub fn anchor_registry() -> Option<String> {
    optional_pin(BOOK_JSON, "AnchorRegistry")
}

/// `BenchmarkRegistry` on 40204, when deployed.
pub fn benchmark_registry() -> Option<String> {
    optional_pin(BOOK_JSON, "BenchmarkRegistry")
}

// ---------------------------------------------------------------------------------------------
// settings

/// The member's choices. Both default to off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChainSettings {
    /// Anchor the day's decision records every night.
    pub anchor_nightly: bool,
    /// Share daily metering aggregates with BenchmarkRegistry.
    pub share_benchmarks: bool,
}

/// Read the settings file. Missing or unreadable means the defaults (off).
pub fn load_settings(path: &Path) -> ChainSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(path: &Path, s: &ChainSettings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not save settings: {}", e.kind()))?;
    }
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, body).map_err(|e| format!("could not save settings: {}", e.kind()))
}

/// Apply a member's change. Turning a feature on is refused while its registry is not deployed;
/// turning anything off is always allowed.
pub fn apply_settings(
    requested: ChainSettings,
    anchor_deployed: bool,
    benchmark_deployed: bool,
) -> Result<ChainSettings, String> {
    if requested.anchor_nightly && !anchor_deployed {
        return Err(
            "Nightly anchoring cannot be turned on: AnchorRegistry is not in this app's 40204 address book."
                .into(),
        );
    }
    if requested.share_benchmarks && !benchmark_deployed {
        return Err(
            "Benchmark sharing cannot be turned on: BenchmarkRegistry is not in this app's 40204 address book."
                .into(),
        );
    }
    Ok(requested)
}

// ---------------------------------------------------------------------------------------------
// the gate + status lines

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorGate {
    /// No `AnchorRegistry` in the address book.
    NotDeployed,
    /// Deployed, but the member has not turned nightly anchoring on.
    Off,
    /// Deployed and on: the scheduler raises an approval card per closed day.
    Ready,
}

pub fn anchor_gate(registry: Option<&str>, s: &ChainSettings) -> AnchorGate {
    match (registry, s.anchor_nightly) {
        (None, _) => AnchorGate::NotDeployed,
        (Some(_), false) => AnchorGate::Off,
        (Some(_), true) => AnchorGate::Ready,
    }
}

pub fn anchor_status_line(gate: AnchorGate) -> String {
    match gate {
        AnchorGate::NotDeployed => "Nightly anchoring is off: AnchorRegistry is not in this app's 40204 address book. Decision records stay on this device.".into(),
        AnchorGate::Off => "Nightly anchoring is off. Turn it on to anchor each day's decision records on 40204.".into(),
        AnchorGate::Ready => "Nightly anchoring is on. Each closed day waits for your approval before it is sent.".into(),
    }
}

pub fn benchmark_status_line(registry: Option<&str>, s: &ChainSettings) -> String {
    match (registry, s.share_benchmarks) {
        (None, _) => "Benchmark sharing is off: BenchmarkRegistry is not in this app's 40204 address book. Nothing leaves this device.".into(),
        (Some(_), false) => "Benchmark sharing is off. Nothing leaves this device.".into(),
        (Some(_), true) => "Benchmark sharing is on: daily aggregates (counts only, no content) are prepared for BenchmarkRegistry.".into(),
    }
}

// ---------------------------------------------------------------------------------------------
// the nightly tick (pure over a port, so it is testable without a sidecar)

/// What core needs from the sidecar's anchor routes.
pub trait AnchorPort {
    fn status(&self) -> Result<serde_json::Value, String>;
    fn plan(&self, day: u64, registry: &str) -> Result<PlannedAnchor, String>;
    fn confirm(&self, day: u64, commitment: &str, tx_hash: &str, block: u64) -> Result<(), String>;
}

impl AnchorPort for crate::hermes::HermesManager {
    fn status(&self) -> Result<serde_json::Value, String> {
        self.anchor_status().map_err(|e| e.to_string())
    }
    fn plan(&self, day: u64, registry: &str) -> Result<PlannedAnchor, String> {
        self.anchor_plan(day, registry).map_err(|e| e.to_string())
    }
    fn confirm(&self, day: u64, commitment: &str, tx_hash: &str, block: u64) -> Result<(), String> {
        self.anchor_confirm(day, commitment, tx_hash, block)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    let h = s.strip_prefix("0x")?;
    if h.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(h, &mut out).ok()?;
    Some(out)
}

/// Turn a sidecar plan into an anchor request. `Ok(None)` for any plan that is not `ready`
/// (nothing to anchor that day); `Err` for a `ready` plan that is malformed.
pub fn request_from_plan(p: &PlannedAnchor) -> Result<Option<AnchorRequest>, String> {
    if p.plan != "ready" {
        return Ok(None);
    }
    let call = p.call.as_ref().ok_or("a ready plan without a call")?;
    let commitment = p
        .commitment
        .as_deref()
        .and_then(hex_to_32)
        .ok_or("a ready plan without a valid commitment")?;
    if call.kind != "nightly_merkle" {
        return Err(format!("unexpected anchor kind {:?}", call.kind));
    }
    let data = hex::decode(call.data.strip_prefix("0x").unwrap_or(&call.data))
        .map_err(|_| "the plan's calldata is not hex".to_string())?;
    Ok(Some(AnchorRequest {
        day: p.day,
        // The date shown on the card is computed from the day here, never taken from the plan.
        date: date_of_day(p.day).ok_or("the plan's day is out of range")?,
        commitment,
        to: call
            .to
            .clone()
            .ok_or("a ready plan without a destination")?,
        chain_id: call.chain_id,
        value: call.value,
        data,
    }))
}

/// Days the sidecar still has to anchor: closed days not batched yet, and batched days whose
/// anchor was never confirmed.
pub fn days_to_anchor(status: &serde_json::Value) -> Vec<u64> {
    let mut days: Vec<u64> = ["pendingDays", "awaitingConfirmation"]
        .iter()
        .filter_map(|k| status.get(*k).and_then(|v| v.as_array()))
        .flatten()
        .filter_map(|d| d.get("day").and_then(|x| x.as_u64()))
        .collect();
    days.sort_unstable();
    days.dedup();
    days
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickReport {
    pub raised: Vec<AnchorCeremonyView>,
    /// `(day, why)` for each day that was not raised.
    pub skipped: Vec<(u64, String)>,
}

/// [`nightly_tick_with`] with nothing in flight. Test convenience: the production scheduler always
/// passes its in-flight set.
#[cfg(test)]
pub fn nightly_tick(
    port: &dyn AnchorPort,
    ceremony: &AnchorCeremony,
    gate: AnchorGate,
    registry: Option<&str>,
) -> Result<TickReport, String> {
    nightly_tick_with(port, ceremony, gate, registry, &BTreeSet::new())
}

/// One pass of the nightly schedule: only when the gate is `Ready`, plan each day the sidecar
/// still has to anchor and raise one approval card per ready day. Signs nothing. Days whose
/// previous anchor was sent and is still waiting for its receipt (`in_flight`) are skipped: a day
/// never has two anchor transactions on the way (`AnchorSettle.tla`, `AtMostOneInFlight`).
pub fn nightly_tick_with(
    port: &dyn AnchorPort,
    ceremony: &AnchorCeremony,
    gate: AnchorGate,
    registry: Option<&str>,
    in_flight: &BTreeSet<u64>,
) -> Result<TickReport, String> {
    let mut report = TickReport::default();
    let (AnchorGate::Ready, Some(registry)) = (gate, registry) else {
        return Ok(report);
    };
    let status = port.status()?;
    for day in days_to_anchor(&status) {
        if in_flight.contains(&day) {
            report
                .skipped
                .push((day, "waiting for the previous anchor's receipt".to_string()));
            continue;
        }
        let plan = match port.plan(day, registry) {
            Ok(p) => p,
            Err(e) => {
                report.skipped.push((day, e));
                continue;
            }
        };
        match request_from_plan(&plan) {
            Ok(Some(req)) => match ceremony.request(req, registry) {
                Ok(v) => report.raised.push(v),
                Err(e) => report.skipped.push((day, e.to_string())),
            },
            Ok(None) => report.skipped.push((day, format!("plan: {}", plan.plan))),
            Err(e) => report.skipped.push((day, e)),
        }
    }
    Ok(report)
}

/// Turning nightly anchoring off drops every pending anchor card unsigned (`AnchorSettle.tla`,
/// `NoPendingCardWhileOff`). Returns how many were dropped.
pub fn drop_pending_when_off(ceremony: &AnchorCeremony, s: &ChainSettings) -> usize {
    if s.anchor_nightly {
        return 0;
    }
    ceremony
        .pending()
        .iter()
        .filter(|v| ceremony.reject(&v.id).is_ok())
        .count()
}

/// After a broadcast: tell the sidecar the day is anchored **only** when the receipt confirms
/// (mined, status 1). Returns whether it did.
pub fn settle(port: &dyn AnchorPort, r: &AnchorReceipt) -> Result<bool, String> {
    if !receipt_confirms(r) {
        return Ok(false);
    }
    let block = r.block_number.ok_or("a confirming receipt has a block")?;
    port.confirm(
        r.day,
        &format!("0x{}", hex::encode(r.commitment)),
        &r.tx_hash,
        block,
    )?;
    Ok(true)
}

/// The file that keeps sent, not yet settled anchors across restarts (in the Hermes folder).
pub const IN_FLIGHT_FILE: &str = "anchor-in-flight.json";

/// Sent anchors whose day is not settled yet (receipt unknown, or mined but the sidecar has not
/// recorded it), by day. Kept on disk, owner-only, so a restart still knows which days are on the
/// way and the scheduler never raises a second anchor for one (`AnchorSettle.tla`,
/// `AtMostOneInFlight`). A file that cannot be read blocks new anchors rather than being treated
/// as empty, and is left in place.
pub struct InFlightAnchors {
    path: Option<PathBuf>,
    map: Mutex<BTreeMap<u64, AnchorReceipt>>,
    blocked: Option<String>,
}

impl InFlightAnchors {
    /// Load from `path` (`None`: no app data folder, so anchoring is blocked).
    pub fn load(path: Option<PathBuf>) -> InFlightAnchors {
        let (map, blocked) = match &path {
            None => (
                BTreeMap::new(),
                Some("the app data folder is unavailable, so sent anchors cannot be tracked".into()),
            ),
            Some(p) => match std::fs::read(p) {
                Ok(bytes) => match serde_json::from_slice::<Vec<AnchorReceipt>>(&bytes) {
                    Ok(v) => (v.into_iter().map(|r| (r.day, r)).collect(), None),
                    Err(_) => (
                        BTreeMap::new(),
                        Some(format!(
                            "the record of sent anchors ({}) could not be read; no new anchor is raised until it is checked",
                            p.display()
                        )),
                    ),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (BTreeMap::new(), None),
                Err(e) => (
                    BTreeMap::new(),
                    Some(format!("the record of sent anchors could not be read ({})", e.kind())),
                ),
            },
        };
        InFlightAnchors {
            path,
            map: Mutex::new(map),
            blocked,
        }
    }

    /// Why new anchors are blocked, if they are.
    pub fn blocked(&self) -> Option<&str> {
        self.blocked.as_deref()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, AnchorReceipt>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, m: &BTreeMap<u64, AnchorReceipt>) -> Result<(), String> {
        use std::io::Write as _;
        if let Some(why) = &self.blocked {
            return Err(why.clone());
        }
        let path = self.path.as_ref().ok_or("no app data folder")?;
        let body = serde_json::to_vec(&m.values().collect::<Vec<_>>())
            .map_err(|e| format!("encode: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        let res = (|| -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let _ = std::fs::remove_file(&tmp);
            let mut f = citrate_core_kit::fsutil::create_secret_file(&tmp)?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("save: {}", e.kind()));
        }
        Ok(())
    }

    /// Record a signed anchor before it is sent (or update a sent one). Fails, and keeps memory
    /// unchanged, when it cannot be saved.
    pub fn record_sent(&self, r: &AnchorReceipt) -> Result<(), String> {
        let mut m = self.lock();
        let mut next = m.clone();
        next.insert(r.day, r.clone());
        self.save(&next)?;
        *m = next;
        Ok(())
    }

    /// The day is settled (anchored or reverted). Kept in memory as removed even if the save
    /// fails; the next save or the next re-poll writes it. (Not named `remove`: the main-thread
    /// tripwire resolves calls by name, and this one writes a file.)
    pub fn settle_day(&self, day: u64) {
        let mut m = self.lock();
        if m.remove(&day).is_some() {
            let _ = self.save(&m);
        }
    }

    /// The node refused this transaction outright, so it is not on the way: forget its record,
    /// but only while the record is still that transaction (never another send for the day).
    pub fn forget_unsent(&self, r: &AnchorReceipt) {
        let mut m = self.lock();
        if m.get(&r.day).is_some_and(|held| held.tx_hash == r.tx_hash) {
            m.remove(&r.day);
            let _ = self.save(&m);
        }
    }

    /// Days on the way.
    pub fn days(&self) -> BTreeSet<u64> {
        self.lock().keys().copied().collect()
    }

    /// Every sent, unsettled anchor.
    pub fn all(&self) -> Vec<AnchorReceipt> {
        self.lock().values().cloned().collect()
    }
}

/// After a broadcast: settle when the receipt confirms, and keep every sent anchor that is not
/// settled yet (receipt unknown, or mined but the sidecar could not record it) in `held`, so the
/// re-poll finishes it and the scheduler never raises a second anchor for that day
/// (`AnchorSettle.tla`, `AtMostOneInFlight`). A reverted anchor is not kept: the day may be raised
/// again. Returns whether the day is now anchored, and the line shown to the member.
pub fn after_broadcast(
    port: Result<&dyn AnchorPort, String>,
    r: &AnchorReceipt,
    held: &InFlightAnchors,
) -> (bool, String) {
    let hold = || {
        // Best effort: the record written before the send already holds the day.
        let _ = held.record_sent(r);
    };
    match (r.block_number, receipt_confirms(r)) {
        (Some(block), true) => match port.and_then(|p| settle(p, r)) {
            Ok(true) => {
                held.settle_day(r.day);
                (true, format!("Anchored in block {block}."))
            }
            _ => {
                hold();
                (
                    false,
                    format!(
                        "Mined in block {block}, but Hermes could not record it yet; it will be retried. The day is not marked anchored yet."
                    ),
                )
            }
        },
        (None, _) => {
            hold();
            (
                false,
                "Sent. Waiting for the block; the day is not marked anchored yet.".to_string(),
            )
        }
        (Some(_), false) => {
            held.settle_day(r.day);
            (
                false,
                "The transaction did not succeed on chain; the day is not anchored.".to_string(),
            )
        }
    }
}

// ---------------------------------------------------------------------------------------------
// app state + commands

fn ceremony() -> &'static AnchorCeremony {
    static C: OnceLock<AnchorCeremony> = OnceLock::new();
    C.get_or_init(AnchorCeremony::new)
}

/// Sent anchors not settled yet, loaded once from the Hermes folder.
fn submitted<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> &'static InFlightAnchors {
    static S: OnceLock<InFlightAnchors> = OnceLock::new();
    S.get_or_init(|| {
        InFlightAnchors::load(
            settings_path(app)
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(IN_FLIGHT_FILE))),
        )
    })
}

fn keyring() -> citrate_core_kit::custody::OsKeyring {
    citrate_core_kit::custody::OsKeyring::with_service(crate::CUSTODY_KEYRING_SERVICE)
}

pub(crate) fn settings_path<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes")
        .join("chain-settings.json"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorView {
    pub gate: AnchorGate,
    pub status_line: String,
    pub registry: Option<String>,
    pub enabled: bool,
    /// The anchor key's address, once one exists (`None` before the member turns anchoring on).
    pub anchor_key: Option<String>,
    pub anchor_key_error: Option<String>,
    /// The sidecar's view of the decision-record days (absent when Hermes is not running).
    pub sidecar: Option<serde_json::Value>,
    pub sidecar_error: Option<String>,
    pub pending: Vec<AnchorCeremonyView>,
    /// Broadcast anchors still waiting for a mined receipt.
    pub submitted: Vec<AnchorReceipt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkView {
    pub registry: Option<String>,
    pub deployed: bool,
    pub sharing: bool,
    pub status_line: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStatusView {
    pub anchor: AnchorView,
    pub benchmark: BenchmarkView,
    pub pending_owner_sign_off: Vec<&'static str>,
}

fn status_view(app: &tauri::AppHandle) -> Result<ChainStatusView, String> {
    let settings = load_settings(&settings_path(app)?);
    let a_reg = anchor_registry();
    let b_reg = benchmark_registry();
    let gate = anchor_gate(a_reg.as_deref(), &settings);
    let (anchor_key, anchor_key_error) = match anchor_address(&keyring()) {
        Ok(a) => (a, None),
        Err(e) => (None, Some(e.to_string())),
    };
    let (sidecar, sidecar_error) = match crate::hermes::chain::manager_for(app) {
        Ok(m) if m.is_running() => match m.anchor_status() {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e.to_string())),
        },
        Ok(_) => (None, Some("Hermes is not running".to_string())),
        Err(e) => (None, Some(e)),
    };
    let submitted = submitted(app).all();
    Ok(ChainStatusView {
        anchor: AnchorView {
            gate,
            status_line: anchor_status_line(gate),
            enabled: gate == AnchorGate::Ready,
            registry: a_reg,
            anchor_key,
            anchor_key_error,
            sidecar,
            sidecar_error,
            pending: ceremony().pending(),
            submitted,
        },
        benchmark: BenchmarkView {
            deployed: b_reg.is_some(),
            sharing: b_reg.is_some() && settings.share_benchmarks,
            status_line: benchmark_status_line(b_reg.as_deref(), &settings),
            registry: b_reg,
        },
        pending_owner_sign_off: PENDING_OWNER_SIGN_OFF.to_vec(),
    })
}

/// **hermes_chain_status** — anchor + benchmark state, honestly: what is deployed, what is on,
/// what the sidecar's decision-record days look like, and what waits for approval.
#[tauri::command]
pub async fn hermes_chain_status(app: tauri::AppHandle) -> Result<ChainStatusView, String> {
    crate::blocking::off_main(move || status_view(&app)).await
}

/// **hermes_chain_settings_set** — the member turns anchoring or benchmark sharing on or off.
/// Turning on a feature whose registry is not deployed is refused. Turning anchoring on creates
/// the anchor key (OS keyring) if there is none.
#[tauri::command]
pub async fn hermes_chain_settings_set(
    app: tauri::AppHandle,
    anchor_nightly: bool,
    share_benchmarks: bool,
) -> Result<ChainStatusView, String> {
    crate::blocking::off_main(move || {
        let s = apply_settings(
            ChainSettings {
                anchor_nightly,
                share_benchmarks,
            },
            anchor_registry().is_some(),
            benchmark_registry().is_some(),
        )?;
        if s.anchor_nightly {
            ensure_anchor_key(&keyring()).map_err(|e| e.to_string())?;
        }
        save_settings(&settings_path(&app)?, &s)?;
        drop_pending_when_off(ceremony(), &s);
        if s.anchor_nightly {
            start_nightly_if_ready(app.clone());
        }
        status_view(&app)
    })
    .await
}

/// **hermes_metering_daily** — the sidecar's daily metering report (`day` = `YYYY-MM-DD`, UTC;
/// default today). Fails honestly when Hermes is not running.
#[tauri::command]
pub async fn hermes_metering_daily(
    app: tauri::AppHandle,
    day: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::blocking::off_main(move || {
        let m = crate::hermes::chain::manager_for(&app)?;
        if !m.is_running() {
            return Err("Hermes is not running, so today's numbers are unknown.".to_string());
        }
        m.metering_daily(day.as_deref()).map_err(|e| e.to_string())
    })
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorApproveView {
    pub receipt: AnchorReceipt,
    /// True only when the receipt was mined with status 1 and the sidecar recorded it.
    pub anchored: bool,
    pub status_line: String,
}

/// **hermes_anchor_approve** — the member approves one pending anchor card: sign with the anchor
/// key, broadcast to 40204, and mark the day anchored only on a confirming receipt.
#[tauri::command]
pub async fn hermes_anchor_approve(
    app: tauri::AppHandle,
    id: String,
) -> Result<AnchorApproveView, String> {
    crate::blocking::off_main(move || {
        let registry = anchor_registry()
            .ok_or("AnchorRegistry is not in this app's 40204 address book; nothing was signed.")?;
        let held = submitted(&app);
        if let Some(why) = held.blocked() {
            return Err(format!("{why}; nothing was signed."));
        }
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app)
            .ok_or("internal: custody state unavailable")?;
        let rpc = crate::rpc::RpcClient::citrate();
        let record = |r: &AnchorReceipt| held.record_sent(r);
        let not_sent = |r: &AnchorReceipt| held.forget_unsent(r);
        let receipt = ceremony()
            .approve_and_broadcast(
                &keyring(),
                &rpc,
                &id,
                &registry,
                // Gas caps are placeholders pending owner sign-off (O-5).
                AnchorTxConfig::with_placeholder_caps(30, std::time::Duration::from_secs(2)),
                AnchorGuards {
                    vault: &custody.0,
                    before_send: &record,
                    not_sent: &not_sent,
                },
            )
            .map_err(|e| e.to_string())?;
        // Signed and sent from here on: never return early and lose the transaction.
        let port = crate::hermes::chain::manager_for(&app).map(|m| m as &dyn AnchorPort);
        let (anchored, status_line) = after_broadcast(port, &receipt, held);
        Ok(AnchorApproveView {
            receipt,
            anchored,
            status_line,
        })
    })
    .await
}

/// **hermes_anchor_reject** — drop a pending anchor card without signing.
#[tauri::command]
pub async fn hermes_anchor_reject(id: String) -> Result<(), String> {
    crate::blocking::off_main(move || ceremony().reject(&id).map_err(|e| e.to_string())).await
}

/// What a re-poll found for one sent, unsettled anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repoll {
    /// Mined: this receipt decides the day.
    Mined(AnchorReceipt),
    /// No receipt, and the sender's mined nonce has passed this transaction's nonce: another
    /// transaction took the nonce, so this one can never be mined and the day may be raised again.
    Dropped,
    /// Still unknown: keep holding the day.
    Wait,
}

/// Decide a re-poll from the sender's mined nonce (read FIRST) and the receipt (read after it).
/// Reading the nonce first means a transaction mined between the two reads shows up as a receipt,
/// never as `Dropped`. Records without a nonce or sender (older ones) only ever wait.
pub fn repoll_decision(
    r: &AnchorReceipt,
    latest_nonce: Option<u64>,
    receipt: Option<crate::rpc::Receipt>,
) -> Repoll {
    if let Some(rc) = receipt {
        return Repoll::Mined(AnchorReceipt {
            block_number: Some(rc.block_number),
            status: rc.status,
            ..r.clone()
        });
    }
    match (r.nonce, latest_nonce) {
        (Some(n), Some(mined)) if mined > n => Repoll::Dropped,
        _ => Repoll::Wait,
    }
}

/// Recheck broadcast anchors whose receipt was not mined in time; settle those that are now, and
/// release a day whose transaction can no longer be mined.
fn repoll_submitted(port: &dyn AnchorPort, held: &InFlightAnchors) {
    let pending: Vec<AnchorReceipt> = held.all();
    let rpc = crate::rpc::RpcClient::citrate();
    for r in pending {
        let latest = r.from.as_deref().and_then(|a| rpc.latest_nonce(a).ok());
        let Ok(receipt) = rpc.transaction_receipt(&r.tx_hash) else {
            continue;
        };
        match repoll_decision(&r, latest, receipt) {
            Repoll::Mined(done) => {
                if settle(port, &done).unwrap_or(false) || done.status == Some(0) {
                    held.settle_day(r.day);
                }
            }
            Repoll::Dropped => {
                eprintln!(
                    "citrate-core: the anchor for day {} was replaced or dropped; it can be raised again",
                    r.day
                );
                held.settle_day(r.day);
            }
            Repoll::Wait => {}
        }
    }
}

/// HUP-S4.2 (`anchor_propose` over the citrate-node MCP server): `Ok` when an anchor pass could
/// raise approval cards now, otherwise the member-facing reason (the same status line the app shows).
pub fn anchor_ready(app: &tauri::AppHandle) -> Result<(), String> {
    let registry = anchor_registry();
    let gate = anchor_gate(registry.as_deref(), &load_settings(&settings_path(app)?));
    match gate {
        AnchorGate::Ready => Ok(()),
        g => Err(anchor_status_line(g)),
    }
}

/// HUP-S4.2: run one anchor pass now (the member approved an `anchor_propose` request). Exactly
/// the nightly pass: only when the gate is `Ready`, days with an anchor already in flight are
/// skipped, and each ready day becomes an approval card. Signs nothing.
pub fn anchor_now(app: &tauri::AppHandle) -> Result<TickReport, String> {
    anchor_ready(app)?;
    let registry = anchor_registry();
    let gate = anchor_gate(registry.as_deref(), &load_settings(&settings_path(app)?));
    let m = crate::hermes::chain::manager_for(app)?;
    if !m.is_running() {
        return Err(
            "Hermes is not running, so its decision-record days cannot be read.".to_string(),
        );
    }
    // The same in-flight set the nightly pass uses (persisted across restarts).
    let held = submitted(app);
    if let Some(why) = held.blocked() {
        return Err(why.to_string());
    }
    nightly_tick_with(m, ceremony(), gate, registry.as_deref(), &held.days())
}

/// Start the nightly scheduler thread if, and only if, the gate is `Ready` (deployed registry and
/// the member turned anchoring on). Without an `AnchorRegistry` pin in the book this returns
/// false and starts nothing. Idempotent.
pub fn start_nightly_if_ready(app: tauri::AppHandle) -> bool {
    static STARTED: OnceLock<()> = OnceLock::new();
    let registry = anchor_registry();
    if registry.is_none() {
        return false;
    }
    let Ok(path) = settings_path(&app) else {
        return false;
    };
    if anchor_gate(registry.as_deref(), &load_settings(&path)) != AnchorGate::Ready {
        return false;
    }
    if STARTED.set(()).is_err() {
        return true;
    }
    std::thread::spawn(move || loop {
        let registry = anchor_registry();
        let gate = settings_path(&app)
            .map(|p| anchor_gate(registry.as_deref(), &load_settings(&p)))
            .unwrap_or(AnchorGate::Off);
        if let Ok(m) = crate::hermes::chain::manager_for(&app) {
            if m.is_running() {
                // HUP-S2.3 (US-2.3 AC3): web-signing records go into the batch before planning.
                crate::web_budgets::export_for_app(&app);
                // HUP-S2.6: core's other HIC records (grants, escalation spend, approval cards).
                crate::hic_records::export_for_app(&app);
                let held = submitted(&app);
                if let Some(why) = held.blocked() {
                    eprintln!("citrate-core: nightly anchor pass skipped: {why}");
                } else if let Err(e) =
                    nightly_tick_with(m, ceremony(), gate, registry.as_deref(), &held.days())
                {
                    eprintln!("citrate-core: nightly anchor pass failed: {e}");
                }
                repoll_submitted(m, held);
            }
        }
        std::thread::sleep(TICK_EVERY);
    });
    true
}

#[cfg(test)]
#[path = "chain_agent_tests.rs"]
mod tests;
