// D-11 tripwires: no vendor key leaves core. The `ApiKey` type is the enforcement (no Serialize,
// Debug, Clone; adapters never see it); these source scans keep the paths to its bytes, the
// keychain accounts and the transport from multiplying, and pin the diagnostics scrub for every
// vendor's key shape. The older name-based scan (`invoke_secret_scan_tests`) stays as a backstop.
//
// XR-002 lesson: each scan proves it can fail (`negative_control_*`).

use std::path::Path;

/// Production sources of this crate: `(file name, text)`, test files excluded.
fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        out.push((name, without_test_module(&text)));
    }
    assert!(
        out.len() > 100,
        "the scan must see the crate (saw {})",
        out.len()
    );
    out
}

/// Drop everything from the first `#[cfg(test)]` on (each file's test module is at its end).
fn without_test_module(src: &str) -> String {
    match src.find("#[cfg(test)]") {
        Some(i) => src[..i].to_string(),
        None => src.to_string(),
    }
}

/// Files (other than `allowed`) whose code mentions `needle`, comments ignored.
fn offenders(srcs: &[(String, String)], needle: &str, allowed: &[&str]) -> Vec<String> {
    srcs.iter()
        .filter(|(name, _)| !allowed.contains(&name.as_str()))
        .filter(|(_, text)| {
            text.lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .any(|l| l.contains(needle))
        })
        .map(|(name, _)| name.clone())
        .collect()
}

#[test]
fn key_bytes_are_read_in_exactly_two_places() {
    let srcs = sources();
    // The transport's auth header, inside model_provider.rs (private to that module).
    assert_eq!(
        offenders(&srcs, ".expose_for_transport(", &["model_provider.rs"]),
        Vec::<String>::new()
    );
    // The keychain write in ModelProxy::set_key.
    assert_eq!(
        offenders(&srcs, ".expose_for_keychain(", &["model_proxy.rs"]),
        Vec::<String>::new()
    );
    let n = srcs
        .iter()
        .find(|(n, _)| n == "model_proxy.rs")
        .map(|(_, t)| t.matches(".expose_for_keychain(").count())
        .unwrap_or(0);
    assert_eq!(n, 1, "exactly one keychain write of a vendor key");
}

#[test]
fn vendor_key_accounts_are_touched_only_by_the_proxy() {
    let srcs = sources();
    assert_eq!(
        offenders(&srcs, "provider-key:", &["model_proxy.rs"]),
        Vec::<String>::new(),
        "only the model proxy reads or writes vendor keys"
    );
    assert_eq!(
        offenders(&srcs, "model_proxy::KEY_ACCOUNT_PREFIX", &[]),
        Vec::<String>::new()
    );
}

#[test]
fn the_pinned_transport_is_the_only_vendor_transport() {
    let srcs = sources();
    assert_eq!(
        offenders(&srcs, "impl ProviderHttp for", &["provider_egress.rs"]),
        Vec::<String>::new()
    );
    // The provider modules reach the network only through the pinned configuration.
    for f in [
        "model_provider.rs",
        "model_proxy.rs",
        "model_proxy_http.rs",
        "provider_openai_compat.rs",
        "provider_guard.rs",
        "provider_usage_log.rs",
        "providers_ipc.rs",
    ] {
        let text = &srcs.iter().find(|(n, _)| n == f).unwrap().1;
        assert!(
            !text.contains("ureq::"),
            "{f} must not open its own HTTP client"
        );
        assert!(
            !text.contains("reqwest"),
            "{f} must not open its own HTTP client"
        );
    }
}

#[test]
fn negative_control_the_scans_can_fail() {
    let fake = vec![
        (
            "evil.rs".to_string(),
            "fn f(k: &ApiKey) { send(k.expose_for_transport()) }".to_string(),
        ),
        (
            "model_proxy.rs".to_string(),
            "x.expose_for_keychain(); // fine\n".to_string(),
        ),
        (
            "other.rs".to_string(),
            "const A: &str = \"provider-key:openai\";".to_string(),
        ),
        (
            "comment.rs".to_string(),
            "// .expose_for_transport( in a comment".to_string(),
        ),
    ];
    assert_eq!(
        offenders(&fake, ".expose_for_transport(", &["model_provider.rs"]),
        vec!["evil.rs".to_string()]
    );
    assert_eq!(
        offenders(&fake, "provider-key:", &["model_proxy.rs"]),
        vec!["other.rs".to_string()]
    );
    assert_eq!(
        without_test_module("a\n#[cfg(test)]\nmod t { provider-key: }"),
        "a\n"
    );
}

