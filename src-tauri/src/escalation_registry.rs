//! HUP-S1.5 — registry escalation (US-1.5 AC1-AC3), core half: "escalate to a registry model and
//! pay for it, at a price I see".
//!
//! The route, end to end:
//!
//! 1. **Read the router (AC1).** `InferenceRouter.getProviders(modelHash)` and
//!    `providers(address)` over `eth_call` give each provider's endpoint, `minPrice`, load and
//!    status. [`select_provider`] keeps active providers with a free slot, a usable endpoint
//!    (https, or http to this computer) and a price within [`MAX_PRICE_BASE_UNITS`], and picks the
//!    cheapest (then the most reliable).
//! 2. **Quote (AC2).** The quote names the provider, its endpoint host and the price in the pinned
//!    asset's units. It is valid for [`crate::escalation::QUOTE_TTL_MS`] and runs at most once.
//! 3. **Approve (AC2, Rule 3).** Core builds the EIP-3009 `TransferWithAuthorization` itself
//!    (`ceremony::x402`, the one form Rule-3 ADR D3 pins), with a CSPRNG nonce and a validity
//!    window of at most ten minutes, and opens an ordinary HIC-1 ceremony for it. The member
//!    approves that ceremony id in the approval window; there is no budgeted (automatic) path,
//!    because the B-2 budget stays inert while the asset allowlist is empty (ADR O-1).
//! 4. **Run (AC3).** Core checks that the approved signature recovers to the payer over exactly the
//!    authorization it built, then calls the sidecar (`POST /escalations/registry`), which sends one
//!    chat completion with the payment as the `X-PAYMENT` header and returns the answer and the
//!    provider's `X-PAYMENT-RESPONSE` receipt. The sidecar meters the escalation with the receipt.
//! 5. **Confirm on chain.** Core reads the asset's `authorizationState(payer, nonce)`: `true` means
//!    the authorization was consumed (the payment settled). The result is kept in the registry
//!    history with the provider's claimed transaction.
//!
//! The route is **off** unless [`pinned_route`] finds both an `InferenceRouter` in the address book
//! and an allowlisted x402 asset (`web_budget::X402_ASSET_ALLOWLIST`). On 40204 today neither is
//! pinned: the router and WrappedSALT are deployed after the reroll, but the core address book has
//! no `InferenceRouter` entry and the asset allowlist is an owner decision (O-1). Nothing changes
//! for members until both land. The whole path is proven against a local anvil deploy of the
//! citrate-chain contracts (`scripts/escalation-registry-anvil-e2e.sh`).

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use citrate_core_kit::ceremony::x402::{self, X402Authorization, X402Domain};
use citrate_core_kit::rpc::{RpcClient, RpcError, RpcTransport};
use serde::{Deserialize, Serialize};

use crate::escalation::{validate_base_url, RegistryStatusView, QUOTE_TTL_MS};

/// `getProviders(bytes32)`.
pub const SEL_GET_PROVIDERS: [u8; 4] = [0x33, 0x17, 0x95, 0xaa];
/// `providers(address)` (the public struct getter; it omits the `supportedModels` array).
pub const SEL_PROVIDERS: [u8; 4] = [0x07, 0x87, 0xbc, 0x27];
/// `authorizationState(address,bytes32)` on an EIP-3009 asset.
pub const SEL_AUTHORIZATION_STATE: [u8; 4] = [0xe9, 0x4a, 0x01, 0x02];
/// Providers read per quote (the router never prunes its lists).
pub const MAX_PROVIDERS_READ: usize = 32;
/// The longest endpoint string accepted from the router.
pub const MAX_ENDPOINT_LEN: usize = 512;
/// The highest price per request a quote accepts: 1 whole token (18 decimals), the ADR O-2
/// `per_signature_max` default. Pending owner sign-off.
pub const MAX_PRICE_BASE_UNITS: u128 = 1_000_000_000_000_000_000;
/// The x402 validity window core asks for (the ADR cap, ten minutes).
pub const AUTHORIZATION_VALIDITY_SECS: u64 = x402::VALIDITY_MAX_SECS;
/// A run is refused when the authorization has less than this left (the provider must settle).
pub const RUN_MARGIN_SECS: u64 = 30;
/// Registry runs kept in the history.
pub const HISTORY_CAP: usize = 200;
/// The registry history file inside the escalation folder.
pub const HISTORY_FILE: &str = "registry-history.json";
/// Outstanding registry quotes kept (oldest dropped first).
pub const MAX_QUOTES: usize = 16;

