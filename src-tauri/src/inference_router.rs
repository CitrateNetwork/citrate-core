//! HUP-S1.5 — the registry escalation route against the on-chain **InferenceRouter**
//! (citrate-chain `contracts/src/InferenceRouter.sol`), US-1.5 AC1 + AC3.
//!
//! What the deployed contract actually offers, and therefore what this module does:
//!
//! - `requestInference(bytes32 modelHash, bytes inputData, uint256 maxPrice)` is **payable in
//!   native SALT** (`msg.value >= maxPrice`). It picks a registered compute provider for the model,
//!   escrows the payment, and credits the unused part back (`refundOwed`, claimed with
//!   `claimRefund()`). The input is kept in contract storage, so it is **public**. A native-value
//!   call is a plain transaction, never a budgetable x402 authorization (ADR-2026-09-30 D3), so
//!   every registry escalation is a pending SignatureCeremony the member approves (HIC-1).
//! - The provider answers later with `completeInference`; `getRequest(uint256)` then returns the
//!   status, the output bytes and the price paid. A request a provider never completes can be
//!   expired by anyone after `REQUEST_TIMEOUT` (1 h), which credits the full price back.
//! - Model CIDs come from the ModelRegistry ([`crate::model_registry`]); providers per model come
//!   from `getProviders(bytes32)` plus the public `providers(address)` getter.
//!
//! The route is **off on chain 40204** while the address book has no `inference_router` pin (the
//! post-reroll redeploy, federation F-4): every command here refuses with an honest message and
//! nothing is sent. The calldata, the decoders and the read client are proven against a real
//! InferenceRouter on a local anvil deploy (`scripts/anvil-registry-dryrun.sh`).
//!
//! x402 (ADR D3, B-2) is not used by this contract: it has no token entry point. The EIP-712
//! hasher and the pinned `TransferWithAuthorization` template exist in the kit
//! (`citrate_core_kit::eip712`, `web_budget::build_x402_authorization`), but B-2 stays inert while
//! the asset allowlist is empty (owner decision O-1) and the router takes native SALT only.

use serde::Serialize;
use serde_json::json;

use crate::model_registry::{be_usize, decode_bytes32_array, read_string_at, selector};
use crate::rpc::{RpcClient, RpcTransport};

pub const REQUEST_INFERENCE_SIG: &str = "requestInference(bytes32,bytes,uint256)";
pub const GET_REQUEST_SIG: &str = "getRequest(uint256)";
pub const GET_PROVIDERS_SIG: &str = "getProviders(bytes32)";
/// The public mapping getter (the struct's dynamic array `supportedModels` is omitted by solc).
pub const PROVIDERS_SIG: &str = "providers(address)";
pub const GET_USER_REQUESTS_SIG: &str = "getUserRequests(address)";
pub const REFUND_OWED_SIG: &str = "refundOwed(address)";
pub const CLAIM_REFUND_SIG: &str = "claimRefund()";

/// The largest prompt sent to the router. The input is stored on chain (gas grows with it, and it
/// is public), so this is far below the member-endpoint limit.
pub const MAX_REGISTRY_INPUT_BYTES: usize = 4096;
/// The highest price ceiling a member may offer per registry request: 10 SALT in wei.
/// Conservative placeholder, pending owner sign-off.
pub const MAX_REGISTRY_PRICE_WEI: u128 = 10_000_000_000_000_000_000;
/// Gas headroom over `eth_estimateGas` (the provider choice can differ between estimate and
/// inclusion): +25 %.
pub const GAS_HEADROOM_PCT: u64 = 25;
/// Providers read for one model (a longer route is cut, and the view says so).
pub const MAX_PROVIDERS: usize = 32;
/// Request ids read for one member.
pub const MAX_USER_REQUESTS: usize = 256;
/// Output bytes returned to the webview (longer outputs are cut and flagged).
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Why the registry route is off, in the member's words.
pub const ROUTE_OFF: &str =
    "Registry escalation is not deployed on chain 40204 yet. Escalations use your own endpoints.";

/// `RequestStatus` in the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RouterStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

impl RouterStatus {
    fn from_word(w: &[u8]) -> Result<Self, String> {
        match be_usize(w)? {
            0 => Ok(RouterStatus::Pending),
            1 => Ok(RouterStatus::Processing),
            2 => Ok(RouterStatus::Completed),
            3 => Ok(RouterStatus::Failed),
            4 => Ok(RouterStatus::Cancelled),
            n => Err(format!("getRequest: unknown status {n}")),
        }
    }
}

