// D-11 (P-5, P-6) — the Hermes agent loop on a cloud model through the core proxy: the session
// body carries a loopback route token and never a key; Local model only refuses the open and each
// later model request; routes are revoked on close and stop; and the tripwire refuses anything
// shaped like a vendor key in a sidecar body, environment value or argument.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use crate::model_proxy::tests::{req, Rig};
use crate::provider_fixtures_tests::fake_key;
use std::sync::{Arc, Mutex};

/// A sidecar control transport that records every POST body and answers `POST /sessions`.
#[derive(Default, Clone)]
struct Recorder {
    posts: Arc<Mutex<Vec<(String, String)>>>,
    fail_open: bool,
    /// The session id the sidecar answers (unique per test: the cloud-session map is global).
    sid: String,
}

fn recorder(sid: &str) -> Recorder {
    Recorder { sid: sid.to_string(), ..Recorder::default() }
}

impl HermesControl for Recorder {
    fn get(&self, _url: &str, _bearer: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp { status: 404, body: String::new() })
    }
    fn post(&self, url: &str, _bearer: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.posts.lock().unwrap().push((url.to_string(), body.to_string()));
        if url.ends_with("/sessions") {
            if self.fail_open {
                return Ok(ControlResp { status: 503, body: "{\"error\":\"busy\"}".into() });
            }
            return Ok(ControlResp { status: 201, body: format!("{{\"id\":\"{}\"}}", self.sid) });
        }
        Ok(ControlResp { status: 200, body: "{}".into() })
    }
}

fn manager(rec: &Recorder) -> HermesManager {
    let d = std::env::temp_dir().join(format!("citrate-hcloud-{}-{:x}", std::process::id(), rand::random::<u32>()));
    let m = HermesManager::new(d.join("bin"), d.join("t"), d.join("c")).with_control(Box::new(rec.clone()));
    m.set_token_for_test("control-bearer");
    m
}

fn open(rig: &Rig, m: &HermesManager, provider: &str, model: &str) -> Result<String, String> {
    open_cloud_session(&rig.proxy, m, provider, model, rig.gate(), "You are Hermes.", "[]", |b| {
        m.session_open(b).map_err(|e| e.to_string())
    })
}

#[test]
fn a_cloud_session_gets_a_loopback_route_token_and_never_the_vendor_key() {
    let rig = Rig::new("hcloud-body");
    let rec = recorder("s-cloud-1");
    let m = manager(&rec);
    let sid = open(&rig, &m, "openai", "gpt-known").expect("opened");
    assert_eq!(sid, "s-cloud-1");
    let posts = rec.posts.lock().unwrap().clone();
    let (_, body) = posts.iter().find(|(u, _)| u.ends_with("/sessions")).unwrap();
    assert!(!body.contains(&rig.key), "the vendor key never crosses into the sidecar");
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(v["llm"]["baseUrl"], "http://127.0.0.1:1/v1");
    assert_eq!(v["llm"]["proxied"], true);
    assert_eq!(v["model"], "gpt-known");
    assert_eq!(v["hicAware"], true, "the same HIC-aware session as a local one");
    assert_eq!(v["contextTokens"], CLOUD_CONTEXT_TOKENS);
    // The bearer is a live route for exactly this provider and model.
    let bearer = v["llm"]["bearer"].as_str().unwrap();
    assert!(bearer.starts_with("cmp_"));
    let route = rig.proxy.route_for(bearer).expect("live route");
    assert_eq!(route.provider.as_str(), "openai");
    assert_eq!(route.model, "gpt-known");
    assert!(route.session_id.starts_with(CLOUD_ROUTE_PREFIX));
    assert!(m.cloud.is_cloud("s-cloud-1"));
    // A model request on that route reaches the vendor through the proxy.
    rig.proxy.call_routed(bearer, req("gpt-known", false), None, &mut |_| {}).expect("call");
    // Closing the session revokes the route.
    on_close(&m, Some(&rig.proxy), "s-cloud-1");
    assert!(rig.proxy.route_for(bearer).is_none());
    assert!(!m.cloud.is_cloud("s-cloud-1"));
}