/// A registry-route failure. Messages are plain language for the member and the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegError {
    Off(Vec<String>),
    Invalid(String),
    Chain(String),
    NoProvider,
    UnknownQuote,
    QuoteExpired,
    PriceNotShown { quoted: String, shown: String },
    Signature,
    Storage(String),
}

impl std::fmt::Display for RegError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegError::Off(m) => write!(
                f,
                "registry escalation is not available: {}",
                m.join("; ")
            ),
            RegError::Invalid(m) => write!(f, "{m}"),
            RegError::Chain(m) => write!(f, "could not read the chain: {m}"),
            RegError::NoProvider => f.write_str(
                "no provider on the InferenceRouter serves this model right now within the price limit",
            ),
            RegError::UnknownQuote => {
                f.write_str("that registry quote was already used or is unknown; ask for a new quote")
            }
            RegError::QuoteExpired => {
                f.write_str("that registry quote or its payment authorization expired; ask for a new quote")
            }
            RegError::PriceNotShown { quoted, shown } => write!(
                f,
                "the price shown ({shown}) is not the quoted price ({quoted}); nothing was signed or sent"
            ),
            RegError::Signature => f.write_str(
                "the approved signature does not match the payment Citrate Core built; nothing was sent",
            ),
            RegError::Storage(m) => write!(f, "registry history: {m}"),
        }
    }
}

impl std::error::Error for RegError {}