/// One router request as `getRequest` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterRequest {
    pub requester: String,
    pub model_hash: String,
    pub status: RouterStatus,
    /// The provider's output, as bytes.
    #[serde(skip)]
    pub output: Vec<u8>,
    /// Price paid in wei (decimal string; uint256 range).
    pub price_paid_wei: String,
}

/// One compute provider on a model's route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterProvider {
    pub address: String,
    pub endpoint: String,
    pub stake_wei: String,
    pub min_price_wei: String,
    pub max_concurrent: u64,
    pub current_load: u64,
    pub total_inferences: u64,
    pub success_bps: u64,
    pub active: bool,
}

impl RouterProvider {
    /// The contract's own eligibility test (`_selectProvider`): active, not above the ceiling, and
    /// with a free slot.
    pub fn eligible_at(&self, max_price_wei: u128) -> bool {
        self.active
            && self.current_load < self.max_concurrent
            && self
                .min_price_wei
                .parse::<u128>()
                .is_ok_and(|p| p <= max_price_wei)
    }
}

fn word_u128(w: &[u8]) -> Result<u128, String> {
    if w.len() != 32 || w[..16].iter().any(|b| *b != 0) {
        return Err("abi word: value too large".into());
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&w[16..]);
    Ok(u128::from_be_bytes(b))
}

fn word_dec(w: &[u8]) -> Result<String, String> {
    let mut a = [0u8; 32];
    if w.len() != 32 {
        return Err("abi word: not 32 bytes".into());
    }
    a.copy_from_slice(w);
    Ok(citrate_core_kit::eip712::u256_to_dec(&a))
}

fn word_u64(w: &[u8]) -> Result<u64, String> {
    be_usize(w).map(|v| v as u64)
}

fn address_of_word(w: &[u8]) -> Result<String, String> {
    if w.len() != 32 || w[..12].iter().any(|b| *b != 0) {
        return Err("abi address: high bytes set".into());
    }
    Ok(format!("0x{}", hex::encode(&w[12..32])))
}

fn bool_of_word(w: &[u8]) -> Result<bool, String> {
    match be_usize(w)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err("abi bool: not 0 or 1".into()),
    }
}

/// Parse a `0x` + 64 hex model hash.
pub fn parse_model_hash(s: &str) -> Result<[u8; 32], String> {
    let h = s
        .strip_prefix("0x")
        .filter(|h| h.len() == 64)
        .ok_or("the model id must be 0x followed by 64 hex digits")?;
    let v = hex::decode(h).map_err(|_| "the model id must be hex".to_string())?;
    let mut out = [0u8; 32];
    out.copy_from_slice(&v);
    Ok(out)
}

/// `requestInference(modelHash, inputData, maxPrice)` calldata (canonical ABI encoding).
pub fn encode_request_inference(
    model_hash: &[u8; 32],
    input: &[u8],
    max_price_wei: u128,
) -> Vec<u8> {
    let padded = input.len().div_ceil(32) * 32;
    let mut d = Vec::with_capacity(4 + 32 * 4 + padded);
    d.extend_from_slice(&selector(REQUEST_INFERENCE_SIG));
    d.extend_from_slice(model_hash);
    d.extend_from_slice(&citrate_core_kit::eip712::u64_word(0x60));
    let mut m = [0u8; 32];
    m[16..].copy_from_slice(&max_price_wei.to_be_bytes());
    d.extend_from_slice(&m);
    d.extend_from_slice(&citrate_core_kit::eip712::u64_word(input.len() as u64));
    d.extend_from_slice(input);
    d.resize(4 + 32 * 4 + padded, 0);
    d
}

fn call_word(sig: &str, word: &[u8; 32]) -> Vec<u8> {
    let mut d = selector(sig).to_vec();
    d.extend_from_slice(word);
    d
}

fn address_arg(addr: &str) -> Result<[u8; 32], String> {
    citrate_core_kit::eip712::parse_address(addr)
        .map(|a| citrate_core_kit::eip712::address_word(&a))
        .map_err(|e| e.to_string())
}

