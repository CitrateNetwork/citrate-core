use super::*;

fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn strs(urls: &[url::Url]) -> Vec<String> {
    urls.iter().map(|u| u.as_str().to_string()).collect()
}

// --- argv -> URL extraction -------------------------------------------------------------------

#[test]
fn a_single_citrate_link_is_accepted() {
    let got = urls_from_argv(&argv(&["/opt/citrate-core", "citrate://join/abc"]));
    assert_eq!(strs(&got), vec!["citrate://join/abc"]);
}

#[test]
fn the_binary_name_is_never_treated_as_a_link() {
    let got = urls_from_argv(&argv(&["citrate://join/abc"]));
    assert!(got.is_empty());
    assert!(urls_from_argv(&argv(&[])).is_empty());
}

#[test]
fn other_schemes_and_plain_arguments_are_dropped() {
    let got = urls_from_argv(&argv(&[
        "citrate-core",
        "https://citrate.ai/join/abc",
        "http://citrate.ai/join/abc",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "mailto:a@b.c",
        "citrate-widget://localhost/x",
        "--minimized",
        "/usr/bin/citrate-core",
        "C:\\Program Files\\Citrate Core\\citrate-core.exe",
        "",
        "   ",
    ]));
    assert!(
        got.is_empty(),
        "nothing but citrate:// may pass, got {:?}",
        strs(&got)
    );
}

#[test]
fn a_scheme_without_an_authority_is_dropped() {
    assert!(urls_from_argv(&argv(&["app", "citrate:join/abc"])).is_empty());
    assert!(urls_from_argv(&argv(&["app", "citrate:"])).is_empty());
}

#[test]
fn several_arguments_keep_only_citrate_links_in_order() {
    let got = urls_from_argv(&argv(&[
        "citrate-core.exe",
        "--flag",
        "citrate://join/first",
        "https://example.com/#x",
        "citrate://pair?v=1&sig=abc",
        "citrate://join/first",
    ]));
    assert_eq!(
        strs(&got),
        vec!["citrate://join/first", "citrate://pair?v=1&sig=abc"],
        "order kept, exact duplicates delivered once"
    );
}

#[test]
fn windows_style_quoting_is_unwrapped() {
    let got = urls_from_argv(&argv(&[
        "C:\\Program Files\\Citrate Core\\citrate-core.exe",
        "\"citrate://join/abc\"",
        "  citrate://join/def  ",
        " \"citrate://join/ghi\" ",
    ]));
    assert_eq!(
        strs(&got),
        vec![
            "citrate://join/abc",
            "citrate://join/def",
            "citrate://join/ghi"
        ]
    );
}

#[test]
fn unbalanced_quotes_are_dropped() {
    assert!(urls_from_argv(&argv(&["app", "\"citrate://join/abc"])).is_empty());
    assert!(urls_from_argv(&argv(&["app", "citrate://join/abc\""])).is_empty());
}

#[test]
fn the_scheme_match_is_case_insensitive() {
    let got = urls_from_argv(&argv(&["app", "CITRATE://join/abc"]));
    assert_eq!(strs(&got), vec!["citrate://join/abc"]);
}

#[test]
fn an_overlong_argument_is_dropped() {
    let long = format!("citrate://join/{}", "a".repeat(MAX_LINK_LEN));
    assert!(urls_from_argv(&argv(&["app", &long])).is_empty());
}

#[test]
fn the_fragment_is_delivered_intact_to_the_app() {
    // The app needs the fragment; only logs must not carry it.
    let got = urls_from_argv(&argv(&["app", "citrate://join/abc#k=s3cr3t-value"]));
    assert_eq!(strs(&got), vec!["citrate://join/abc#k=s3cr3t-value"]);
}

#[test]
fn accepted_schemes_match_tauri_conf() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json parses");
    let schemes: Vec<String> = conf["plugins"]["deep-link"]["desktop"]["schemes"]
        .as_array()
        .expect("deep-link desktop schemes listed")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    assert_eq!(
        schemes,
        SCHEMES.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    );
}

// --- log redaction: scheme and host only ------------------------------------------------------

/// Marker values placed in every part of a link (path, query, fragment) that a log must not carry.
const LINK_PARTS: &[&str] = &[
    "tok3n-T-value",
    "s1g-S-value",
    "c0de-C-value",
    "k3y-K-value",
    "member@example.org",
    "ord3r-O-value",
    "pAth-s3gment",
    "fr4gment-value",
];

fn sample_links() -> Vec<String> {
    vec![
        "citrate://invite/pAth-s3gment?t=tok3n-T-value&s=s1g-S-value#fr4gment-value".to_string(),
        "citrate://pair/pAth-s3gment?c=c0de-C-value&k=k3y-K-value#fr4gment-value".to_string(),
        "citrate://claim/pAth-s3gment?email=member%40example.org&order=ord3r-O-value#fr4gment-value"
            .to_string(),
        "citrate://claim?email=member@example.org&order=ord3r-O-value&t=tok3n-T-value".to_string(),
    ]
}

fn assert_clean(out: &str) {
    // The failure message names the marker by position only: it never repeats a link part.
    for (i, part) in LINK_PARTS.iter().enumerate() {
        assert!(
            !out.contains(part),
            "link part #{i} appears in the log line"
        );
    }
    assert!(!out.contains("member%40example.org"), "{out}");
    assert!(!out.contains('?'), "query marker appears in: {out}");
    assert!(!out.contains('#'), "fragment marker appears in: {out}");
}