impl From<RpcError> for RegError {
    fn from(e: RpcError) -> Self {
        RegError::Chain(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Configuration: the router and the asset
// ---------------------------------------------------------------------------

/// The x402 asset the route pays in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryAsset {
    pub domain: X402Domain,
    pub symbol: String,
    pub decimals: u32,
}

/// Everything the route needs from chain configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRoute {
    /// Lower-case `0x` router address.
    pub router: String,
    pub asset: RegistryAsset,
}

impl RegistryRoute {
    /// CAIP-2 network id for the x402 payload.
    pub fn network(&self) -> String {
        format!("eip155:{}", self.asset.domain.chain_id)
    }
}

/// The route from explicit inputs (`router` from the address book, `assets` from the allowlist).
/// Returns what is missing when it cannot run.
pub fn route_from(
    router: Option<&str>,
    assets: &[citrate_core_kit::web_budget::X402Asset],
    chain_id: u64,
) -> Result<RegistryRoute, Vec<String>> {
    let mut missing = Vec::new();
    let router = match router.map(|r| x402::parse_address(r, "router")) {
        Some(Ok(a)) => Some(x402::fmt_address(&a)),
        Some(Err(_)) | None => {
            missing.push(
                "the InferenceRouter address is not pinned in Citrate Core's address book for this chain"
                    .to_string(),
            );
            None
        }
    };
    let asset = assets
        .iter()
        .find(|a| a.chain_id == chain_id)
        .and_then(|a| X402Domain::from_asset(a).ok());
    if asset.is_none() {
        missing.push(
            "no x402 payment token is allowlisted (a token with TransferWithAuthorization such as Wrapped SALT; owner decision O-1)"
                .to_string(),
        );
    }
    match (router, asset) {
        (Some(router), Some(domain)) => Ok(RegistryRoute {
            router,
            asset: RegistryAsset {
                domain,
                // The pinned asset today can only be Wrapped SALT (18 decimals, like native SALT).
                symbol: "wSALT".into(),
                decimals: 18,
            },
        }),
        _ => Err(missing),
    }
}

/// The production route: the address book's router and the kit's asset allowlist.
pub fn pinned_route() -> Result<RegistryRoute, Vec<String>> {
    route_from(
        crate::addresses::inference_router(),
        citrate_core_kit::web_budget::X402_ASSET_ALLOWLIST,
        crate::addresses::chain_id(),
    )
}

/// The status Settings shows.
pub fn status_view(route: &Result<RegistryRoute, Vec<String>>) -> RegistryStatusView {
    match route {
        Ok(r) => RegistryStatusView {
            enabled: true,
            reason: format!(
                "Registry models are on: providers come from the InferenceRouter at {} and each request is paid in {} after you approve it.",
                r.router, r.asset.symbol
            ),
            missing: Vec::new(),
        },
        Err(missing) => RegistryStatusView {
            enabled: false,
            reason: "Registry escalation is not available yet. Escalations use your own endpoints."
                .to_string(),
            missing: missing.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// ABI: reading the router
// ---------------------------------------------------------------------------

/// One provider as the router reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterProvider {
    pub address: String,
    pub endpoint: String,
    pub stake: u128,
    pub min_price: u128,
    pub max_concurrent: u128,
    pub current_load: u128,
    pub total_inferences: u128,
    /// Basis points (10000 = 100%).
    pub success_rate: u128,
    pub is_active: bool,
}

fn abi_word(data: &[u8], i: usize) -> Result<&[u8], RegError> {
    data.get(i * 32..i * 32 + 32)
        .ok_or_else(|| RegError::Chain("the router's answer is too short".into()))
}

/// A word as `u128`, refusing values above `u128::MAX` (no on-chain amount here is that large).
fn abi_u128(w: &[u8]) -> Result<u128, RegError> {
    if w.len() != 32 || w[..16].iter().any(|b| *b != 0) {
        return Err(RegError::Chain("a router value is out of range".into()));
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&w[16..]);
    Ok(u128::from_be_bytes(b))
}

fn abi_usize(w: &[u8]) -> Result<usize, RegError> {
    usize::try_from(abi_u128(w)?).map_err(|_| RegError::Chain("a router offset is out of range".into()))
}

fn abi_address(w: &[u8]) -> Result<String, RegError> {
    if w.len() != 32 || w[..12].iter().any(|b| *b != 0) {
        return Err(RegError::Chain("a router address is malformed".into()));
    }
    Ok(format!("0x{}", hex::encode(&w[12..])))
}

fn abi_bool(w: &[u8]) -> Result<bool, RegError> {
    match abi_u128(w)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(RegError::Chain("a router flag is malformed".into())),
    }
}

/// Decode `address[]` (the return of `getProviders`).
pub fn decode_address_array(data: &[u8]) -> Result<Vec<String>, RegError> {
    let off = abi_usize(abi_word(data, 0)?)?;
    if off % 32 != 0 {
        return Err(RegError::Chain("the provider list is malformed".into()));
    }
    let base = off / 32;
    let len = abi_usize(abi_word(data, base)?)?;
    (0..len)
        .take(MAX_PROVIDERS_READ)
        .map(|i| abi_address(abi_word(data, base + 1 + i)?))
        .collect()
}

/// Decode the `providers(address)` getter: `(address provider, string endpoint, uint256 stake,
/// uint256 minPrice, uint256 maxConcurrent, uint256 currentLoad, uint256 totalInferences, uint256
/// successRate, bool isActive)`.
pub fn decode_provider(data: &[u8]) -> Result<RouterProvider, RegError> {
    let address = abi_address(abi_word(data, 0)?)?;
    let off = abi_usize(abi_word(data, 1)?)?;
    let len = abi_usize(
        data.get(off..off + 32)
            .ok_or_else(|| RegError::Chain("the provider endpoint is missing".into()))?,
    )?;
    if len > MAX_ENDPOINT_LEN {
        return Err(RegError::Chain("the provider endpoint is too long".into()));
    }
    let bytes = data
        .get(off + 32..off + 32 + len)
        .ok_or_else(|| RegError::Chain("the provider endpoint is truncated".into()))?;
    let endpoint = String::from_utf8(bytes.to_vec())
        .map_err(|_| RegError::Chain("the provider endpoint is not text".into()))?;
    Ok(RouterProvider {
        address,
        endpoint,
        stake: abi_u128(abi_word(data, 2)?)?,
        min_price: abi_u128(abi_word(data, 3)?)?,
        max_concurrent: abi_u128(abi_word(data, 4)?)?,
        current_load: abi_u128(abi_word(data, 5)?)?,
        total_inferences: abi_u128(abi_word(data, 6)?)?,
        success_rate: abi_u128(abi_word(data, 7)?)?,
        is_active: abi_bool(abi_word(data, 8)?)?,
    })
}

/// Parse a model hash (`0x` + 64 hex) to 32 bytes.
pub fn parse_model_hash(s: &str) -> Result<[u8; 32], RegError> {
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| RegError::Invalid("the model hash must be 0x and 64 hex digits".into()))?;
    let v = hex::decode(h)
        .map_err(|_| RegError::Invalid("the model hash must be 0x and 64 hex digits".into()))?;
    v.as_slice()
        .try_into()
        .map_err(|_| RegError::Invalid("the model hash must be 0x and 64 hex digits".into()))
}

fn call_data(sel: [u8; 4], words: &[[u8; 32]]) -> String {
    let mut out = Vec::with_capacity(4 + 32 * words.len());
    out.extend_from_slice(&sel);
    for w in words {
        out.extend_from_slice(w);
    }
    format!("0x{}", hex::encode(out))
}

fn addr_word(a: &str) -> Result<[u8; 32], RegError> {
    let b = x402::parse_address(a, "address").map_err(|e| RegError::Invalid(e.to_string()))?;
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(&b);
    Ok(w)
}

/// Every provider the router lists for `model_hash` (at most [`MAX_PROVIDERS_READ`]).
pub fn read_providers<T: RpcTransport>(
    rpc: &RpcClient<T>,
    router: &str,
    model_hash: &[u8; 32],
) -> Result<Vec<RouterProvider>, RegError> {
    let list = rpc.eth_call(serde_json::json!({
        "to": router,
        "data": call_data(SEL_GET_PROVIDERS, &[*model_hash]),
    }))?;
    let mut out = Vec::new();
    for addr in decode_address_array(&list)? {
        let raw = rpc.eth_call(serde_json::json!({
            "to": router,
            "data": call_data(SEL_PROVIDERS, &[addr_word(&addr)?]),
        }))?;
        out.push(decode_provider(&raw)?);
    }
    Ok(out)
}

/// The provider a quote uses: active, a free slot, a usable endpoint, a non-zero price within
/// `max_price`; the cheapest, then the most reliable, then the least loaded, then by address.
pub fn select_provider(providers: &[RouterProvider], max_price: u128) -> Option<RouterProvider> {
    providers
        .iter()
        .filter(|p| {
            p.is_active
                && p.current_load < p.max_concurrent
                && p.min_price > 0
                && p.min_price <= max_price
                && p.endpoint.len() <= MAX_ENDPOINT_LEN
                && validate_base_url(&p.endpoint).is_ok()
        })
        .min_by(|a, b| {
            a.min_price
                .cmp(&b.min_price)
                .then(b.success_rate.cmp(&a.success_rate))
                .then(a.current_load.cmp(&b.current_load))
                .then(a.address.cmp(&b.address))
        })
        .cloned()
}

/// `authorizationState(payer, nonce)` on the asset: `true` once the authorization was consumed.
pub fn authorization_settled<T: RpcTransport>(
    rpc: &RpcClient<T>,
    asset: &str,
    payer: &str,
    nonce: &str,
) -> Result<bool, RegError> {
    let n = hex::decode(nonce.strip_prefix("0x").unwrap_or(nonce))
        .ok()
        .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
        .ok_or_else(|| RegError::Invalid("bad nonce".into()))?;
    let raw = rpc.eth_call(serde_json::json!({
        "to": asset,
        "data": call_data(SEL_AUTHORIZATION_STATE, &[addr_word(payer)?, n]),
    }))?;
    abi_bool(abi_word(&raw, 0)?)
}

// ---------------------------------------------------------------------------
// Quotes, approval and runs
// ---------------------------------------------------------------------------

/// The longest prompt (and system prompt) accepted, matching the sidecar.
pub const MAX_PROMPT_BYTES: usize = crate::escalation::MAX_PROMPT_BYTES;

/// A registry quote: what the member is shown before anything is signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryQuoteView {
    pub quote_id: String,
    pub model_hash: String,
    pub provider: String,
    pub provider_host: String,
    /// The price in base units of the asset, decimal.
    pub price_base_units: String,
    /// e.g. `0.01 wSALT`.
    pub price_label: String,
    pub asset: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
struct QuoteEntry {
    view: RegistryQuoteView,
    endpoint: String,
    system: Option<String>,
    prompt: String,
    max_tokens: u32,
    stage: Stage,
}

#[derive(Debug, Clone)]
enum Stage {
    Quoted,
    Requested {
        authorization: X402Authorization,
        domain: X402Domain,
    },
}

/// One registry run, as the history keeps it (no prompt, no answer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryRunRecord {
    pub escalation_id: String,
    pub quote_id: String,
    pub at_ms: u64,
    pub model_hash: String,
    pub provider: String,
    pub asset: String,
    pub value_base_units: String,
    pub payer: String,
    pub nonce: String,
    /// The provider answered (2xx).
    pub answered: bool,
    /// The request (with the signed authorization) may have reached the provider.
    pub sent: bool,
    /// The transaction the provider says settled the payment.
    pub claimed_transaction: Option<String>,
    /// Core's own check: `authorizationState(payer, nonce)`. `None` when the chain could not be
    /// read; `Some(false)` means not settled (yet).
    pub settled_on_chain: Option<bool>,
}

