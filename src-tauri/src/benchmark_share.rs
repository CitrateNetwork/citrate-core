//! HUP-S7.5 (US-7.3 AC3): opt-in sharing of a day's Hermes aggregates with `BenchmarkRegistry`.
//!
//! The sidecar builds the unsigned `record(uint256,bytes32,bytes32,uint256)` calls from the day's
//! metering report (`POST /metering/benchmark`, crate `citrate-agent-metering`). Core treats that
//! payload as untrusted input: every call is rebuilt here from its parts and must match byte for
//! byte, its destination must be the pinned registry, it moves no value, its agent id is the
//! member's own AgentSBT and its capsule slot is the Hermes label. Each surviving call becomes one
//! PENDING [`crate::ceremony::SignatureCeremony`] card from the member's own wallet (HIC-1); nothing
//! signs here (Rule 3), and the cards are not one-click (the kit decoder does not list `record`,
//! so each needs the raw acknowledgement).
//!
//! Off by default: the member must turn sharing on (`chain_agent::ChainSettings`), and that is
//! refused while `BenchmarkRegistry` is not in the 40204 address book. Counts only, no content.
//!
//! Pending owner sign-off (conservative placeholders):
//! - **Submission shape.** One wallet card per metric (up to fifteen a day), signed by the member's
//!   account so the series is theirs (`BenchmarkRegistry` keys records by `msg.sender`). The
//!   alternatives are a batch call on the registry or folding the aggregates into the nightly
//!   anchor (which changes whose series it is).
//! - **Agent id.** The member's first AgentSBT token. No AgentSBT, no sharing.
//! - **Repeats.** Sharing the same day twice in one app session is refused; across restarts it
//!   is not tracked (the registry appends every record and stores no day).

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// The function every call must be.
pub const RECORD_SIGNATURE: &str = "record(uint256,bytes32,bytes32,uint256)";
/// The label hashed into the capsule slot for the Hermes agent loop.
pub const HERMES_CAPSULE_LABEL: &str = "citrate.hermes.agent-loop.v1";
/// Every metric name starts with this.
pub const METRIC_PREFIX: &str = "hermes.daily.";
/// The most calls one day may carry.
pub const MAX_CALLS: usize = 32;
/// Origin shown on every benchmark card.
pub const SHARE_ORIGIN: &str = "hermes:benchmark-share";
const CHAIN_ID: u64 = 40_204;

/// Decisions this build makes with conservative placeholders, pending owner sign-off.
pub const PENDING_OWNER_SIGN_OFF: &[&str] = &[
    "Benchmark sharing sends one wallet approval card per metric (up to fifteen a day) from your account. A batched call or folding the numbers into the nightly anchor are the alternatives. Pending owner sign-off.",
    "The agent id on shared numbers is your first AgentSBT. Pending owner sign-off.",
];

fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

/// `keccak256(RECORD_SIGNATURE)[..4]`.
pub fn record_selector() -> [u8; 4] {
    let h = keccak(RECORD_SIGNATURE.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// `keccak256(HERMES_CAPSULE_LABEL)`.
pub fn hermes_capsule_id() -> [u8; 32] {
    keccak(HERMES_CAPSULE_LABEL.as_bytes())
}

fn word_u128(v: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&v.to_be_bytes());
    w
}

/// `record(agent_id, capsule_id, metric_name, value)` calldata.
pub fn record_calldata(
    agent_id: u128,
    capsule: &[u8; 32],
    metric: &[u8; 32],
    value: u128,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 4 * 32);
    out.extend_from_slice(&record_selector());
    out.extend_from_slice(&word_u128(agent_id));
    out.extend_from_slice(capsule);
    out.extend_from_slice(metric);
    out.extend_from_slice(&word_u128(value));
    out
}

/// One call as the sidecar sends it (`citrate_agent_metering::BenchmarkCall`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PayloadCall {
    pub metric: String,
    pub metric_name: String,
    pub value: String,
    pub data: String,
}