/// `getRequest(uint256)` return: `(address, bytes32, uint8, bytes, uint256)`.
pub fn decode_get_request(ret: &[u8]) -> Result<RouterRequest, String> {
    if ret.len() < 32 * 5 {
        return Err("getRequest: short return".into());
    }
    let requester = address_of_word(&ret[0..32])?;
    let model_hash = format!("0x{}", hex::encode(&ret[32..64]));
    let status = RouterStatus::from_word(&ret[64..96])?;
    let off = be_usize(&ret[96..128])?;
    let price_paid_wei = word_dec(&ret[128..160])?;
    let start = off.checked_add(32).ok_or("getRequest: offset overflow")?;
    if start > ret.len() {
        return Err("getRequest: offset past end".into());
    }
    let len = be_usize(&ret[off..start])?;
    let end = start
        .checked_add(len)
        .ok_or("getRequest: length overflow")?;
    if end > ret.len() {
        return Err("getRequest: output length exceeds data".into());
    }
    Ok(RouterRequest {
        requester,
        model_hash,
        status,
        output: ret[start..end].to_vec(),
        price_paid_wei,
    })
}

/// `providers(address)` return:
/// `(address provider, string endpoint, uint256 stake, uint256 minPrice, uint256 maxConcurrent,
///   uint256 currentLoad, uint256 totalInferences, uint256 successRate, bool isActive)`.
pub fn decode_provider(ret: &[u8]) -> Result<RouterProvider, String> {
    if ret.len() < 32 * 9 {
        return Err("providers: short return".into());
    }
    let w = |i: usize| &ret[i * 32..(i + 1) * 32];
    let endpoint_off = be_usize(w(1))?;
    Ok(RouterProvider {
        address: address_of_word(w(0))?,
        endpoint: read_string_at(ret, endpoint_off)?,
        stake_wei: word_dec(w(2))?,
        min_price_wei: word_dec(w(3))?,
        max_concurrent: word_u64(w(4))?,
        current_load: word_u64(w(5))?,
        total_inferences: word_u64(w(6))?,
        success_bps: word_u64(w(7))?,
        active: bool_of_word(w(8))?,
    })
}

/// An `address[]` return.
pub fn decode_address_array(ret: &[u8]) -> Result<Vec<String>, String> {
    decode_bytes32_array(ret)?
        .iter()
        .map(|w| address_of_word(w))
        .collect()
}

/// A `uint256[]` return of ids (each must fit `u64`).
pub fn decode_id_array(ret: &[u8]) -> Result<Vec<u64>, String> {
    decode_bytes32_array(ret)?
        .iter()
        .map(|w| word_u64(w))
        .collect()
}

/// Read-only access to one InferenceRouter.
pub struct RouterReader<'a, T: RpcTransport> {
    rpc: &'a RpcClient<T>,
    router: String,
}

impl<'a, T: RpcTransport> RouterReader<'a, T> {
    pub fn new(rpc: &'a RpcClient<T>, router: &str) -> Result<Self, String> {
        citrate_core_kit::eip712::parse_address(router).map_err(|e| e.to_string())?;
        Ok(RouterReader {
            rpc,
            router: router.to_ascii_lowercase(),
        })
    }

    fn call(&self, data: Vec<u8>) -> Result<Vec<u8>, String> {
        self.rpc
            .eth_call(json!({
                "to": self.router,
                "data": format!("0x{}", hex::encode(data)),
            }))
            .map_err(|e| e.to_string())
    }

    /// The providers on `model_hash`'s route (at most [`MAX_PROVIDERS`]), each read from chain.
    pub fn providers(&self, model_hash: &[u8; 32]) -> Result<Vec<RouterProvider>, String> {
        let addrs = decode_address_array(&self.call(call_word(GET_PROVIDERS_SIG, model_hash))?)?;
        addrs
            .iter()
            .take(MAX_PROVIDERS)
            .map(|a| decode_provider(&self.call(call_word(PROVIDERS_SIG, &address_arg(a)?))?))
            .collect()
    }

    pub fn request(&self, request_id: u64) -> Result<RouterRequest, String> {
        let id = citrate_core_kit::eip712::u64_word(request_id);
        decode_get_request(&self.call(call_word(GET_REQUEST_SIG, &id))?)
    }

    /// The member's request ids, oldest first (at most the newest [`MAX_USER_REQUESTS`]).
    pub fn user_requests(&self, member: &str) -> Result<Vec<u64>, String> {
        let ids =
            decode_id_array(&self.call(call_word(GET_USER_REQUESTS_SIG, &address_arg(member)?))?)?;
        let skip = ids.len().saturating_sub(MAX_USER_REQUESTS);
        Ok(ids.into_iter().skip(skip).collect())
    }

    /// Refund credited to `member` (wei, decimal string), claimable with `claimRefund()`.
    pub fn refund_owed(&self, member: &str) -> Result<String, String> {
        let ret = self.call(call_word(REFUND_OWED_SIG, &address_arg(member)?))?;
        if ret.len() < 32 {
            return Err("refundOwed: short return".into());
        }
        word_dec(&ret[0..32])
    }
}

