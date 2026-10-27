//! D-11 (RC52) — the provider commands for Settings › Models and the usage view (ADR P-7, P-10).
//!
//! **No command returns a key.** A key goes in one way (`providers_key_set`, a `SecretString`
//! argument) and is tested live before it is stored; status reports only whether one is stored.
//! Every view below is built from non-secret fields, and the `ApiKey` type cannot be serialized.
//!
//! ## Data sources (Rule 7)
//! | Command | Source |
//! |---|---|
//! | `providers_status` | pinned vendor table (`provider_egress::VENDORS`), keychain presence of `provider-key:<id>`, the sealed guard file (caps, halt, approved/listed models, prices, custom endpoints, key-set time) |
//! | `providers_usage_summary` | the signed usage log (`providers/usage.jsonl`): calls, tokens and spend per provider for the UTC day and month, unsettled calls, the latest calls (metadata only), chain status |
//! | `providers_caps_set`, `providers_price_set`, `providers_halt`, `providers_resume` | the sealed guard file (written) |
//! | `providers_key_set`, `providers_key_test` | the vendor's own free endpoint (`GET /models`), live, through the pinned transport; the keychain (written on success only) |
//! | `providers_key_remove` | the keychain (deleted) |
//! | `providers_custom_add`, `providers_custom_confirm`, `providers_custom_remove` | the sealed guard file (custom endpoints), the keychain on remove |
//! | `providers_usage_log_reset` | the usage log (a broken file is archived, a new chain starts) |

use crate::model_provider::{ChatMessage, ChatRequest, ModelProvider, ProviderId, Role, Vendor};
use crate::model_proxy::{now_ms, ModelProxy, ProxyError};
use crate::provider_egress::{vendor_spec, CustomEndpoint};
use crate::provider_guard::{Caps, GuardBook, Halt, Price};
use crate::provider_usage_log::UsageLog;
use crate::secret::SecretString;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Views (non-secret by construction)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaltView {
    pub kind: String,
    pub message: String,
    pub at_ms: u64,
}

impl From<&Halt> for HaltView {
    fn from(h: &Halt) -> Self {
        let kind = serde_json::to_value(&h.reason)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
            .unwrap_or_default();
        HaltView {
            kind,
            message: h.reason.describe(),
            at_ms: h.at_ms,
        }
    }
}