/// In-memory quotes plus the persisted history.
#[derive(Debug, Default)]
pub struct RegistryBook {
    quotes: VecDeque<QuoteEntry>,
    pub history: VecDeque<RegistryRunRecord>,
}

/// What a run needs, taken out of the book (the quote is spent).
#[derive(Debug, Clone)]
pub struct Approved {
    pub view: RegistryQuoteView,
    pub endpoint: String,
    pub system: Option<String>,
    pub prompt: String,
    pub max_tokens: u32,
    pub authorization: X402Authorization,
    pub domain: X402Domain,
    pub signature: String,
}

fn host_of(url: &str) -> String {
    crate::escalation::host_of(url)
}

impl RegistryBook {
    /// Quote `provider` for this request. Nothing is signed.
    #[allow(clippy::too_many_arguments)]
    pub fn quote(
        &mut self,
        route: &RegistryRoute,
        model_hash: &str,
        provider: &RouterProvider,
        prompt: &str,
        system: Option<String>,
        max_tokens: u32,
        now_ms: u64,
        quote_id: String,
    ) -> Result<RegistryQuoteView, RegError> {
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT_BYTES {
            return Err(RegError::Invalid("the prompt must be 1 to 65536 bytes".into()));
        }
        if system.as_deref().is_some_and(|s| s.len() > MAX_PROMPT_BYTES) {
            return Err(RegError::Invalid("the system prompt is too long".into()));
        }
        if max_tokens == 0 || max_tokens > crate::escalation::MAX_ESCALATION_TOKENS {
            return Err(RegError::Invalid("max tokens must be 1 to 8192".into()));
        }
        let view = RegistryQuoteView {
            quote_id,
            model_hash: model_hash.to_ascii_lowercase(),
            provider: provider.address.clone(),
            provider_host: host_of(&provider.endpoint),
            price_base_units: provider.min_price.to_string(),
            price_label: format!(
                "{} {}",
                x402::format_units(provider.min_price, route.asset.decimals),
                route.asset.symbol
            ),
            asset: route.asset.domain.verifying_contract.clone(),
            expires_at_ms: now_ms.saturating_add(QUOTE_TTL_MS),
        };
        self.quotes.push_back(QuoteEntry {
            view: view.clone(),
            endpoint: provider.endpoint.clone(),
            system,
            prompt: prompt.to_string(),
            max_tokens,
            stage: Stage::Quoted,
        });
        while self.quotes.len() > MAX_QUOTES {
            self.quotes.pop_front();
        }
        Ok(view)
    }