#[test]
fn local_model_only_refuses_the_open_and_every_later_request_of_a_running_session() {
    let rig = Rig::new("hcloud-local-only");
    let rec = recorder("s-cloud-2");
    let m = manager(&rec);
    crate::chat_route::set(&rig.gate(), crate::chat_route::ChatRoute::LocalOnly, 1).unwrap();
    let err = open(&rig, &m, "openai", "gpt-known").unwrap_err();
    assert!(err.contains("Local only is on"), "{err}");
    assert!(rec.posts.lock().unwrap().is_empty(), "nothing reached the sidecar");
    // Opened while allowed, then the member turns Local model only on mid-session.
    crate::chat_route::set(&rig.gate(), crate::chat_route::ChatRoute::AskFirst, 2).unwrap();
    open(&rig, &m, "openai", "gpt-known").expect("opened");
    let body = rec.posts.lock().unwrap().last().unwrap().1.clone();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bearer = v["llm"]["bearer"].as_str().unwrap().to_string();
    crate::chat_route::set(&rig.gate(), crate::chat_route::ChatRoute::LocalOnly, 3).unwrap();
    let before = rig.vendor.calls();
    let r = rig.proxy.call_routed(&bearer, req("gpt-known", false), None, &mut |_| {});
    assert!(matches!(&r, Err(crate::model_proxy::ProxyError::Egress(m)) if m.contains("Local only")), "{r:?}");
    assert_eq!(rig.vendor.calls(), before, "nothing sent");
    assert!(check_send(&m, "s-cloud-2", &rig.gate()).is_err(), "no new turn starts either");
    on_close(&m, Some(&rig.proxy), "s-cloud-2");
}

#[test]
fn an_unconnected_provider_or_the_delegate_is_refused_and_a_failed_open_revokes_its_route() {
    let rig = Rig::new("hcloud-refuse");
    let rec = recorder("s-cloud-3");
    let m = manager(&rec);
    let err = open(&rig, &m, "xai", "grok-x").unwrap_err();
    assert!(err.contains("not connected"), "{err}");
    let err = open(&rig, &m, "cognition", "devin").unwrap_err();
    assert!(err.contains("Cognition"), "{err}");
    assert!(open(&rig, &m, "nope", "m").is_err());
    assert!(rec.posts.lock().unwrap().is_empty());
    // The sidecar refuses the open: the route minted for it is revoked.
    let failing = Recorder { fail_open: true, ..recorder("s-cloud-3-f") };
    let m2 = manager(&failing);
    assert!(open(&rig, &m2, "openai", "gpt-known").is_err());
    let body = failing.posts.lock().unwrap().last().unwrap().1.clone();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(rig.proxy.route_for(v["llm"]["bearer"].as_str().unwrap()).is_none());
}

#[test]
fn the_members_model_choice_is_approved_so_it_does_not_trip_the_unknown_model_halt() {
    let rig = Rig::new("hcloud-approve");
    let rec = recorder("s-cloud-4");
    let m = manager(&rec);
    open(&rig, &m, "openai", "gpt-chosen").expect("opened");
    let body = rec.posts.lock().unwrap().last().unwrap().1.clone();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bearer = v["llm"]["bearer"].as_str().unwrap().to_string();
    rig.proxy
        .call_routed(&bearer, req("gpt-chosen", false), None, &mut |_| {})
        .expect("an approved model is not halted");
    on_close(&m, Some(&rig.proxy), "s-cloud-4");
}

