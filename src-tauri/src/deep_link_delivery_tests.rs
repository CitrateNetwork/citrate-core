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

// --- fragment redaction -----------------------------------------------------------------------

const SECRET: &str = "s3cr3t-fragment-value";

#[test]
fn redaction_removes_everything_after_the_hash() {
    let raw = format!("citrate://join/abc?x=1#{SECRET}");
    let out = redact_fragment(&raw);
    assert!(!out.contains(SECRET));
    assert_eq!(out, "citrate://join/abc?x=1#<redacted>");
}

#[test]
fn redaction_leaves_a_fragmentless_url_unchanged() {
    assert_eq!(redact_fragment("citrate://join/abc"), "citrate://join/abc");
    assert_eq!(redact_fragment(""), "");
}

#[test]
fn redaction_cuts_at_the_first_hash() {
    let raw = format!("citrate://join/a#{SECRET}#more-{SECRET}");
    let out = redact_fragment(&raw);
    assert!(!out.contains(SECRET));
    assert!(!out.contains("more"));
    assert_eq!(out, "citrate://join/a#<redacted>");
}

#[test]
fn redaction_handles_an_empty_fragment_and_non_ascii() {
    assert_eq!(
        redact_fragment("citrate://join/a#"),
        "citrate://join/a#<redacted>"
    );
    let out = redact_fragment("citrate://join/é#ü-secret");
    assert_eq!(out, "citrate://join/é#<redacted>");
}

#[test]
fn the_delivery_log_line_never_contains_a_fragment() {
    let raw = argv(&[
        "app",
        &format!("citrate://join/abc#{SECRET}"),
        &format!("citrate://pair?v=1#{SECRET}"),
        &format!("https://citrate.ai/join/abc#{SECRET}"),
        "--flag",
    ]);
    let urls = urls_from_argv(&raw);
    assert_eq!(urls.len(), 2);
    let line = delivery_log_line(&urls, ignored_count(&raw, &urls));
    assert!(!line.contains(SECRET), "log line leaked a fragment: {line}");
    assert!(line.contains("citrate://join/abc#<redacted>"));
    assert!(line.contains("2 ignored"), "{line}");
    // Ignored arguments are counted, never echoed.
    assert!(!line.contains("https://"));
    assert!(!line.contains("--flag"));
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
    let err = appimage_register_log_line(&Err(format!(
        "xdg-mime failed for citrate://join/a#{SECRET}"
    )));
    for line in [&ok, &err] {
        assert!(!line.contains(SECRET), "{line}");
    }
    assert!(ok.contains("registered"));
    assert!(err.contains("failed"));
}