    fn find(&mut self, quote_id: &str, now_ms: u64) -> Result<usize, RegError> {
        let i = self
            .quotes
            .iter()
            .position(|q| q.view.quote_id == quote_id)
            .ok_or(RegError::UnknownQuote)?;
        if now_ms >= self.quotes[i].view.expires_at_ms {
            self.quotes.remove(i);
            return Err(RegError::QuoteExpired);
        }
        Ok(i)
    }

    /// Build the authorization for a shown quote (the member saw `shown_price`). The returned
    /// authorization goes to `SignatureCeremony::request_x402`; nothing is signed here.
    pub fn authorize(
        &mut self,
        route: &RegistryRoute,
        quote_id: &str,
        shown_price: &str,
        payer: &str,
        now_ms: u64,
        nonce: String,
    ) -> Result<X402Authorization, RegError> {
        let i = self.find(quote_id, now_ms)?;
        let q = &mut self.quotes[i];
        if !matches!(q.stage, Stage::Quoted) {
            return Err(RegError::UnknownQuote);
        }
        if q.view.price_base_units != shown_price {
            return Err(RegError::PriceNotShown {
                quoted: q.view.price_base_units.clone(),
                shown: shown_price.to_string(),
            });
        }
        let auth = x402::build_authorization(
            payer,
            &q.view.provider,
            &q.view.price_base_units,
            now_ms / 1000,
            AUTHORIZATION_VALIDITY_SECS,
            nonce,
        )
        .map_err(|e| RegError::Invalid(e.to_string()))?;
        q.stage = Stage::Requested {
            authorization: auth.clone(),
            domain: route.asset.domain.clone(),
        };
        Ok(auth)
    }