#[test]
fn log_redact_keeps_only_scheme_and_host() {
    let urls = urls_from_argv(&argv(
        &std::iter::once("app".to_string())
            .chain(sample_links())
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    ));
    assert_eq!(urls.len(), 4);
    let redacted: Vec<String> = urls.iter().map(log_redact).collect();
    assert_eq!(
        redacted,
        vec![
            "citrate://invite",
            "citrate://pair",
            "citrate://claim",
            "citrate://claim"
        ]
    );
    for r in &redacted {
        assert_clean(r);
    }
}

#[test]
fn delivery_keeps_the_full_url_while_the_log_does_not() {
    let raw = argv(&["app", &sample_links()[0]]);
    let urls = urls_from_argv(&raw);
    assert_eq!(
        urls[0].as_str(),
        sample_links()[0],
        "the app receives the link unchanged"
    );
    assert_eq!(log_redact(&urls[0]), "citrate://invite");
}

#[test]
fn the_delivery_log_line_records_scheme_host_and_counts_only() {
    let mut args = vec!["app".to_string()];
    args.extend(sample_links());
    args.push("https://citrate.ai/join/pAth-s3gment?t=tok3n-T-value#fr4gment-value".to_string());
    args.push("--flag".to_string());
    let raw: Vec<&str> = args.iter().map(String::as_str).collect();
    let raw = argv(&raw);
    let urls = urls_from_argv(&raw);
    let line = delivery_log_line(&urls, ignored_count(&raw, &urls));
    assert_clean(&line);
    assert!(line.contains("4 link(s) delivered"), "{line}");
    assert!(
        line.contains("citrate://invite, citrate://pair, citrate://claim"),
        "{line}"
    );
    assert!(line.contains("2 ignored"), "{line}");
    // Ignored arguments are counted, never echoed.
    assert!(!line.contains("https://"));
    assert!(!line.contains("--flag"));
}

#[test]
fn a_single_link_logs_as_one_link_delivered() {
    let raw = argv(&["app", "citrate://invite?t=tok3n-T-value"]);
    let urls = urls_from_argv(&raw);
    let line = delivery_log_line(&urls, ignored_count(&raw, &urls));
    assert_clean(&line);
    assert!(
        line.contains("1 link(s) delivered to the running app: citrate://invite"),
        "{line}"
    );
}

#[test]
fn log_redact_text_reduces_urls_in_free_text() {
    for link in sample_links() {
        let msg = format!("xdg-mime failed for '{link}' (exit 1) k=k3y-K-value#x");
        let out = log_redact_text(&msg);
        assert_clean(&out);
        assert!(out.contains("citrate://"), "{out}");
        assert!(out.contains("xdg-mime failed for"), "{out}");
    }
    assert_eq!(log_redact_text("plain error text"), "plain error text");
    assert_eq!(log_redact_text(""), "");
}

#[test]
fn log_redact_text_drops_a_bare_email_raw_or_percent_encoded() {
    for msg in [
        "registration failed for a@b.example: permission denied",
        "registration failed for (a@b.example), see log",
        "registration failed for a%40b.example: permission denied",
        "registration failed for a%40B.example: permission denied",
        "owner member@example.org; retry",
    ] {
        let out = log_redact_text(msg);
        for leaked in [
            "a@",
            "@b",
            "b.example",
            "B.example",
            "%40",
            "member",
            "example.org",
        ] {
            assert!(!out.contains(leaked), "{leaked} appears in: {out}");
        }
        assert!(out.contains("<email>"), "{out}");
        assert!(
            out.starts_with("registration failed for") || out.starts_with("owner"),
            "{out}"
        );
    }
}

#[test]
fn log_redact_text_hides_an_unparseable_url() {
    let out = log_redact_text("bad citrate://[pAth-s3gment?t=tok3n-T-value");
    assert_clean(&out);
    assert!(out.contains("<url>"), "{out}");
}

#[test]
fn ignored_count_excludes_the_binary_name() {
    let raw = argv(&["app", "citrate://join/a", "--x"]);
    let urls = urls_from_argv(&raw);
    assert_eq!(ignored_count(&raw, &urls), 1);
    assert_eq!(ignored_count(&argv(&["app"]), &[]), 0);
    assert_eq!(ignored_count(&argv(&[]), &[]), 0);
}

// --- AppImage scheme registration guard -------------------------------------------------------

#[test]
fn appimage_registration_runs_only_on_linux_with_appimage_set() {
    use std::ffi::OsStr;
    let path = OsStr::new("/home/m/Apps/Citrate_Core.AppImage");
    assert!(should_register_appimage_schemes(true, Some(path)));
    assert!(!should_register_appimage_schemes(true, None));
    assert!(!should_register_appimage_schemes(
        true,
        Some(OsStr::new(""))
    ));
    assert!(!should_register_appimage_schemes(false, Some(path)));
    assert!(!should_register_appimage_schemes(false, None));
}

#[test]
fn appimage_registration_log_lines_carry_no_url() {
    let ok = appimage_register_log_line(&Ok(()));
    let err =
        appimage_register_log_line(&Err(format!("xdg-mime failed for {}", sample_links()[1])));
    for line in [&ok, &err] {
        assert_clean(line);
    }
    assert!(ok.contains("registered"));
    assert!(err.contains("failed"));
}
