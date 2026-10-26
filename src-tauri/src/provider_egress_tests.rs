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
    assert_eq!(resolve_vendor(Vendor::Meta).unwrap().host, "api.llama.com");
    for d in DELEGATES {
        assert!(parse_base_url(d.base_url).is_ok(), "{}", d.id);
    }
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