    /// Spend a requested quote with the member's approved signature. The signature must recover to
    /// the payer over exactly the authorization built in [`authorize`](Self::authorize), and the
    /// authorization must have at least [`RUN_MARGIN_SECS`] left. One run per quote.
    pub fn take_approved(
        &mut self,
        quote_id: &str,
        signature: &str,
        now_ms: u64,
    ) -> Result<Approved, RegError> {
        let i = self
            .quotes
            .iter()
            .position(|q| q.view.quote_id == quote_id)
            .ok_or(RegError::UnknownQuote)?;
        let (authorization, domain) = match &self.quotes[i].stage {
            Stage::Requested {
                authorization,
                domain,
            } => (authorization.clone(), domain.clone()),
            Stage::Quoted => return Err(RegError::UnknownQuote),
        };
        if authorization.valid_before <= (now_ms / 1000).saturating_add(RUN_MARGIN_SECS) {
            self.quotes.remove(i);
            return Err(RegError::QuoteExpired);
        }
        x402::verify(&domain, &authorization, signature).map_err(|_| RegError::Signature)?;
        let q = self.quotes.remove(i).ok_or(RegError::UnknownQuote)?;
        let sig = signature.strip_prefix("0x").unwrap_or(signature);
        Ok(Approved {
            view: q.view,
            endpoint: q.endpoint,
            system: q.system,
            prompt: q.prompt,
            max_tokens: q.max_tokens,
            authorization,
            domain,
            signature: format!("0x{}", sig.to_ascii_lowercase()),
        })
    }

    pub fn record(&mut self, rec: RegistryRunRecord) {
        self.history.push_back(rec);
        while self.history.len() > HISTORY_CAP {
            self.history.pop_front();
        }
    }
}

/// The `POST /escalations/registry` body for the sidecar. No key: the payment is a signature over
/// an authorization the member approved.
pub fn sidecar_body(a: &Approved, escalation_id: &str) -> String {
    serde_json::json!({
        "escalationId": escalation_id,
        "baseUrl": a.endpoint,
        "model": a.view.model_hash,
        "system": a.system,
        "prompt": a.prompt,
        "maxTokens": a.max_tokens,
        "payment": {
            "network": format!("eip155:{}", a.domain.chain_id),
            "asset": a.domain.verifying_contract,
            "from": a.authorization.from,
            "to": a.authorization.to,
            "value": a.authorization.value,
            "validAfter": a.authorization.valid_after,
            "validBefore": a.authorization.valid_before,
            "nonce": a.authorization.nonce,
            "signature": a.signature,
        },
    })
    .to_string()
}

/// The provider's receipt, as the sidecar relays it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptView {
    pub success: bool,
    #[serde(default)]
    pub transaction: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub payer: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SidecarRegistryOutcome {
    content: String,
    #[serde(default)]
    receipt: Option<ReceiptView>,
}