/// The sidecar's payload (`citrate_agent_metering::BenchmarkPayload`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Payload {
    pub chain_id: u64,
    pub to: String,
    pub value: String,
    pub function: String,
    pub agent_id: String,
    pub capsule_id: String,
    pub day: String,
    pub calls: Vec<PayloadCall>,
    pub sent: bool,
}

/// A call core has rebuilt and accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareCall {
    pub metric: String,
    pub value: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}

fn same_addr(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn metric_ok(m: &str) -> bool {
    m.len() <= 64
        && m.strip_prefix(METRIC_PREFIX).is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// Check the sidecar's payload against what core expects and rebuild every call. Refuses the whole
/// payload on the first thing that does not hold.
pub fn validate(
    p: &Payload,
    registry: &str,
    agent_id: u128,
    day: &str,
) -> Result<Vec<ShareCall>, String> {
    if p.chain_id != CHAIN_ID {
        return Err(format!(
            "the payload targets chain {}, not {CHAIN_ID}",
            p.chain_id
        ));
    }
    if !same_addr(&p.to, registry) {
        return Err("the payload is not addressed to the pinned BenchmarkRegistry".into());
    }
    if p.value != "0" {
        return Err("a benchmark record moves no value".into());
    }
    if p.function != RECORD_SIGNATURE {
        return Err(format!("unexpected function {:?}", p.function));
    }
    if p.sent {
        return Err("the payload claims it was already sent".into());
    }
    if p.day != day {
        return Err("the payload is for another day".into());
    }
    if p.agent_id != agent_id.to_string() {
        return Err("the payload names another agent id".into());
    }
    let capsule = hermes_capsule_id();
    if !same_addr(&p.capsule_id, &format!("0x{}", hex::encode(capsule))) {
        return Err("the payload's capsule slot is not the Hermes label".into());
    }
    if p.calls.is_empty() {
        return Err("the payload has no calls".into());
    }
    if p.calls.len() > MAX_CALLS {
        return Err(format!(
            "the payload has {} calls, more than {MAX_CALLS}",
            p.calls.len()
        ));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(p.calls.len());
    for c in &p.calls {
        if !metric_ok(&c.metric) {
            return Err(format!("unexpected metric name {:?}", c.metric));
        }
        if !seen.insert(c.metric.clone()) {
            return Err(format!("metric {} appears twice", c.metric));
        }
        let name = keccak(c.metric.as_bytes());
        if !same_addr(&c.metric_name, &format!("0x{}", hex::encode(name))) {
            return Err(format!("the hash of {} does not match its name", c.metric));
        }
        let value: u128 = c
            .value
            .parse()
            .map_err(|_| format!("the value of {} is not a whole number", c.metric))?;
        let data = record_calldata(agent_id, &capsule, &name, value);
        let given = hex::decode(c.data.strip_prefix("0x").unwrap_or(&c.data))
            .map_err(|_| format!("the calldata of {} is not hex", c.metric))?;
        if given != data {
            return Err(format!(
                "the calldata of {} is not the record call core builds",
                c.metric
            ));
        }
        out.push(ShareCall {
            metric: c.metric.clone(),
            value: value.to_string(),
            data,
        });
    }
    Ok(out)
}

/// The member's agent id: their first AgentSBT token, if they hold one.
pub fn agent_id_of(st: &crate::agent_sbt::AgentSbtStatus) -> Result<u128, String> {
    let first = st
        .tokens
        .as_ref()
        .and_then(|t| t.first())
        .ok_or("Sharing needs your Hermes identity (an AgentSBT) first.")?;
    first
        .token_id
        .parse()
        .map_err(|_| "the AgentSBT token id is not a number".to_string())
}

/// The day guard for this app session (see the module docs).
fn shared_days() -> &'static Mutex<BTreeSet<String>> {
    static S: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Reserve `day` for sharing; `false` when it was already shared in this session.
pub fn reserve_day(day: &str) -> bool {
    shared_days()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(day.to_string())
}

/// Give a reservation back (nothing reached the ceremony).
pub fn release_day(day: &str) {
    shared_days()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(day);
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub day: String,
    pub registry: String,
    pub agent_id: String,
    pub calls: Vec<ShareCall>,
    /// One pending wallet card per call, in order.
    pub cards: Vec<crate::ceremony::CeremonyView>,
    pub pending_owner_sign_off: Vec<&'static str>,
}

/// **hermes_benchmark_share**: the member shares one closed day's aggregates: core fetches the
/// sidecar's calls, rebuilds and checks each, and raises one pending wallet card per call. Refused
/// unless sharing is on, `BenchmarkRegistry` is in the address book and the member holds an
/// AgentSBT. Nothing signs here.
#[tauri::command]
pub async fn hermes_benchmark_share(
    app: tauri::AppHandle,
    day: String,
) -> Result<ShareView, String> {
    crate::blocking::off_main(move || {
        if !crate::hermes::chain::valid_day(&day) {
            return Err("day must be YYYY-MM-DD".into());
        }
        let settings = crate::chain_agent::load_settings(&crate::chain_agent::settings_path(&app)?);
        let registry = crate::chain_agent::benchmark_registry()
            .ok_or("BenchmarkRegistry is not deployed on 40204 yet; nothing was shared.")?;
        if !settings.share_benchmarks {
            return Err("Benchmark sharing is off. Turn it on first.".into());
        }
        let m = crate::hermes::chain::manager_for(&app)?;
        if !m.is_running() {
            return Err("Hermes is not running, so the day's numbers are unknown.".into());
        }
        let st = crate::agent_sbt::status_now(&app)?;
        let agent_id = agent_id_of(&st)?;
        if !reserve_day(&day) {
            return Err(format!(
                "The numbers of {day} were already prepared for approval in this session."
            ));
        }
        let result = (|| {
            let raw = m
                .metering_benchmark(&day, agent_id, &registry)
                .map_err(|e| e.to_string())?;
            let payload: Payload =
                serde_json::from_value(raw).map_err(|e| format!("unexpected payload: {e}"))?;
            let calls = validate(&payload, &registry, agent_id, &day)?;
            let rpc = crate::rpc::RpcClient::citrate();
            let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app)
                .ok_or("internal: managed state unavailable")?;
            // Estimate every call before raising any card, so a failure raises none.
            let mut txs = Vec::with_capacity(calls.len());
            for c in &calls {
                let gas = rpc
                    .estimate_gas(serde_json::json!({
                        "from": st.member,
                        "to": registry,
                        "data": format!("0x{}", hex::encode(&c.data)),
                    }))
                    .map_err(|e| format!("could not estimate gas for {}: {e}", c.metric))?;
                txs.push(crate::agent_sbt::mint_tx_json(
                    &st.member,
                    &registry,
                    &c.data,
                    crate::agent_sbt::with_gas_margin(gas),
                ));
            }
            let cards = txs
                .into_iter()
                .map(|raw| {
                    ceremony.0.request(crate::ceremony::SignatureIntent {
                        origin: SHARE_ORIGIN.to_string(),
                        kind: crate::ceremony::IntentKind::Transaction,
                        chain_id: CHAIN_ID,
                        raw,
                    })
                })
                .collect();
            Ok(ShareView {
                day: day.clone(),
                registry: registry.clone(),
                agent_id: agent_id.to_string(),
                calls,
                cards,
                pending_owner_sign_off: PENDING_OWNER_SIGN_OFF.to_vec(),
            })
        })();
        if result.is_err() {
            release_day(&day);
        }
        result
    })
    .await
}

#[cfg(test)]
#[path = "benchmark_share_tests.rs"]
mod tests;