#[test]
fn stopping_the_sidecar_revokes_every_cloud_route() {
    let rig = Rig::new("hcloud-stop");
    let rec = recorder("s-cloud-5");
    let m = manager(&rec);
    open(&rig, &m, "openai", "gpt-known").expect("opened");
    let body = rec.posts.lock().unwrap().last().unwrap().1.clone();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let bearer = v["llm"]["bearer"].as_str().unwrap().to_string();
    // A non-cloud route survives.
    let other = rig.proxy.mint_route(rig.route("gpt-known"), rig.gate()).unwrap();
    m.stop();
    on_stop(Some(&rig.proxy));
    assert!(rig.proxy.route_for(&bearer).is_none());
    assert!(rig.proxy.route_for(other.bearer.expose()).is_some());
    assert!(!m.cloud.is_cloud("s-cloud-5"));
}

// ---- the tripwire ----------------------------------------------------------------------------

#[test]
fn the_tripwire_refuses_every_vendor_key_shape_and_passes_route_tokens_and_ordinary_text() {
    for prefix in ["sk-", "sk-ant-", "xai-", "AIza", "LLM|", "cog_", "apk_", "cgk_"] {
        let k = fake_key(prefix);
        assert!(ensure_keyless("session", &format!("{{\"x\":\"{k}\"}}")).is_err(), "{prefix}");
        assert!(ensure_keyless("environment", &k).is_err(), "{prefix}");
    }
    assert!(ensure_keyless("session", &format!("cmp_{}", "ab".repeat(32))).is_ok());
    for ok in [
        "run the task-runner-for-every-repository-in-the-org",
        "risk-assessment-of-the-deployment-plan",
        "/Users/me/Library/Application Support/ai.citrate.core/hermes/token",
        &"0f".repeat(32),
    ] {
        assert!(ensure_keyless("session", ok).is_ok(), "{ok}");
    }
}

#[test]
fn a_session_body_carrying_a_key_shape_is_refused_before_the_sidecar_sees_it() {
    let rec = recorder("s-cloud-7");
    let m = manager(&rec);
    let body = format!("{{\"llm\":{{\"baseUrl\":\"https://api.openai.com/v1\",\"bearer\":\"{}\"}}}}", fake_key("sk-"));
    let err = m.session_open(&body).unwrap_err();
    assert!(err.to_string().contains("shaped like an API key"), "{err}");
    let err = m.session_open_store(&body, &[]).unwrap_err();
    assert!(err.to_string().contains("shaped like an API key"), "{err}");
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn inherited_vendor_key_variables_and_key_shaped_values_are_stripped_from_the_sidecar() {
    let inherited = vec![
        ("OPENAI_API_KEY".to_string(), "anything".to_string()),
        ("anthropic_api_key".to_string(), "x".to_string()),
        ("MY_TOKEN".to_string(), fake_key("xai-")),
        ("HOME".to_string(), "/Users/me".to_string()),
        ("CITRATE_HERMES_SEARCH".to_string(), "1".to_string()),
    ];
    let mut out = inherited_key_vars(inherited.into_iter());
    out.sort();
    assert_eq!(out, vec!["MY_TOKEN".to_string(), "OPENAI_API_KEY".to_string(), "anthropic_api_key".to_string()]);
}

#[test]
fn the_sidecar_spec_carries_no_key_shaped_environment_value() {
    let src: crate::hermes_web::EnvSource = Arc::new(|| {
        vec![
            ("CITRATE_HERMES_SEARCH".to_string(), "1".to_string()),
            ("CITRATE_HERMES_SEARXNG".to_string(), fake_key("sk-")),
        ]
    });
    let rec = Recorder::default();
    let m = manager(&rec).with_env_source(src);
    let env = m.spec_env_for_test();
    assert!(env.iter().any(|(k, v)| k == "CITRATE_HERMES_SEARCH" && v == "1"));
    assert!(!env.iter().any(|(k, _)| k == "CITRATE_HERMES_SEARXNG"), "{env:?}");
    assert!(env.iter().all(|(_, v)| ensure_keyless("environment", v).is_ok()));
}