/// Read the sidecar's answer: `Ok((content, receipt))`, or `Err((sent, message))`. An answer core
/// cannot read counts as possibly sent.
pub fn interpret_sidecar(
    status: u16,
    body: &str,
) -> Result<(String, Option<ReceiptView>), (bool, String)> {
    if (200..300).contains(&status) {
        return serde_json::from_str::<SidecarRegistryOutcome>(body)
            .map(|o| (o.content, o.receipt))
            .map_err(|_| (true, "the registry answer could not be read".to_string()));
    }
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let msg: String = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("the registry escalation failed")
        .chars()
        .take(200)
        .collect();
    let sent = v.get("sent").and_then(|s| s.as_bool()).unwrap_or(true);
    Err((sent, msg))
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

/// Load the history (missing or unreadable: empty; an unreadable file is kept aside).
pub fn load_history(dir: &Path, now_ms: u64) -> VecDeque<RegistryRunRecord> {
    let path = dir.join(HISTORY_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<VecDeque<RegistryRunRecord>>(&bytes) {
            Ok(h) => h,
            Err(_) => {
                let _ = std::fs::rename(
                    &path,
                    dir.join(format!("registry-history.unreadable-{now_ms}.json")),
                );
                VecDeque::new()
            }
        },
        Err(_) => VecDeque::new(),
    }
}

/// Persist the history atomically.
pub fn save_history(dir: &Path, h: &VecDeque<RegistryRunRecord>) -> Result<(), RegError> {
    std::fs::create_dir_all(dir).map_err(|e| RegError::Storage(e.kind().to_string()))?;
    let bytes =
        serde_json::to_vec_pretty(h).map_err(|_| RegError::Storage("encode history".into()))?;
    let path = dir.join(HISTORY_FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| RegError::Storage(e.kind().to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| RegError::Storage(e.kind().to_string()))
}

// ---------------------------------------------------------------------------
// Tauri state and commands
// ---------------------------------------------------------------------------

/// Lazily loaded on first use.
#[derive(Default)]
pub struct RegistryState(Mutex<Option<(std::path::PathBuf, RegistryBook)>>);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn with_book<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    f: impl FnOnce(&Path, &mut RegistryBook) -> Result<T, RegError>,
) -> Result<T, String> {
    use tauri::Manager;
    let st = app
        .try_state::<RegistryState>()
        .ok_or("internal: registry state unavailable")?;
    let mut guard = st.0.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("escalation");
        let book = RegistryBook {
            quotes: VecDeque::new(),
            history: load_history(&dir, now_ms()),
        };
        *guard = Some((dir, book));
    }
    let Some((dir, book)) = guard.as_mut() else {
        return Err("internal: registry state unavailable".into());
    };
    f(dir, book).map_err(|e| e.to_string())
}

fn route_or_off() -> Result<RegistryRoute, String> {
    pinned_route().map_err(|m| RegError::Off(m).to_string())
}

/// **escalation_registry_status** — whether the registry route can run, and what is missing.
#[tauri::command]
pub async fn escalation_registry_status() -> Result<RegistryStatusView, String> {
    Ok(status_view(&pinned_route()))
}

/// **escalation_registry_quote** — read the router and quote the cheapest usable provider for
/// `model_hash`. Signs nothing.
#[tauri::command]
pub async fn escalation_registry_quote(
    app: tauri::AppHandle,
    model_hash: String,
    prompt: String,
    system: Option<String>,
    max_tokens: Option<u32>,
) -> Result<RegistryQuoteView, String> {
    crate::blocking::off_main(move || {
        let route = route_or_off()?;
        let mh = parse_model_hash(&model_hash).map_err(|e| e.to_string())?;
        let rpc = RpcClient::citrate();
        let providers = read_providers(&rpc, &route.router, &mh).map_err(|e| e.to_string())?;
        let provider = select_provider(&providers, MAX_PRICE_BASE_UNITS)
            .ok_or_else(|| RegError::NoProvider.to_string())?;
        with_book(&app, |_, b| {
            b.quote(
                &route,
                &model_hash,
                &provider,
                &prompt,
                system,
                max_tokens.unwrap_or(crate::escalation::DEFAULT_ESCALATION_TOKENS),
                now_ms(),
                format!("rq-{:016x}", rand::random::<u64>()),
            )
        })
    })
    .await
}