/// One model core knows for a provider, with its price and where that price came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceView {
    pub id: String,
    pub price: Option<Price>,
    /// `member` (entered in Settings, wins), `published` (the adapter's cited vendor price), or
    /// `None` (unknown: refused while a spending cap is set).
    pub source: Option<&'static str>,
    /// The least core reserves against the caps before one call to this model is sent (an empty
    /// prompt, the adapter's output bound), in micro-USD. `None` when the model has no price.
    pub min_reserve_micros: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    /// `vendor` or `custom`.
    pub kind: &'static str,
    /// A governed vendor on its pinned host (false for a custom endpoint).
    pub verified: bool,
    pub base_url: String,
    pub host: String,
    /// `native` (a governed vendor) or `openai-compatible` (a custom endpoint).
    pub adapter: Option<&'static str>,
    pub key_configured: bool,
    pub key_set_ms: Option<u64>,
    pub key_page: Option<&'static str>,
    pub revoke_page: Option<&'static str>,
    /// Custom endpoints only: whether the member confirmed this exact URL.
    pub confirmed: bool,
    pub halt: Option<HaltView>,
    pub caps: Caps,
    pub approved_models: Vec<String>,
    pub listed_models: Vec<String>,
    pub prices: BTreeMap<String, Price>,
    /// Every known model (adapter, listed, approved, custom, priced), with its price source.
    pub models: Vec<ModelPriceView>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodView {
    pub calls: u64,
    pub tokens: u64,
    pub spend_micros: u64,
    pub unpriced_calls: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageView {
    pub id: String,
    pub today: PeriodView,
    pub month: PeriodView,
    pub unsettled_calls: u64,
    pub caps: Caps,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryView {
    pub log_intact: bool,
    pub log_problem: Option<String>,
    pub records: u64,
    pub as_of_ms: u64,
    pub providers: Vec<ProviderUsageView>,
    /// The latest calls, newest first: metadata only (no content, no session or call ids).
    pub recent_calls: Vec<crate::provider_usage_log::RecentCall>,
    /// Set when this computer's clock reads well before the last record: every call is refused
    /// until the clock is corrected.
    pub clock_problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyTestView {
    pub ok: bool,
    pub listed_models: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpointView {
    pub id: String,
    pub label: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub confirmed: bool,
    /// Always false: a custom endpoint is never a governed vendor.
    pub verified: bool,
}

impl From<&CustomEndpoint> for CustomEndpointView {
    fn from(c: &CustomEndpoint) -> Self {
        CustomEndpointView {
            id: c.id.clone(),
            label: c.label.clone(),
            base_url: c.base_url.clone(),
            models: c.models.clone(),
            confirmed: c.confirmed,
            verified: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Pure builders (tested without Tauri)
// ---------------------------------------------------------------------------

/// The least the guard reserves for one call to `model`: an empty prompt, no tools, the adapter's
/// output bound (the same `input_token_bound` + `output_bound` the proxy reserves with).
fn min_reserve(
    adapter: Option<&dyn ModelProvider>,
    model: &str,
    price: Option<Price>,
) -> Option<u64> {
    let price = price?;
    let probe = ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: String::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }],
        tools: Vec::new(),
        max_tokens: 1,
        stream: false,
    };
    let out = adapter.map_or(u64::from(probe.max_tokens), |a| a.output_bound(&probe));
    Some(price.cost(crate::model_proxy::input_token_bound(&probe), out))
}

/// The models to show for a provider, each with its price and source. The price is the one a
/// call is charged with: `GuardBook::price_for` over `ModelProxy::adapter_price`, the same pair
/// `ModelProxy::call` uses, so the view cannot drift from the charge.
pub fn model_prices(
    g: &GuardBook,
    provider: &str,
    adapter: Option<&dyn ModelProvider>,
    extra: &[String],
) -> Vec<ModelPriceView> {
    let pg = g.get(provider);
    let published = adapter.map(|a| a.models()).unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    let names = published
        .iter()
        .map(|m| &m.id)
        .chain(extra)
        .chain(&pg.listed_models)
        .chain(&pg.approved_models)
        .chain(pg.prices.keys());
    for id in names {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids.into_iter()
        .map(|id| {
            let adapter_price = adapter.and_then(|a| ModelProxy::adapter_price(a, &id));
            let price = g.price_for(provider, &id, adapter_price);
            let source = match price {
                None => None,
                Some(_) if pg.prices.contains_key(&id) => Some("member"),
                Some(_) => Some("published"),
            };
            ModelPriceView {
                min_reserve_micros: min_reserve(adapter, &id, price),
                id,
                price,
                source,
            }
        })
        .collect()
}

/// Every provider id the status lists: the six vendors, then the custom endpoints.
fn all_ids(g: &GuardBook) -> Vec<ProviderId> {
    let mut ids: Vec<ProviderId> = Vendor::ALL.into_iter().map(ProviderId::Vendor).collect();
    ids.extend(g.customs.iter().map(|c| ProviderId::Custom(c.id.clone())));
    ids
}

/// One provider's model list with prices. A custom endpoint is served by the OpenAI-compatible
/// adapter, whose output bound is the request's own `max_tokens` (the `None` case in
/// `min_reserve`); its models are the ones the member listed.
fn models_for(g: &GuardBook, id: &ProviderId) -> Vec<ModelPriceView> {
    match id {
        ProviderId::Vendor(v) => {
            let a = crate::model_proxy::vendor_adapter(*v).0;
            model_prices(g, id.as_str(), Some(a.as_ref()), &[])
        }
        ProviderId::Custom(cid) => {
            let extra = g
                .customs
                .iter()
                .find(|c| &c.id == cid)
                .map(|c| c.models.clone())
                .unwrap_or_default();
            model_prices(g, id.as_str(), None, &extra)
        }
    }
}

pub fn status(proxy: &ModelProxy) -> Vec<ProviderView> {
    let (ids, customs, guards, model_lists) = proxy.with_state(|g, _| {
        let ids = all_ids(g);
        let guards: Vec<_> = ids.iter().map(|id| g.get(id.as_str())).collect();
        let model_lists: Vec<_> = ids.iter().map(|id| models_for(g, id)).collect();
        (ids, g.customs.clone(), guards, model_lists)
    });
    ids.into_iter()
        .zip(guards)
        .zip(model_lists)
        .map(|((id, pg), models)| {
            let key_configured = proxy.key_configured(&id);
            let adapter = proxy.kind_for(&id).map(|k| k.as_str());
            let (name, kind, verified, base_url, key_page, revoke_page, confirmed) = match &id {
                ProviderId::Vendor(v) => {
                    let s = vendor_spec(*v);
                    (
                        s.name.to_string(),
                        "vendor",
                        true,
                        s.base_url.to_string(),
                        Some(s.key_page),
                        Some(s.revoke_page),
                        true,
                    )
                }
                ProviderId::Custom(cid) => {
                    let c = customs.iter().find(|c| &c.id == cid);
                    (
                        c.map(|c| c.label.clone()).unwrap_or_default(),
                        "custom",
                        false,
                        c.map(|c| c.base_url.clone()).unwrap_or_default(),
                        None,
                        None,
                        c.is_some_and(|c| c.confirmed),
                    )
                }
            };
            let host = url::Url::parse(&base_url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_default();
            ProviderView {
                id: id.to_string(),
                name,
                kind,
                verified,
                base_url,
                host,
                adapter,
                key_configured,
                key_set_ms: pg.key_set_ms,
                key_page,
                revoke_page,
                confirmed,
                halt: pg.halt.as_ref().map(HaltView::from),
                caps: pg.caps,
                approved_models: pg.approved_models.iter().cloned().collect(),
                listed_models: pg.listed_models.clone(),
                prices: pg.prices.clone(),
                models,
            }
        })
        .collect()
}

fn period(calls: u64, p: crate::provider_guard::PeriodUse) -> PeriodView {
    PeriodView {
        calls,
        tokens: p.tokens,
        spend_micros: p.micros,
        unpriced_calls: p.unpriced_calls,
    }
}

pub fn usage_summary_of(g: &GuardBook, log: &UsageLog, now: u64) -> UsageSummaryView {
    let providers = all_ids(g)
        .into_iter()
        .map(|id| {
            let (d, m) = log.period_use(id.as_str(), now);
            let (dc, mc) = log.call_counts(id.as_str(), now);
            ProviderUsageView {
                id: id.to_string(),
                today: period(dc, d),
                month: period(mc, m),
                unsettled_calls: log.unsettled(id.as_str()),
                caps: g.get(id.as_str()).caps,
            }
        })
        .collect();
    UsageSummaryView {
        log_intact: log.ready().is_ok(),
        log_problem: match log.ready() {
            Ok(()) => None,
            Err(e) => Some(
                log.broken_reason()
                    .map_or_else(|| e.to_string(), |b| format!("{e}: {b}")),
            ),
        },
        records: log.records(),
        as_of_ms: now,
        providers,
        recent_calls: log.recent(),
        clock_problem: log.check_clock(now).err().map(|e| e.to_string()),
    }
}

fn one(proxy: &ModelProxy, id: &str) -> Result<ProviderView, String> {
    status(proxy)
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| "unknown provider".to_string())
}

fn known_id(proxy: &ModelProxy, id: &str) -> Result<ProviderId, String> {
    let pid = ProviderId::parse(id).ok_or("unknown provider")?;
    if let ProviderId::Custom(c) = &pid {
        if !proxy.with_state(|g, _| g.customs.iter().any(|x| &x.id == c)) {
            return Err("that custom endpoint is not set up".into());
        }
    }
    Ok(pid)
}

/// Apply a guard change and persist it (the change is dropped from the file if it cannot be saved).
fn guard_change(
    proxy: &ModelProxy,
    id: &str,
    f: impl FnOnce(&mut GuardBook) -> Result<(), String>,
) -> Result<ProviderView, String> {
    known_id(proxy, id)?;
    proxy.with_state(|g, _| {
        let was_unreadable = g.unreadable;
        f(g)?;
        // A change that cannot be saved does not take effect: an unreadable guard stays halted.
        proxy
            .save_guard(g)
            .inspect_err(|_| g.unreadable |= was_unreadable)
    })?;
    one(proxy, id)
}

pub fn caps_set(proxy: &ModelProxy, id: &str, caps: Caps) -> Result<ProviderView, String> {
    guard_change(proxy, id, |g| g.set_caps(id, caps))
}

pub fn price_set(
    proxy: &ModelProxy,
    id: &str,
    model: &str,
    price: Option<Price>,
) -> Result<ProviderView, String> {
    guard_change(proxy, id, |g| g.set_price(id, model, price))
}

pub fn halt(proxy: &ModelProxy, id: &str) -> Result<ProviderView, String> {
    guard_change(proxy, id, |g| {
        g.member_halt(id, now_ms());
        Ok(())
    })
}

pub fn resume(proxy: &ModelProxy, id: &str) -> Result<ProviderView, String> {
    guard_change(proxy, id, |g| g.member_resume(id))
}

pub fn key_set(proxy: &ModelProxy, id: &str, key: SecretString) -> Result<KeyTestView, String> {
    let t = proxy
        .set_key(id, key, now_ms())
        .map_err(|e| e.to_string())?;
    Ok(KeyTestView {
        ok: true,
        listed_models: t.models.len(),
    })
}

pub fn key_test(proxy: &ModelProxy, id: &str) -> Result<KeyTestView, String> {
    match proxy.test_key(id) {
        Ok(t) => Ok(KeyTestView {
            ok: true,
            listed_models: t.models.len(),
        }),
        Err(ProxyError::Provider(e)) => Err(e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn custom_add(
    proxy: &ModelProxy,
    label: &str,
    base_url: &str,
    models: Vec<String>,
) -> Result<CustomEndpointView, String> {
    proxy
        .add_custom(label, base_url, models, now_ms())
        .map(|c| CustomEndpointView::from(&c))
        .map_err(|e| e.to_string())
}

pub fn custom_confirm(
    proxy: &ModelProxy,
    id: &str,
    base_url: &str,
) -> Result<CustomEndpointView, String> {
    proxy
        .confirm_custom(id, base_url)
        .map_err(|e| e.to_string())?;
    proxy
        .with_state(|g, _| g.customs.iter().find(|c| c.id == id).cloned())
        .map(|c| CustomEndpointView::from(&c))
        .ok_or_else(|| "that custom endpoint is not set up".into())
}

pub fn usage_log_reset(proxy: &ModelProxy) -> Result<UsageSummaryView, String> {
    let now = now_ms();
    proxy.with_state(|g, log| {
        log.reset_broken(now).map_err(|e| e.to_string())?;
        Ok(usage_summary_of(g, log, now))
    })
}

// ---------------------------------------------------------------------------
// Tauri state and commands
// ---------------------------------------------------------------------------

/// The proxy, opened (and its loopback endpoint started) on first use, off the main thread.
#[derive(Default)]
pub struct ModelProxyState(Mutex<Option<Arc<ModelProxy>>>);

impl ModelProxyState {
    /// The proxy for the app (opening it and starting its endpoint on first use). Blocking: call
    /// from the blocking pool. In-process callers (the sidecar session opener) mint routes on it.
    pub fn get<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
    ) -> Result<Arc<ModelProxy>, String> {
        use tauri::Manager;
        let mut slot = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = slot.as_ref() {
            return Ok(p.clone());
        }
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join(crate::model_proxy::DIR_NAME);
        let proxy = Arc::new(ModelProxy::new(
            dir,
            Arc::new(crate::ai::OsAiKeyring),
            Arc::new(crate::provider_egress::PinnedHttp),
        ));
        crate::model_proxy_http::start(proxy.clone())
            .map_err(|e| format!("the model proxy could not start: {}", e.kind()))?;
        *slot = Some(proxy.clone());
        Ok(proxy)
    }
}

fn with_proxy<R: tauri::Runtime, T: Send + 'static>(
    app: tauri::AppHandle<R>,
    f: impl FnOnce(&ModelProxy) -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let st = app
            .try_state::<ModelProxyState>()
            .ok_or("internal: model proxy state unavailable")?;
        let p = st.get(&app)?;
        f(&p)
    })
}

/// Source: pinned vendor table, keychain presence, sealed guard file.
#[tauri::command]
pub async fn providers_status(app: tauri::AppHandle) -> Result<Vec<ProviderView>, String> {
    with_proxy(app, |p| Ok(status(p))).await
}

/// Source: the signed usage log.
#[tauri::command]
pub async fn providers_usage_summary(app: tauri::AppHandle) -> Result<UsageSummaryView, String> {
    with_proxy(app, |p| {
        Ok(p.with_state(|g, log| usage_summary_of(g, log, now_ms())))
    })
    .await
}

/// Source: the sealed guard file (written).
#[tauri::command]
pub async fn providers_caps_set(
    app: tauri::AppHandle,
    provider_id: String,
    caps: Caps,
) -> Result<ProviderView, String> {
    with_proxy(app, move |p| caps_set(p, &provider_id, caps)).await
}

/// Source: the sealed guard file (written). `price: null` removes the member's price.
#[tauri::command]
pub async fn providers_price_set(
    app: tauri::AppHandle,
    provider_id: String,
    model: String,
    price: Option<Price>,
) -> Result<ProviderView, String> {
    with_proxy(app, move |p| price_set(p, &provider_id, &model, price)).await
}

/// Source: the sealed guard file (written).
#[tauri::command]
pub async fn providers_halt(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<ProviderView, String> {
    with_proxy(app, move |p| halt(p, &provider_id)).await
}

/// Source: the sealed guard file (written).
#[tauri::command]
pub async fn providers_resume(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<ProviderView, String> {
    with_proxy(app, move |p| resume(p, &provider_id)).await
}

/// One way: the key goes in, is format-checked and tested live (vendor `GET /models`), and is
/// stored in the keychain only if the test passes. Nothing returns it.
#[tauri::command]
pub async fn providers_key_set(
    app: tauri::AppHandle,
    provider_id: String,
    api_key: SecretString,
) -> Result<KeyTestView, String> {
    with_proxy(app, move |p| key_set(p, &provider_id, api_key)).await
}

/// Source: the vendor's free endpoint, live, with the stored key.
#[tauri::command]
pub async fn providers_key_test(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<KeyTestView, String> {
    with_proxy(app, move |p| key_test(p, &provider_id)).await
}

/// Source: the keychain (deleted). Also revokes every sidecar route to this provider.
#[tauri::command]
pub async fn providers_key_remove(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<(), String> {
    with_proxy(app, move |p| {
        p.remove_key(&provider_id).map_err(|e| e.to_string())
    })
    .await
}

/// Source: the sealed guard file. Starts unconfirmed (every call refused until confirmed).
#[tauri::command]
pub async fn providers_custom_add(
    app: tauri::AppHandle,
    label: String,
    base_url: String,
    models: Vec<String>,
) -> Result<CustomEndpointView, String> {
    with_proxy(app, move |p| custom_add(p, &label, &base_url, models)).await
}

/// Source: the sealed guard file. The member echoes the exact URL they are confirming.
#[tauri::command]
pub async fn providers_custom_confirm(
    app: tauri::AppHandle,
    id: String,
    base_url: String,
) -> Result<CustomEndpointView, String> {
    with_proxy(app, move |p| custom_confirm(p, &id, &base_url)).await
}

/// Source: the sealed guard file and the keychain (its key is deleted).
#[tauri::command]
pub async fn providers_custom_remove(app: tauri::AppHandle, id: String) -> Result<(), String> {
    with_proxy(app, move |p| {
        p.remove_custom(&id).map_err(|e| e.to_string())
    })
    .await
}

/// Source: the usage log. Only a broken log is reset (archived aside, a new chain started).
#[tauri::command]
pub async fn providers_usage_log_reset(app: tauri::AppHandle) -> Result<UsageSummaryView, String> {
    with_proxy(app, usage_log_reset).await
}

#[cfg(test)]
mod tests {
    include!("providers_ipc_tests.rs");
}
