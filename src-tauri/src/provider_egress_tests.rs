// D-11 — egress: the pinned vendor table, endpoint resolution, and the refusal cases (off-host,
// redirect, unconfirmed custom endpoint, proxy environment).

use super::*;

#[test]
fn every_vendor_has_exactly_one_pinned_spec() {
    for v in Vendor::ALL {
        let n = VENDORS.iter().filter(|s| s.vendor == v).count();
        assert_eq!(n, 1, "{v:?} must have exactly one pinned spec");
        let ep = resolve_vendor(v).unwrap();
        assert!(ep.verified);
        assert_eq!(ep.base_url.scheme(), "https");
        assert_eq!(ep.port, 443);
        assert_eq!(ep.provider, ProviderId::Vendor(v));
        for page in [vendor_spec(v).key_page, vendor_spec(v).revoke_page] {
            assert!(page.starts_with("https://"), "{page}");
        }
    }
    assert_eq!(resolve_vendor(Vendor::OpenAi).unwrap().host, "api.openai.com");
    assert_eq!(resolve_vendor(Vendor::Anthropic).unwrap().host, "api.anthropic.com");
    assert_eq!(
        resolve_vendor(Vendor::Gemini).unwrap().host,
        "generativelanguage.googleapis.com"
    );
    assert_eq!(resolve_vendor(Vendor::Xai).unwrap().host, "api.x.ai");
    assert_eq!(resolve_vendor(Vendor::DeepSeek).unwrap().host, "api.deepseek.com");
    assert_eq!(
        resolve_vendor(Vendor::DeepSeek).unwrap().base_url.as_str(),
        "https://api.deepseek.com/",
        "the bare documented host"
    );
    assert_eq!(resolve_vendor(Vendor::Meta).unwrap().host, "api.meta.ai");
    assert_eq!(resolve_vendor(Vendor::Meta).unwrap().base_url.path(), "/v1");
    for d in DELEGATES {
        assert!(parse_base_url(d.base_url).is_ok(), "{}", d.id);
    }
    assert_eq!(DELEGATES[0].base_url, "https://api.devin.ai/v3");
}

#[test]
fn base_urls_must_be_plain_https() {
    for bad in [
        "http://api.example.com/v1",
        "https://user:pw@api.example.com/v1",
        "https://api.example.com/v1?x=1",
        "https://api.example.com/v1#f",
        "ftp://api.example.com",
        "https://",
        "https://api.example.com/v 1",
    ] {
        assert!(parse_base_url(bad).is_err(), "{bad} must be refused");
    }
    assert!(parse_base_url("https://llm.example.org:8443/v1").is_ok());
}

fn ep(v: Vendor) -> ResolvedEndpoint {
    resolve_vendor(v).unwrap()
}

#[test]
fn an_adapter_path_can_never_leave_the_pinned_host() {
    let e = ep(Vendor::OpenAi);
    assert_eq!(
        join_checked(&e, "/responses").unwrap().as_str(),
        "https://api.openai.com/v1/responses"
    );
    let g = ep(Vendor::Gemini);
    assert_eq!(
        join_checked(&g, "/models/gemini-x:streamGenerateContent?alt=sse")
            .unwrap()
            .as_str(),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-x:streamGenerateContent?alt=sse"
    );
    for bad in [
        "//evil.example/v1/chat",
        "https://evil.example/v1",
        "@evil.example/x",
        ".evil.example/x",
        "/../../x",
        "/a/../../b",
        "/x#frag",
        "/x\\y",
        "chat/completions",
        "",
        "/ space",
        "/x\r\nHost: evil.example",
    ] {
        assert!(
            join_checked(&e, bad).is_err(),
            "{bad:?} must be refused (joined: {:?})",
            join_checked(&e, bad).map(|u| u.to_string())
        );
    }
}

#[test]
fn adapters_may_not_set_auth_host_cookie_or_proxy_headers() {
    for bad in [
        "authorization",
        "Authorization",
        "x-api-key",
        "x-goog-api-key",
        "host",
        "cookie",
        "proxy-authorization",
        "content-length",
        "transfer-encoding",
        "connection",
        "x-forwarded-host",
    ] {
        assert!(check_extra_header(bad, "v").is_err(), "{bad} must be refused");
    }
    assert!(check_extra_header("anthropic-version", "2023-06-01").is_ok());
    assert!(check_extra_header("accept", "text/event-stream").is_ok());
    assert!(check_extra_header("anthropic-version", "a\r\nx: y").is_err(), "no header injection");
    // The key goes only in the vendors' own auth headers.
    assert!(check_auth_header_name("x-api-key").is_ok());
    assert!(check_auth_header_name("x-goog-api-key").is_ok());
    assert!(check_auth_header_name("x-forwarded-key").is_err());
    assert!(check_auth_header_name("cookie").is_err());
}