/// What a registry escalation will cost before it is sent: the worst case is the whole ceiling
/// (paid up front; the unused part is credited back to claim).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryQuote {
    pub router: String,
    pub model_hash: String,
    pub max_price_wei: String,
    /// The ceiling in SALT (18 decimals), for the card.
    pub max_price_salt: String,
    pub input_bytes: usize,
    pub eligible_providers: usize,
    pub cheapest_min_price_wei: Option<String>,
    /// Always true: the router stores the input in contract storage.
    pub input_is_public: bool,
}

/// Check a registry request before anything is built or sent.
pub fn validate_registry_request(input: &str, max_price_wei: u128) -> Result<(), String> {
    if input.trim().is_empty() {
        return Err("the request is empty".into());
    }
    if input.len() > MAX_REGISTRY_INPUT_BYTES {
        return Err(format!(
            "registry requests are stored on chain, so they are limited to {MAX_REGISTRY_INPUT_BYTES} bytes"
        ));
    }
    if max_price_wei == 0 {
        return Err("set a price ceiling above zero".into());
    }
    if max_price_wei > MAX_REGISTRY_PRICE_WEI {
        return Err("the price ceiling is above the 10 SALT per request limit".into());
    }
    Ok(())
}

/// Quote a registry escalation from the model's live route.
pub fn quote_registry(
    router: &str,
    model_hash: &[u8; 32],
    providers: &[RouterProvider],
    input: &str,
    max_price_wei: u128,
) -> Result<RegistryQuote, String> {
    validate_registry_request(input, max_price_wei)?;
    let eligible: Vec<&RouterProvider> = providers
        .iter()
        .filter(|p| p.eligible_at(max_price_wei))
        .collect();
    if eligible.is_empty() {
        return Err("no registered provider can serve this model at that price right now".into());
    }
    let cheapest = eligible
        .iter()
        .filter_map(|p| p.min_price_wei.parse::<u128>().ok())
        .min()
        .map(|p| p.to_string());
    let mut max_word = [0u8; 32];
    max_word[16..].copy_from_slice(&max_price_wei.to_be_bytes());
    Ok(RegistryQuote {
        router: router.to_ascii_lowercase(),
        model_hash: format!("0x{}", hex::encode(model_hash)),
        max_price_wei: max_price_wei.to_string(),
        max_price_salt: citrate_core_kit::eip712::format_units(&max_word, 18),
        input_bytes: input.len(),
        eligible_providers: eligible.len(),
        cheapest_min_price_wei: cheapest,
        input_is_public: true,
    })
}