/// **escalation_registry_request** — the member saw `shown_price_base_units` for `quote_id`: build
/// the x402 authorization and open an HIC-1 ceremony for it. Returns the pending ceremony; the
/// member approves it in the approval window (`sign_approve`), which returns the signature.
#[tauri::command]
pub async fn escalation_registry_request(
    app: tauri::AppHandle,
    quote_id: String,
    shown_price_base_units: String,
) -> Result<citrate_core_kit::ceremony::CeremonyView, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let route = route_or_off()?;
        let custody = app
            .try_state::<citrate_core_kit::custody::CustodyState>()
            .ok_or("internal: custody state unavailable")?;
        let payer = citrate_core_kit::wallet::address(&custody.0)
            .map_err(|_| "unlock your wallet to approve a registry payment".to_string())?
            .address;
        let (auth, resource) = with_book(&app, |_, b| {
            let auth = b.authorize(
                &route,
                &quote_id,
                &shown_price_base_units,
                &payer,
                now_ms(),
                x402::fresh_nonce(),
            )?;
            let resource = b
                .quotes
                .iter()
                .find(|q| q.view.quote_id == quote_id)
                .map(|q| format!("model {} via {}", q.view.model_hash, q.view.provider_host))
                .unwrap_or_default();
            Ok((auth, resource))
        })?;
        let ceremony = app
            .try_state::<citrate_core_kit::ceremony::CeremonyState>()
            .ok_or("internal: ceremony state unavailable")?;
        ceremony
            .0
            .request_x402(citrate_core_kit::ceremony::X402SignRequest {
                origin: "Hermes (registry escalation)".into(),
                domain: route.asset.domain.clone(),
                authorization: auth,
                resource,
                asset_symbol: route.asset.symbol.clone(),
                asset_decimals: route.asset.decimals,
            })
            .map_err(|e| e.to_string())
    })
    .await
}

/// The result of a registry run, as the agent and Settings see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryRunView {
    pub escalation_id: String,
    pub content: String,
    pub provider: String,
    pub price_label: String,
    pub receipt: Option<ReceiptView>,
    pub settled_on_chain: Option<bool>,
}

/// **escalation_registry_run** — run an approved quote: check the signature against the payment
/// core built, send it through the sidecar, then check settlement on chain.
#[tauri::command]
pub async fn escalation_registry_run(
    app: tauri::AppHandle,
    quote_id: String,
    signature: String,
) -> Result<RegistryRunView, String> {
    crate::blocking::off_main(move || {
        let approved = with_book(&app, |_, b| b.take_approved(&quote_id, &signature, now_ms()))?;
        let escalation_id = format!("esc-reg-{:016x}", rand::random::<u64>());
        let body = sidecar_body(&approved, &escalation_id);
        let outcome = match crate::hermes::sidecar_escalate_registry(&app, &body) {
            Ok((status, text)) => interpret_sidecar(status, &text),
            Err(sent) => Err((
                sent,
                "the Hermes agent is not running; start it, then try again".to_string(),
            )),
        };
        let (answered, sent, content, receipt) = match outcome {
            Ok((content, receipt)) => (true, true, Ok(content), receipt),
            Err((sent, msg)) => (false, sent, Err(msg), None),
        };
        let settled = if sent {
            authorization_settled(
                &RpcClient::citrate(),
                &approved.domain.verifying_contract,
                &approved.authorization.from,
                &approved.authorization.nonce,
            )
            .ok()
        } else {
            Some(false)
        };
        let rec = RegistryRunRecord {
            escalation_id: escalation_id.clone(),
            quote_id: approved.view.quote_id.clone(),
            at_ms: now_ms(),
            model_hash: approved.view.model_hash.clone(),
            provider: approved.view.provider.clone(),
            asset: approved.domain.verifying_contract.clone(),
            value_base_units: approved.authorization.value.clone(),
            payer: approved.authorization.from.clone(),
            nonce: approved.authorization.nonce.clone(),
            answered,
            sent,
            claimed_transaction: receipt.as_ref().and_then(|r| r.transaction.clone()),
            settled_on_chain: settled,
        };
        with_book(&app, |dir, b| {
            b.record(rec);
            save_history(dir, &b.history)
        })?;
        let content = content?;
        Ok(RegistryRunView {
            escalation_id,
            content,
            provider: approved.view.provider.clone(),
            price_label: approved.view.price_label.clone(),
            receipt,
            settled_on_chain: settled,
        })
    })
    .await
}

/// **escalation_registry_history** — recent registry runs (no prompts, no answers).
#[tauri::command]
pub async fn escalation_registry_history(
    app: tauri::AppHandle,
) -> Result<Vec<RegistryRunRecord>, String> {
    crate::blocking::off_main(move || {
        with_book(&app, |_, b| Ok(b.history.iter().rev().cloned().collect()))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("escalation_registry_tests.rs");
}