#[test]
fn the_transport_follows_no_redirect_and_ignores_proxy_environment() {
    let c = pinned_config();
    assert_eq!(c.max_redirects(), 0, "redirects are never followed");
    assert!(!c.max_redirects_will_error(), "a 3xx comes back as an answer, mapped to Redirect");
    assert!(c.proxy().is_none(), "HTTPS_PROXY / ALL_PROXY must not route vendor calls");
    assert!(c.https_only(), "https only");
    assert_eq!(
        crate::model_provider::status_error(307),
        crate::model_provider::ProviderError::Redirect
    );
}

#[test]
fn a_custom_endpoint_resolves_unverified_and_on_its_own_host_only() {
    let c = CustomEndpoint {
        id: "custom-box".into(),
        label: "Box".into(),
        base_url: "https://llm.example.org:8443/v1".into(),
        models: vec!["m".into()],
        confirmed: true,
        created_ms: 1,
    };
    let e = resolve_custom(&c).unwrap();
    assert!(!e.verified);
    assert_eq!((e.host.as_str(), e.port), ("llm.example.org", 8443));
    assert!(join_checked(&e, "/chat/completions").is_ok());
    assert!(join_checked(&e, "/models").is_ok());
    for bad in ["/responses", "/messages", "/files", "/chat/completions/x"] {
        assert!(join_checked(&e, bad).is_err(), "a custom endpoint refuses {bad}");
    }
    let mut bad = c.clone();
    bad.id = "openai".into();
    assert!(resolve_custom(&bad).is_err(), "a custom record cannot claim a vendor id");
    let mut http = c;
    http.base_url = "http://llm.example.org/v1".into();
    assert!(resolve_custom(&http).is_err());
}

#[test]
fn custom_endpoints_refuse_ip_literals_local_names_and_trailing_dots() {
    for bad in [
        "https://169.254.169.254/v1",
        "https://127.0.0.1/v1",
        "https://10.0.0.5/v1",
        "https://[::1]/v1",
        "https://[fe80::1]/v1",
        "https://localhost/v1",
        "https://box.localhost/v1",
        "https://llm.example.org./v1",
        "https://printer.local/v1",
    ] {
        assert!(check_custom_host(bad).is_err(), "{bad} must be refused for a custom endpoint");
    }
    assert!(check_custom_host("https://llm.example.org/v1").is_ok());
}

/// Tripwire: every vendor answers exactly its documented endpoint paths and nothing else, so
/// the check holds for a bare-host base (DeepSeek) as much as for one with a path.
#[test]
fn every_vendor_answers_only_its_exact_endpoint_paths() {
    let all: Vec<&str> = VENDORS.iter().flat_map(|s| s.paths.iter().copied()).collect();
    for spec in VENDORS {
        let e = ep(spec.vendor);
        assert!(!spec.paths.is_empty(), "{:?}", spec.vendor);
        for p in spec.paths {
            let concrete = p.replace("{model}", "m-1.5_x");
            let url = join_checked(&e, &concrete);
            assert!(url.is_ok(), "{:?} {concrete}", spec.vendor);
            let url = url.map(|u| u.to_string()).unwrap_or_default();
            assert_eq!(
                url,
                format!("{}{concrete}", spec.base_url.trim_end_matches('/')),
                "{:?}",
                spec.vendor
            );
        }
        let mut bad: Vec<String> = [
            "/", "/files", "/v1/files", "/v1/chat/completions", "/batches", "/models/m",
            "/models/../files", "/chat/completions/x", "/models?key=x", "/responses/r1",
            "/messages?beta=1", "/models/a/b:generateContent", "/models/.x:generateContent",
            "/models/m:generateContent?key=1", "/models/m:streamGenerateContent",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        // Another vendor's endpoints are not this vendor's.
        bad.extend(
            all.iter()
                .filter(|p| !spec.paths.contains(p))
                .map(|p| p.replace("{model}", "m")),
        );
        for b in bad {
            if spec.paths.iter().any(|p| p.replace("{model}", "m") == b) {
                continue;
            }
            assert!(join_checked(&e, &b).is_err(), "{:?} must refuse {b}", spec.vendor);
        }
    }
    let ds = ep(Vendor::DeepSeek);
    assert_eq!(
        join_checked(&ds, "/chat/completions").map(|u| u.to_string()),
        Ok("https://api.deepseek.com/chat/completions".to_string())
    );
}
