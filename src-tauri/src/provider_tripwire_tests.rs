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

/// The `pub` items of `impl ApiKey` in model_provider.rs (any visibility qualifier).
fn api_key_public_fns(src: &str) -> Vec<String> {
    let Some(start) = src.find("impl ApiKey {") else {
        return vec!["<impl ApiKey not found>".into()];
    };
    let body = &src[start..];
    let end = body.find("\n}\n").unwrap_or(body.len());
    body[..end]
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("pub"))
        .filter_map(|l| l.split("fn ").nth(1))
        .map(|rest| rest.split('(').next().unwrap_or("").to_string())
        .collect()
}

#[test]
fn api_key_has_no_public_accessor_beyond_construction_and_the_keychain_write() {
    // Holding `&ApiKey` must not let code read it piecewise (prefix, length, byte predicates): every
    // accessor is private to model_provider.rs, and vendor formats are data (`KeyFormat`) that core
    // evaluates. Adapters get no `&ApiKey` at all.
    let srcs = sources();
    let mp = &srcs
        .iter()
        .find(|(n, _)| n == "model_provider.rs")
        .unwrap()
        .1;
    let mut public = api_key_public_fns(mp);
    public.sort();
    assert_eq!(
        public,
        vec!["expose_for_keychain".to_string(), "from_secret".to_string()]
    );
    assert!(
        !mp.contains("fn check_key_format(&self, key: &ApiKey)"),
        "adapters must not receive &ApiKey for a format check"
    );
}

#[test]
fn negative_control_a_public_key_accessor_is_flagged() {
    let src = "impl ApiKey {\n    pub(crate) fn from_secret(s: S) -> ApiKey {}\n    pub fn starts_with(&self, p: &str) -> bool {}\n    fn len(&self) -> usize {}\n}\n";
    let mut got = api_key_public_fns(src);
    got.sort();
    assert_eq!(
        got,
        vec!["from_secret".to_string(), "starts_with".to_string()]
    );
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
        "provider_openai.rs",
        "provider_anthropic.rs",
        "provider_gemini.rs",
        "provider_chat_vendors.rs",
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
        format!("sk-proj-{tail}"),              // OpenAI
        format!("sk-ant-api03-{tail}"),         // Anthropic
        format!("AIza{tail}"),                  // Gemini
        format!("xai-{tail}"),                  // xAI
        format!("sk-{tail}"),                   // DeepSeek
        format!("LLM|{tail}"),                  // Meta Llama API (retired)
        format!("LLM_1607358788850350_{tail}"), // Meta Model API (`LLM_<id>_<secret>`)
        format!("apk_user_{tail}"),             // Cognition (legacy)
        format!("cog_{tail}"),                  // Cognition API v3
        format!("cmp_{tail}"),                  // a model-proxy route token
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

/// A line without its `//` comment (whole-line or trailing). A `//` inside a string literal (such
/// as `"https://…"`) is not a comment.
fn code_part(l: &str) -> &str {
    let b = l.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 1,
            b'"' => in_str = !in_str,
            b'/' if !in_str && b.get(i + 1) == Some(&b'/') => return &l[..i],
            _ => {}
        }
        i += 1;
    }
    l
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `(file, where)` for every mention of the identifier `name` in code (comments ignored): `def` for
/// its own `fn name` definition, else the enclosing fn (the last `fn <x>` before it). A call, an
/// alias (`use … as x`) and a bare function pointer are all mentions.
fn mentions(srcs: &[(String, String)], name: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (file, text) in srcs {
        let code: String = text.lines().map(code_part).collect::<Vec<_>>().join("\n");
        let mut from = 0;
        while let Some(i) = code[from..].find(name) {
            let at = from + i;
            from = at + name.len();
            let before = &code[..at];
            let after = code[at + name.len()..].chars().next();
            if before.chars().next_back().is_some_and(is_ident) || after.is_some_and(is_ident) {
                continue;
            }
            let place = if before.ends_with("fn ") {
                "def".to_string()
            } else {
                before
                    .rfind("fn ")
                    .map(|f| {
                        before[f + 3..]
                            .chars()
                            .take_while(|c| is_ident(*c))
                            .collect::<String>()
                    })
                    .unwrap_or_default()
            };
            out.push((file.clone(), place));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Files whose code contains `needle` anywhere (comments ignored).
fn files_naming(srcs: &[(String, String)], needle: &str) -> Vec<String> {
    srcs.iter()
        .filter(|(_, t)| t.lines().any(|l| code_part(l).contains(needle)))
        .map(|(f, _)| f.clone())
        .collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut p: Vec<_> = v
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    p.sort();
    p
}

const PUL: &str = "provider_usage_log.rs";

#[test]
fn only_a_fresh_install_and_a_log_reset_create_the_anchor() {
    let srcs = sources();
    // A new anchor comes from exactly two places: a fresh install opening the proxy
    // (`fresh_install_anchor`, which writes only when none exists) and the member's log reset.
    // `update_anchor` writes only over an anchor that exists. Any other mention (a call, an alias,
    // a function pointer) is flagged.
    assert_eq!(
        mentions(&srcs, "fresh_install_anchor"),
        pairs(&[(PUL, "def"), ("model_proxy.rs", "new")])
    );
    assert_eq!(
        mentions(&srcs, "create_anchor"),
        pairs(&[(PUL, "def"), (PUL, "fresh_install_anchor")])
    );
    assert_eq!(
        mentions(&srcs, "write_anchor"),
        pairs(&[
            (PUL, "def"),
            (PUL, "create_anchor"),
            (PUL, "reset_broken"),
            (PUL, "update_anchor"),
        ])
    );
    // The keychain account, by constant or by its literal name, only in provider_usage_log.rs.
    assert_eq!(files_naming(&srcs, "ANCHOR_ACCOUNT"), vec![PUL.to_string()]);
    assert_eq!(
        files_naming(&srcs, "provider-anchor-v1"),
        vec![PUL.to_string()]
    );
}

#[test]
fn negative_control_an_extra_anchor_creator_is_flagged() {
    let src = "fn create_anchor(k: &K) { write_anchor(k, &A) }\n\
               fn sneaky() { // write_anchor(k, &a) in a comment\n write_anchor(k, &a); }\n\
               fn pointer() { let f = write_anchor; }\n\
               fn url() { let u = \"https://x\"; write_anchor(k, &a); }\n";
    let got = mentions(&[(PUL.into(), src.into())], "write_anchor");
    assert_eq!(
        got,
        pairs(&[
            (PUL, "create_anchor"),
            (PUL, "pointer"),
            (PUL, "sneaky"),
            (PUL, "url")
        ]),
        "calls, a pointer and a call after a URL string are all seen; the comment is not"
    );
    let alias = "use crate::provider_usage_log::create_anchor as x;\nfn save() { x(k); }\n";
    assert_eq!(
        mentions(
            &[("provider_guard.rs".into(), alias.into())],
            "create_anchor"
        ),
        pairs(&[("provider_guard.rs", "")])
    );
    let literal = "fn f(k: &K) { k.set(\"provider-anchor-v1\", \"{}\"); }\n";
    assert_eq!(
        files_naming(
            &[("provider_guard.rs".into(), literal.into())],
            "provider-anchor-v1"
        ),
        vec!["provider_guard.rs".to_string()]
    );
    assert_eq!(
        code_part("let u = \"https://x\"; // c"),
        "let u = \"https://x\"; "
    );
}