/// The pending-ceremony tx JSON for one registry escalation: from the member, to the router,
/// value = the ceiling, data = `requestInference` calldata, gas from the live estimate + headroom.
pub fn request_tx_json(
    from: &str,
    router: &str,
    calldata: &[u8],
    max_price_wei: u128,
    gas: u64,
) -> String {
    json!({
        "from": from,
        "to": router,
        "value": format!("0x{max_price_wei:x}"),
        "data": format!("0x{}", hex::encode(calldata)),
        "gas": format!("0x{gas:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string()
}

/// `eth_estimateGas` plus [`GAS_HEADROOM_PCT`]. A failed estimate (for example the call would
/// revert because no provider can serve it) is an error, never a guessed gas value.
pub fn estimate_request_gas<T: RpcTransport>(
    rpc: &RpcClient<T>,
    from: &str,
    router: &str,
    calldata: &[u8],
    max_price_wei: u128,
) -> Result<u64, String> {
    let est = rpc
        .estimate_gas(json!({
            "from": from,
            "to": router,
            "value": format!("0x{max_price_wei:x}"),
            "data": format!("0x{}", hex::encode(calldata)),
        }))
        .map_err(|e| format!("the router would not accept this request: {e}"))?;
    est.checked_add(est / 100 * GAS_HEADROOM_PCT)
        .ok_or_else(|| "gas estimate overflow".to_string())
}

/// The output as text for the webview, cut at [`MAX_OUTPUT_BYTES`]; the provider's answer is
/// untrusted data and is fenced as such by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryResultView {
    pub request_id: u64,
    pub status: RouterStatus,
    pub model_hash: String,
    pub price_paid_wei: String,
    pub output: Option<String>,
    pub output_truncated: bool,
    /// Output bytes that were not UTF-8 are shown as hex.
    pub output_is_hex: bool,
}

pub fn result_view(request_id: u64, r: &RouterRequest) -> RegistryResultView {
    let cut = &r.output[..r.output.len().min(MAX_OUTPUT_BYTES)];
    let (output, is_hex) = if r.output.is_empty() {
        (None, false)
    } else {
        match std::str::from_utf8(cut) {
            Ok(s) => (Some(s.to_string()), false),
            Err(_) => (Some(format!("0x{}", hex::encode(cut))), true),
        }
    };
    RegistryResultView {
        request_id,
        status: r.status,
        model_hash: r.model_hash.clone(),
        price_paid_wei: r.price_paid_wei.clone(),
        output,
        output_truncated: r.output.len() > MAX_OUTPUT_BYTES,
        output_is_hex: is_hex,
    }
}

fn pinned_router() -> Result<&'static str, String> {
    crate::addresses::inference_router().ok_or_else(|| ROUTE_OFF.to_string())
}

/// **escalation_registry_quote** — read the model's live route and quote the request. Off (an
/// honest error, no network call) while no router is pinned for 40204.
#[tauri::command]
pub async fn escalation_registry_quote(
    model_hash: String,
    input: String,
    max_price_wei: String,
) -> Result<RegistryQuote, String> {
    let router = pinned_router()?;
    let hash = parse_model_hash(&model_hash)?;
    let max: u128 = max_price_wei
        .parse()
        .map_err(|_| "the price ceiling must be a whole number of wei".to_string())?;
    validate_registry_request(&input, max)?;
    crate::blocking::off_main(move || {
        let rpc = RpcClient::citrate();
        let providers = RouterReader::new(&rpc, router)?.providers(&hash)?;
        quote_registry(router, &hash, &providers, &input, max)
    })
    .await
}

/// **escalation_registry_request** — raise the HIC-1 approval for one registry escalation. The
/// member must echo the ceiling they were shown; the transaction is a pending SignatureCeremony
/// (nothing is signed here). Off while no router is pinned for 40204.
#[tauri::command]
pub async fn escalation_registry_request(
    app_h: tauri::AppHandle,
    model_hash: String,
    input: String,
    max_price_wei: String,
    shown_max_price_wei: String,
) -> Result<(), String> {
    let router = pinned_router()?;
    let hash = parse_model_hash(&model_hash)?;
    if max_price_wei != shown_max_price_wei {
        return Err("the price ceiling changed after it was shown; quote again".into());
    }
    let max: u128 = max_price_wei
        .parse()
        .map_err(|_| "the price ceiling must be a whole number of wei".to_string())?;
    validate_registry_request(&input, max)?;
    crate::blocking::off_main(move || {
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
        let calldata = encode_request_inference(&hash, input.as_bytes(), max);
        let rpc = RpcClient::citrate();
        let gas = estimate_request_gas(&rpc, &wallet.address, router, &calldata, max)?;
        let raw = request_tx_json(&wallet.address, router, &calldata, max, gas);
        ceremony.0.request(crate::ceremony::SignatureIntent {
            origin: "agent:hermes".to_string(),
            kind: crate::ceremony::IntentKind::Transaction,
            chain_id: 40204,
            raw,
        });
        Ok(())
    })
    .await
}

/// **escalation_registry_result** — read one of the member's router requests.
#[tauri::command]
pub async fn escalation_registry_result(request_id: u64) -> Result<RegistryResultView, String> {
    let router = pinned_router()?;
    crate::blocking::off_main(move || {
        let rpc = RpcClient::citrate();
        let r = RouterReader::new(&rpc, router)?.request(request_id)?;
        Ok(result_view(request_id, &r))
    })
    .await
}

/// The member's own router requests and the refund credited to them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryMineView {
    pub router: String,
    /// Request ids, newest last (at most [`MAX_USER_REQUESTS`]).
    pub request_ids: Vec<u64>,
    /// Wei credited back (unused ceilings, expired requests), claimable with `claimRefund()`.
    pub refund_owed_wei: String,
}

/// **escalation_registry_mine** — the member's router request ids and claimable refund.
#[tauri::command]
pub async fn escalation_registry_mine(app_h: tauri::AppHandle) -> Result<RegistryMineView, String> {
    let router = pinned_router()?;
    crate::blocking::off_main(move || {
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
        let rpc = RpcClient::citrate();
        let reader = RouterReader::new(&rpc, router)?;
        Ok(RegistryMineView {
            router: router.to_ascii_lowercase(),
            request_ids: reader.user_requests(&wallet.address)?,
            refund_owed_wei: reader.refund_owed(&wallet.address)?,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("inference_router_tests.rs");
}