/// Every governed vendor's key shape (built at run time) and a proxy route token.
fn planted() -> Vec<String> {
    let tail = "Z9".repeat(16);
    vec![
        format!("sk-proj-{tail}"),      // OpenAI
        format!("sk-ant-api03-{tail}"), // Anthropic
        format!("AIza{tail}"),          // Gemini
        format!("xai-{tail}"),          // xAI
        format!("sk-{tail}"),           // DeepSeek
        format!("LLM|{tail}"),          // Meta Llama API
        format!("apk_user_{tail}"),     // Cognition (deprecated v1 key)
        format!("cog_{tail}"),          // Cognition (v3 service user / PAT)
        format!("cmp_{tail}"),          // a model-proxy route token
    ]
}

#[test]
fn the_diagnostics_bundle_scrubs_every_vendor_key_shape() {
    for k in planted() {
        let crash =
            format!("panicked at provider call: key={k} status=401\nBearer {k}\n{{\"k\":\"{k}\"}}");
        let b = crate::telemetry::build_bundle(
            "rpt_0a1b2c".into(),
            "0.5.2",
            "macos",
            &crash,
            &format!("node said {k}"),
            &[format!("ui: {k}")],
            "/Users/someone",
        );
        let json = serde_json::to_string(&b).unwrap();
        assert!(
            !json.contains(&k),
            "the diagnostics bundle leaked a key shaped like {:?}",
            &k[..8]
        );
        // And the re-scrub of a bundle the webview sends back.
        let re = crate::telemetry::rescrub_bundle_json(
            &json.replace("<redacted>", &k),
            "/Users/someone",
        )
        .unwrap_or_default();
        assert!(!re.contains(&k), "re-scrub leaked {:?}", &k[..8]);
    }
}

// ---------------------------------------------------------------------------------------------
// D-11 (WP-S1-PROV-AGENT): no key crosses into the Hermes sidecar, and every remote path goes
// through the proxy.
// ---------------------------------------------------------------------------------------------

/// A production source file's code, whole (to its trailing test module) and without comment
/// lines. (`sources()` stops at the first `#[cfg(test)]`, which some files use for test hooks.)
fn code_of(f: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(f);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("{f} not found"));
    let end = text.find("#[cfg(test)]\nmod ").unwrap_or(text.len());
    text[..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_sidecar_session_body_and_proxied_escalation_passes_the_key_tripwire() {
    let hermes = code_of("hermes.rs");
    // Both session-open paths check the body before the sidecar sees it.
    for f in [
        "pub fn session_open(&self",
        "pub fn session_open_store(&self",
    ] {
        let at = hermes.find(f).unwrap_or_else(|| panic!("{f}"));
        let body = &hermes[at..at + 400.min(hermes.len() - at)];
        assert!(
            body.contains("cloud::ensure_keyless(\"session\""),
            "{f} must run the key tripwire first"
        );
    }
    // The spawn environment and arguments are filtered, and vendor key variables are removed.
    assert!(hermes.contains("cloud::ensure_keyless(\"environment\""));
    assert!(hermes.contains("cloud::ensure_keyless(\"arguments\""));
    assert!(hermes.contains("spec.env_remove = cloud::inherited_key_vars("));
    assert!(code_of("escalation.rs").contains("ensure_keyless(\"escalation\""));
}

#[test]
fn sidecar_facing_modules_never_read_a_vendor_key() {
    for f in [
        "hermes.rs",
        "hermes_cloud.rs",
        "cognition.rs",
        "escalation.rs",
    ] {
        let t = code_of(f);
        for needle in [
            "provider-key:",
            "expose_for_",
            "read_key(",
            "model_proxy::KEY_ACCOUNT_PREFIX",
        ] {
            assert!(
                !t.contains(needle),
                "{f} must not touch vendor keys ({needle})"
            );
        }
    }
    // The only key escalation still reads itself is a LOCAL endpoint's (plain http to this
    // computer), inside the `is_local_endpoint` branch.
    let esc = code_of("escalation.rs");
    let reads: Vec<usize> = esc
        .match_indices(".get(&key_account(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(reads.len(), 2, "one local read, one legacy-migration read");
    let local = esc
        .find("if is_local_endpoint(&e.base_url) {")
        .expect("local branch");
    assert!(reads.iter().any(|&i| i > local && i < local + 300));
    assert!(
        esc.contains("proxy\n            .import_key(&pid, SecretString::new(key), false)"),
        "the legacy read only migrates"
    );
}

#[test]
fn remote_chat_and_media_have_no_http_client_of_their_own() {
    let ai = code_of("ai.rs");
    // The AiHttpClient seam is used by the two LOCAL paths only.
    assert_eq!(ai.matches("self.http.post_json(").count(), 2);
    for f in ["fn chat_local(", "fn chat_local_tools("] {
        let at = ai.find(f).unwrap();
        assert!(ai[at..].find("self.http.post_json(").unwrap() < 1500, "{f}");
    }
    // Remote sends are proxy calls.
    assert!(ai.contains("proxy\n        .call(route, req, None, &mut |_| {})"));
    assert!(ai.contains(".post_fixed(&route.provider"));
    // No preset blob is ever written with a key again.
    assert!(ai.contains("#[serde(rename = \"apiKey\", default, skip_serializing)]"));
}
