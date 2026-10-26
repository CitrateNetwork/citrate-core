// D-11 — the provider trait's interface: ids, the typed key, the SSE reader, and `ProviderCtx::send`
// (the only way an adapter reaches the network).

use super::*;
use std::sync::Mutex;

/// Compile-time "does NOT implement" check (the static_assertions trick): if `$t` implemented
/// `$tr`, both blanket impls would apply and `check()` would be ambiguous, so this would not build.
macro_rules! assert_not_impl {
    ($t:ty, $($tr:path),+) => {{
        $(
            {
                trait AmbiguousIfImpl<A> {
                    fn check() {}
                }
                impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
                #[allow(dead_code)]
                struct Invalid;
                impl<T: ?Sized + $tr> AmbiguousIfImpl<Invalid> for T {}
                let _ = <$t as AmbiguousIfImpl<_>>::check;
            }
        )+
    }};
}

#[test]
fn api_key_has_no_leaky_traits() {
    // P-9b: the type is the enforcement. A struct holding an ApiKey cannot derive any of these.
    assert_not_impl!(
        ApiKey,
        serde::Serialize,
        std::fmt::Debug,
        std::fmt::Display,
        Clone
    );
    assert_not_impl!(ApiKey, serde::de::DeserializeOwned);
    fn wipes<T: zeroize::ZeroizeOnDrop>() {}
    wipes::<crate::secret::SecretString>();
}

#[test]
fn api_key_shape_helpers_do_not_expose_the_key() {
    let k = ApiKey::from_secret(SecretString::new("  sk-test-0123456789abcdefABCDEF  ".into()));
    assert_eq!(k.len(), "sk-test-0123456789abcdefABCDEF".len(), "entry is trimmed");
    assert!(k.starts_with("sk-test-"));
    assert!(k.appears_in(b"xx sk-test-0123456789abcdefABCDEF yy"));
    assert!(!k.appears_in(b"sk-test-0123"));
    assert!(basic_key_shape(&k).is_ok());
    let short = ApiKey::from_secret(SecretString::new("abc".into()));
    assert!(matches!(basic_key_shape(&short), Err(ProviderError::KeyFormat(_))));
    let spaced = ApiKey::from_secret(SecretString::new("sk-abc def 0123456789abcdef".into()));
    assert!(basic_key_shape(&spaced).is_err());
    let empty = ApiKey::from_secret(SecretString::new(String::new()));
    assert!(!empty.appears_in(b"anything"), "an empty key matches nothing");
}

#[test]
fn provider_ids_round_trip_and_custom_ids_are_slugs() {
    for v in Vendor::ALL {
        assert_eq!(Vendor::from_id(v.id()), Some(v));
        assert_eq!(ProviderId::parse(v.id()), Some(ProviderId::Vendor(v)));
    }
    assert_eq!(
        ProviderId::parse("custom-my-box"),
        Some(ProviderId::Custom("custom-my-box".into()))
    );
    // D-11: the Citrate gateway and the Cognition delegate are provider ids too.
    assert_eq!(ProviderId::parse("gateway"), Some(ProviderId::Gateway));
    assert_eq!(ProviderId::parse("cognition"), Some(ProviderId::Cognition));
    assert_eq!(ProviderId::Gateway.as_str(), "gateway");
    assert_eq!(ProviderId::Cognition.as_str(), "cognition");
    assert!(ProviderId::Cognition.is_delegate() && !ProviderId::Gateway.is_delegate());
    for bad in ["", "custom-", "custom-UP", "custom-a/b", "custom-a.b", "Gateway", "devin", "OpenAI"] {
        assert!(ProviderId::parse(bad).is_none(), "{bad:?} must not parse");
    }
    assert!(ProviderId::parse(&format!("custom-{}", "a".repeat(41))).is_none());
}

#[test]
fn read_sse_yields_data_lines_with_their_event_name() {
    let raw = "event: message_start\ndata: {\"a\":1}\n\n: comment\ndata: two\r\n\nevent: x\ndata:three\n";
    let mut seen = Vec::new();
    read_sse(raw.as_bytes(), &mut |e, d| {
        seen.push((e.to_string(), d.to_string()));
        Ok(true)
    })
    .unwrap();
    assert_eq!(
        seen,
        vec![
            ("message_start".into(), "{\"a\":1}".into()),
            (String::new(), "two".into()),
            ("x".into(), "three".into()),
        ]
    );
    // Stop early.
    let mut n = 0;
    read_sse("data: 1\ndata: 2\n".as_bytes(), &mut |_, _| {
        n += 1;
        Ok(false)
    })
    .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn status_errors_are_coarse() {
    assert_eq!(status_error(302), ProviderError::Redirect);
    assert_eq!(status_error(401), ProviderError::Unauthorized);
    assert_eq!(status_error(403), ProviderError::Unauthorized);
    assert_eq!(status_error(429), ProviderError::RateLimited);
    assert_eq!(status_error(500), ProviderError::Status(500));
}

/// A transport that records what it was asked to send (with the key header) and answers `200 {}`.
/// `(url, extra headers, "auth-header: value", body)` of one send.
pub(crate) type Sent = (String, Vec<(String, String)>, String, Option<Vec<u8>>);

pub(crate) struct RecordingHttp {
    pub sent: Mutex<Vec<Sent>>,
}

impl RecordingHttp {
    pub(crate) fn new() -> Self {
        RecordingHttp {
            sent: Mutex::new(Vec::new()),
        }
    }
}

impl ProviderHttp for RecordingHttp {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        self.sent.lock().unwrap().push((
            req.url.to_string(),
            req.headers.clone(),
            format!("{}: {}", req.auth_header.0, req.auth_header.1.as_str()),
            req.body.clone(),
        ));
        Ok(InboundResponse {
            status: 200,
            content_type: "application/json".into(),
            body: Box::new(std::io::Cursor::new(b"{}".to_vec())),
        })
    }
}

#[test]
fn ctx_send_attaches_the_key_only_in_its_auth_header() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::Anthropic).unwrap();
    let key = ApiKey::from_secret(SecretString::new("sk-ant-test-0123456789abcdef".into()));
    let http = RecordingHttp::new();
    let ctx = ProviderCtx::new(&ep, &key, &http);
    ctx.send(OutboundRequest {
        method: Method::Post,
        path: "/messages".into(),
        headers: vec![("anthropic-version", "2023-06-01".into())],
        auth: AuthPlacement::Header("x-api-key"),
        body: Some(serde_json::json!({"model": "m"})),
    })
    .unwrap();
    let sent = http.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "https://api.anthropic.com/v1/messages");
    assert_eq!(sent[0].2, "x-api-key: sk-ant-test-0123456789abcdef");
    assert!(!sent[0].0.contains("sk-ant"));
    assert!(!String::from_utf8_lossy(sent[0].3.as_deref().unwrap()).contains("sk-ant"));
}

#[test]
fn ctx_send_refuses_a_key_in_the_url_body_or_extra_headers() {
    let ep = crate::provider_egress::resolve_vendor(Vendor::Gemini).unwrap();
    // Built at run time: test keys are never literals (secret scanners, and no real key shape).
    let fake = format!("AIza{}", "T".repeat(24));
    let key = ApiKey::from_secret(SecretString::new(fake.clone()));
    let http = RecordingHttp::new();
    let ctx = ProviderCtx::new(&ep, &key, &http);
    let in_query = ctx.send(OutboundRequest {
        method: Method::Get,
        path: format!("/models?key={fake}"),
        headers: vec![],
        auth: AuthPlacement::Header("x-goog-api-key"),
        body: None,
    });
    assert!(matches!(in_query, Err(ProviderError::Egress(_))), "{in_query:?}");
    let in_body = ctx.send(OutboundRequest {
        method: Method::Post,
        path: "/models".into(),
        headers: vec![],
        auth: AuthPlacement::Header("x-goog-api-key"),
        body: Some(serde_json::json!({ "k": fake })),
    });
    assert!(matches!(in_body, Err(ProviderError::Egress(_))));
    let in_header = ctx.send(OutboundRequest {
        method: Method::Get,
        path: "/models".into(),
        headers: vec![("x-extra", fake.clone())],
        auth: AuthPlacement::Header("x-goog-api-key"),
        body: None,
    });
    assert!(matches!(in_header, Err(ProviderError::Egress(_))));
    assert!(http.sent.lock().unwrap().is_empty(), "nothing was sent");
}
