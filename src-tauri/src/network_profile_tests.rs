// K6 network switch: the network profiles, their fail-closed "not set" state, and the
// tripwires that keep every other copy of a network value equal to the active profile.

use super::*;

const NEXT: &str = "genesis-2026-12";

fn ready_alpha() -> ReadyNetwork {
    active_ready().expect("the active profile is ready")
}

/// A release build must target a network whose every value is set. Flipping `active` to a
/// profile that is not set yet fails here, in CI, before a build can ship.
#[test]
fn the_active_profile_is_ready() {
    let net = ready_alpha();
    assert_eq!(net.id, active_id().unwrap());
    assert!(!net.bootnodes.is_empty());
}

#[test]
fn the_embedded_profiles_parse_and_the_active_one_exists() {
    let all = profiles().expect("profiles.json parses");
    assert!(all.len() >= 2, "the live network and the next genesis");
    assert!(all.iter().any(|p| p.id == active_id().unwrap()));
    assert!(matches!(
        profile("no-such-network"),
        Err(ProfileError::UnknownProfile(_))
    ));
    assert!(matches!(parse("{"), Err(ProfileError::Malformed(_))));
    // An unknown key is refused, so a typo cannot silently drop a value.
    let typo = r#"{"active":"a","profiles":{"a":{"label":"x","chainId":1,"genesisHas":"0x"}}}"#;
    assert!(matches!(parse(typo), Err(ProfileError::Malformed(_))));
}

/// The next genesis is defined but not set: every value is null with a reason, and nothing about
/// it looks like a real value.
#[test]
fn the_next_genesis_profile_is_explicitly_not_set() {
    let p = profile(NEXT).expect("the next-genesis profile exists");
    let missing = p.missing();
    let names: Vec<&str> = missing.iter().map(|(k, _)| k.as_str()).collect();
    for field in [
        "chainId",
        "genesisHash",
        "nodeNetwork",
        "rpcUrl",
        "explorerUrl",
        "faucetUrl",
        "bootnodes",
        "addressBook",
        "addressBookSha256",
        "minChainRev",
    ] {
        assert!(
            names.contains(&field),
            "{field} must be unset until decided: {names:?}"
        );
    }
    for (field, why) in &missing {
        assert!(
            why != "not recorded" && !why.is_empty(),
            "{field} is unset with no reason recorded in `pending`"
        );
    }
    match p.ready() {
        Err(ProfileError::NotSet { profile, missing }) => {
            assert_eq!(profile, NEXT);
            assert_eq!(missing.len(), 10);
        }
        other => panic!("a not-set profile must not be ready: {other:?}"),
    }
    let gate = p.expected_network();
    assert_eq!(gate.chain_id, None);
    assert_eq!(gate.genesis_hash, None);
}

/// Every profile: a null value has a reason, and a reason is only given for a null value (a
/// stale reason next to a real value would mislead).
#[test]
fn pending_reasons_match_exactly_the_unset_values() {
    for p in profiles().unwrap() {
        let unset: Vec<String> = p.missing().into_iter().map(|(k, _)| k).collect();
        let reasons: Vec<String> = p.pending.keys().cloned().collect();
        let mut a = unset.clone();
        a.sort();
        let mut b = reasons.clone();
        b.sort();
        assert_eq!(
            a, b,
            "profile {}: unset {unset:?} vs reasons {reasons:?}",
            p.id
        );
    }
}

#[test]
fn ready_refuses_values_that_only_look_set() {
    let base = active().unwrap().clone();
    type Mutate = Box<dyn Fn(&mut NetworkProfile)>;
    let cases: Vec<(&str, Mutate)> = vec![
        ("chainId", Box::new(|p| p.chain_id = Some(0))),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some(format!("0x{}", "0".repeat(64)))),
        ),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some("0x1234".into())),
        ),
        (
            "genesisHash",
            Box::new(|p| p.genesis_hash = Some(String::new())),
        ),
        (
            "nodeNetwork",
            Box::new(|p| p.node_network = Some(String::new())),
        ),
        (
            "nodeNetwork",
            Box::new(|p| p.node_network = Some("test net".into())),
        ),
        (
            "rpcUrl",
            Box::new(|p| p.rpc_url = Some("http://rpc.citrate.ai".into())),
        ),
        ("rpcUrl", Box::new(|p| p.rpc_url = Some("https://".into()))),
        (
            "explorerUrl",
            Box::new(|p| p.explorer_url = Some(String::new())),
        ),
        (
            "faucetUrl",
            Box::new(|p| p.faucet_url = Some("ftp://x".into())),
        ),
        ("bootnodes", Box::new(|p| p.bootnodes = Some(Vec::new()))),
        (
            "bootnodes",
            Box::new(|p| p.bootnodes = Some(vec!["boot1.citrate.ai:30303".into()])),
        ),
        (
            "bootnodes",
            Box::new(|p| p.bootnodes = Some(vec![format!("noise_{}@h\"x:30303", "a".repeat(64))])),
        ),
        (
            "addressBook",
            Box::new(|p| p.address_book = Some(String::new())),
        ),
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("ab".repeat(31))),
        ),
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("AB".repeat(32))),
        ),
        (
            "addressBookSha256",
            Box::new(|p| p.address_book_sha256 = Some("0".repeat(64))),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("c384fc98".into())),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("0".repeat(40))),
        ),
        (
            "minChainRev",
            Box::new(|p| p.min_chain_rev = Some("g".repeat(40))),
        ),
    ];
    for (field, mutate) in cases {
        let mut p = base.clone();
        mutate(&mut p);
        match p.ready() {
            Err(ProfileError::Invalid { field: f, .. }) => assert_eq!(f, field),
            other => panic!("{field}: expected Invalid, got {other:?}"),
        }
    }
}

#[test]
fn bootnode_form_is_checked() {
    let key = "ab".repeat(32);
    assert!(is_bootnode(&format!("noise_{key}@boot1.citrate.ai:30303")));
    assert!(!is_bootnode(&format!("noise_{key}@boot1.citrate.ai:0")));
    assert!(!is_bootnode(&format!("noise_{key}@:30303")));
    assert!(!is_bootnode(&format!("noise_{}@h:30303", "ab".repeat(31))));
    assert!(!is_bootnode(&format!("{key}@h:30303")));
    assert!(!is_bootnode(&format!("noise_{key}@h")));
}

#[test]
fn the_gate_for_the_active_profile_carries_its_chain_id_and_genesis() {
    let net = ready_alpha();
    let gate = active().unwrap().expected_network();
    assert_eq!(gate.chain_id, Some(net.chain_id));
    assert_eq!(
        gate.genesis_hash.as_deref(),
        Some(net.genesis_hash.as_str())
    );
    assert_eq!(gate.name, net.id);
}

#[test]
fn not_set_error_names_the_missing_values() {
    let e = profile(NEXT).unwrap().ready().unwrap_err().to_string();
    assert!(
        e.contains(NEXT) && e.contains("genesisHash") && e.contains("bootnodes"),
        "{e}"
    );
    assert!(e.contains("nothing is synced or signed"), "{e}");
}

// ---- rendering the member node config ----

#[test]
fn the_template_has_no_network_values_of_its_own() {
    let t = include_str!("../config/member-node.toml");
    assert!(t.contains(CHAIN_ID_PLACEHOLDER), "chain id placeholder");
    assert!(
        t.contains(BOOTNODES_PLACEHOLDER),
        "bootnode placeholder line"
    );
    assert_eq!(
        t.matches("noise_").count(),
        0,
        "no bootnode outside the profile"
    );
    assert!(!t.contains("40204"), "no chain id outside the profile");
    // The template stays valid TOML, so tools that read ports from it keep working.
    t.parse::<toml::Table>().expect("template is valid TOML");
}

#[test]
fn rendering_fills_every_network_value() {
    let net = ready_alpha();
    let out = render_node_network(include_str!("../config/member-node.toml"), &net);
    assert!(!out.contains("{{CHAIN_ID}}") && !out.contains("{{BOOTSTRAP_NODES}}"));
    let t: toml::Table = out
        .replace("{{DATA_DIR}}", "/tmp/x")
        .parse()
        .expect("rendered config is valid TOML");
    assert_eq!(
        t["chain"]["chain_id"].as_integer(),
        Some(i64::try_from(net.chain_id).unwrap())
    );
    let nodes: Vec<&str> = t["network"]["bootstrap_nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(
        nodes,
        net.bootnodes.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

// ---- tripwires: every other copy equals the active profile ----

/// The address book the build embeds is the one the profile names, for the same chain and
/// genesis. A new genesis means a new book AND a new profile; they cannot drift apart.
#[test]
fn the_address_book_matches_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(net.address_book, "addresses/40204.json");
    assert!(
        include_str!("addresses.rs")
            .contains(&format!("include_str!(\"../{}\")", net.address_book)),
        "addresses.rs must embed the book the active profile names"
    );
    assert_eq!(crate::addresses::chain_id(), net.chain_id);
    assert_eq!(crate::addresses::genesis_hash(), net.genesis_hash);
}

#[test]
fn the_kit_rpc_constants_match_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(crate::rpc::CITRATE_RPC_URL, net.rpc_url);
    assert_eq!(crate::rpc::CITRATE_CHAIN_ID, net.chain_id);
}

#[test]
fn the_explorer_and_faucet_urls_match_the_active_profile() {
    let net = ready_alpha();
    assert_eq!(crate::activity::EXPLORER_BASE, net.explorer_url);
    assert_eq!(crate::faucet::DEFAULT_FAUCET_URL, net.faucet_url);
}

/// The withdrawn-branch checks are per genesis; each must belong to a profile this build knows.
#[test]
fn every_withdrawn_branch_check_belongs_to_a_known_profile() {
    let known: Vec<String> = profiles()
        .unwrap()
        .iter()
        .filter_map(|p| p.genesis_hash.clone())
        .collect();
    for c in crate::node_withdrawn::FORK_CHECKS {
        assert!(known.iter().any(|g| g == c.genesis), "{}", c.label);
    }
}

/// The webview reads the same file (src/network/profile.ts), not a copy.
#[test]
fn the_webview_reads_the_same_profiles_file() {
    let ts = include_str!("../../src/network/profile.ts");
    assert!(
        ts.contains("src-tauri/network/profiles.json"),
        "profile.ts imports profiles.json"
    );
}

/// K6: the signing gate is set only from a fully ready profile. The active profile gives a set
/// gate; the next-genesis profile (every value null) would give one that refuses every signature.
#[test]
fn the_signing_gate_is_set_only_from_a_ready_profile() {
    let gate = active_gate();
    assert!(gate.is_set(), "the active profile is ready: {gate:?}");
    let r = active().unwrap().ready().unwrap();
    assert_eq!(gate.chain_id, Some(r.chain_id));
    assert_eq!(gate.genesis_hash.as_deref(), Some(r.genesis_hash.as_str()));
    let next = profile("genesis-2026-12").unwrap();
    assert!(next.ready().is_err());
    assert!(!next.expected_network().is_set());
    assert!(crate::rpc::with_added_network_gate(next.expected_network(), || {
        crate::rpc::check_wallet_signing(crate::rpc::SignatureScope::Unbound)
    })
    .is_err());
}

/// K6: a profile with its chain id and genesis set but anything else not set is not fully set,
/// so its gate refuses every signature.
#[test]
fn a_partly_set_profile_gives_a_gate_that_refuses() {
    let json = PROFILES_JSON.replace(
        "\"addressBook\": \"addresses/40204.json\"",
        "\"addressBook\": null",
    );
    assert_ne!(json, PROFILES_JSON, "the replacement hit the active profile");
    let (active_id, all) = parse(&json).expect("parses");
    let p = all.iter().find(|p| p.id == active_id).expect("active");
    assert!(p.chain_id.is_some() && p.genesis_hash.is_some());
    assert!(!p.expected_network().is_set());
}

/// The `cfg` names a build script's output sets (`cargo:rustc-cfg=…` or `cargo::rustc-cfg=…`),
/// value included (`name` or `name="value"`).
fn k6_emitted_cfgs(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| {
            l.strip_prefix("cargo::rustc-cfg=")
                .or_else(|| l.strip_prefix("cargo:rustc-cfg="))
        })
        .map(|c| c.trim().to_string())
        .collect()
}

/// K6 (MAC round 6 on core#363): the key-access scans in kit read `#[cfg(test)]` as test-only.
/// That holds only while no build script sets `cfg(test)` for a non-test build. kit has no build
/// script (pinned in kit). This reads what the app's build script actually emitted (Cargo keeps
/// its stdout beside OUT_DIR, so the answer comes from the build, not from reading the script),
/// and pins the script's source so a conditional emission cannot be added unnoticed.
#[test]
fn k6_app_build_script_sets_no_test_cfg() {
    let sample = concat!(
        "cargo:rerun-if-changed=x\n",
        "cargo:rustc-cfg=desktop\n",
        "cargo::rustc-cfg=test\n",
        "cargo:rustc-cfg=feature=\"x\"\n",
        "cargo:rustc-check-cfg=cfg(test)\n",
        "cargo:rustc-env=A=cargo:rustc-cfg=test\n",
    );
    assert_eq!(
        k6_emitted_cfgs(sample),
        vec!["desktop", "test", "feature=\"x\""],
        "both directive spellings; check-cfg and env lines set no cfg"
    );
    let out = std::path::Path::new(env!("OUT_DIR")).join("../output");
    let emitted = std::fs::read_to_string(&out)
        .unwrap_or_else(|e| panic!("read build script output {}: {e}", out.display()));
    let cfgs = k6_emitted_cfgs(&emitted);
    assert!(
        cfgs.iter().any(|c| c == "desktop"),
        "positive control: tauri_build's `desktop` cfg is seen ({cfgs:?})"
    );
    assert!(
        !cfgs.iter().any(|c| c == "test"),
        "the app build script set cfg(test) for every build: {cfgs:?}"
    );
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");
    let src = std::fs::read_to_string(&script).expect("read src-tauri/build.rs");
    let squashed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(
        squashed, "fnmain(){tauri_build::build()}",
        "src-tauri/build.rs changed; re-check that it sets no cfg(test), then update this pin"
    );
}

// ---------------------------------------------------------------------------
// #493 / K6-3 tripwire: no chain id or RPC URL literal outside `profiles.json`.
//
// Every signing and network path takes the chain id from `rpc::CITRATE_CHAIN_ID` and the RPC from
// `rpc::CITRATE_RPC_URL`, the two kit constants `the_kit_rpc_constants_match_the_active_profile`
// pins to the active profile. A literal anywhere else would keep the old value when the profile
// changes (owner decision D1 may change the chain id at genesis), so this scans the production
// Rust of the app and the kit and fails on:
// - the chain id as a number in code (`40204`, `40_204`, `40204u64`, `0x9d0c`);
// - the chain id as a whole string value (`"40204"`, `"0x9d0c"`, `eip155:40204`);
// - an RPC URL (`https://rpc.citrate.ai`, `wss://rpc.citrate.ai`).
// Comments, test modules and `*_tests.rs` files are not production paths and are skipped. Prose
// that names the chain in a message ("chain 40204") is copy, not a value a path uses, and is not
// matched.
// ---------------------------------------------------------------------------

/// One Rust source file with its comments removed and its string literals blanked, plus every
/// string literal (byte offset in the blanked text, contents). Newlines are kept, so line numbers
/// match the file.
struct K6Lexed {
    code: String,
    strings: Vec<(usize, String)>,
}

fn k6_lex_rust(src: &str) -> K6Lexed {
    let b: Vec<char> = src.chars().collect();
    let mut code = String::with_capacity(src.len());
    let mut strings = Vec::new();
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                code.push(' ');
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    code.push_str("  ");
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    code.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    code.push(blank(b[i]));
                    i += 1;
                }
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br#".."# (an identifier ending in r is not one: the char
        // before must not be part of an identifier).
        let ident_before = i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_');
        if !ident_before && (c == 'r' || (c == 'b' && next == Some('r'))) {
            let mut j = i + if c == 'b' { 2 } else { 1 };
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&'"') {
                let start = code.len() + (j - i);
                for _ in i..=j {
                    code.push(' ');
                }
                j += 1;
                let mut s = String::new();
                loop {
                    if j >= b.len() {
                        break;
                    }
                    if b[j] == '"' && (0..hashes).all(|k| b.get(j + 1 + k) == Some(&'#')) {
                        for _ in 0..=hashes {
                            code.push(' ');
                        }
                        j += 1 + hashes;
                        break;
                    }
                    s.push(b[j]);
                    code.push(blank(b[j]));
                    j += 1;
                }
                strings.push((start, s));
                i = j;
                continue;
            }
        }
        if c == '"' {
            let start = code.len();
            code.push(' ');
            i += 1;
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    s.push(b[i]);
                    s.push(b[i + 1]);
                    code.push(blank(b[i]));
                    code.push(blank(b[i + 1]));
                    i += 2;
                    continue;
                }
                s.push(b[i]);
                code.push(blank(b[i]));
                i += 1;
            }
            code.push(' ');
            i += 1;
            strings.push((start, s));
            continue;
        }
        if c == '\'' {
            // A char literal ('x', '\n', '\'', '\u{..}'); otherwise a lifetime.
            let end = if next == Some('\\') {
                (i + 2..b.len().min(i + 12)).find(|&k| b[k] == '\'' && k > i + 2)
            } else if b.get(i + 2) == Some(&'\'') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(e) = end {
                for &ch in &b[i..=e] {
                    code.push(blank(ch));
                }
                i = e + 1;
                continue;
            }
        }
        code.push(c);
        i += 1;
    }
    K6Lexed { code, strings }
}

/// Byte ranges of `#[cfg(test)] mod name { .. }` blocks in lexed code (comments and strings are
/// already blanked, so brace counting is exact).
fn k6_test_module_ranges(code: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = code[from..].find("#[cfg(test)]") {
        let at = from + p;
        let rest = &code[at + "#[cfg(test)]".len()..];
        let trimmed = rest.trim_start();
        let after_vis = trimmed.strip_prefix("pub ").unwrap_or(trimmed).trim_start();
        if let Some(m) = after_vis.strip_prefix("mod ") {
            let name_end = m
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(m.len());
            if m[name_end..].trim_start().starts_with('{') {
                let open = code.len() - m.len() + name_end + m[name_end..].find('{').unwrap_or(0);
                let mut depth = 0i64;
                let mut end = code.len();
                for (k, ch) in code[open..].char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = open + k + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                out.push((at, end));
                from = end;
                continue;
            }
        }
        from = at + 1;
    }
    out
}

/// The value of a Rust integer literal token (`40_204`, `40204u64`, `0x9d0c`), or None.
fn k6_int_literal_value(tok: &str) -> Option<u64> {
    let t = tok.to_ascii_lowercase().replace('_', "");
    const SUFFIXES: &[&str] = &[
        "u128", "usize", "isize", "i128", "u64", "u32", "u16", "i64", "i32", "i16", "u8", "i8",
    ];
    let t = SUFFIXES
        .iter()
        .find_map(|s| t.strip_suffix(s))
        .unwrap_or(&t);
    match t.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => t.parse().ok(),
    }
}

/// A chain id as a JSON or key=value value inside a string (lowercased): `"chainid":40204`,
/// `chain_id: "0x9d0c"`, with or without quotes, escapes and spaces.
fn k6_json_chain_id(lower: &str) -> bool {
    for value in ["40204", "40_204", "0x9d0c"] {
        let mut from = 0;
        while let Some(p) = lower[from..].find(value) {
            let at = from + p;
            from = at + value.len();
            let after_ok = lower[from..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
            let key = lower[..at].trim_end_matches(|c: char| {
                c.is_whitespace() || matches!(c, '"' | '\'' | '\\' | ':' | '=')
            });
            if after_ok && (key.ends_with("chainid") || key.ends_with("chain_id")) {
                return true;
            }
        }
    }
    false
}

/// The literals in one production Rust source that pin a network value outside the profile.
/// Each finding is `line: what`.
fn k6_network_literals_in_rust(src: &str) -> Vec<String> {
    let lexed = k6_lex_rust(src);
    let skip = k6_test_module_ranges(&lexed.code);
    let in_test = |off: usize| skip.iter().any(|&(a, b)| off >= a && off < b);
    let line_of = |off: usize| lexed.code[..off].matches('\n').count() + 1;
    let mut out = Vec::new();
    // Numbers in code. A digit run is one token; `40_204`, `40204u64`, `0x9d0c` all count.
    let code = lexed.code.as_bytes();
    let mut i = 0;
    while i < code.len() {
        let c = code[i];
        let starts_token = i == 0 || !(code[i - 1].is_ascii_alphanumeric() || code[i - 1] == b'_');
        if c.is_ascii_digit() && starts_token {
            let mut j = i;
            while j < code.len() && (code[j].is_ascii_alphanumeric() || code[j] == b'_') {
                j += 1;
            }
            if k6_int_literal_value(&lexed.code[i..j]) == Some(40204) && !in_test(i) {
                out.push(format!("{}: chain id {}", line_of(i), &lexed.code[i..j]));
            }
            i = j;
            continue;
        }
        i += 1;
    }
    for (off, s) in &lexed.strings {
        if in_test(*off) {
            continue;
        }
        let lower = s.to_ascii_lowercase();
        if lower == "40204"
            || lower == "0x9d0c"
            || lower.contains("eip155:40204")
            || k6_json_chain_id(&lower)
        {
            out.push(format!("{}: chain id string {s:?}", line_of(*off)));
        }
        // The RPC host with any scheme or none.
        if lower.contains("rpc.citrate.ai") {
            out.push(format!("{}: RPC host {s:?}", line_of(*off)));
        }
    }
    out
}

/// The two definitions every path reads; `the_kit_rpc_constants_match_the_active_profile` pins
/// them to the active profile.
const K6_PINNED_DEFINITIONS: &[(&str, &str)] = &[
    ("kit/src/rpc.rs", "pub const CITRATE_RPC_URL: &str = \"https://rpc.citrate.ai\";"),
    ("kit/src/rpc.rs", "pub const CITRATE_CHAIN_ID: u64 = 40204;"),
];

fn k6_production_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if p.is_dir() {
            if name != "tests" {
                k6_production_rust_files(&p, out);
            }
        } else if name.ends_with(".rs") && !name.ends_with("_tests.rs") && name != "tests.rs" {
            out.push(p);
        }
    }
}

#[test]
fn k6_the_literal_scan_finds_each_form_and_skips_comments_tests_and_prose() {
    let src = r##"
// a comment with 40204 and https://rpc.citrate.ai
/* block 40204 */
const A: u64 = 40204;
const B: u64 = 40_204;
fn f() -> String { format!("0x{:x}", 40204u64) }
const C: u64 = 0x9d0c;
const D: &str = "40204";
const E: &str = "https://rpc.citrate.ai";
fn g(h: &str) -> String { format!("eip155:40204/tx/{h}") }
const PROSE: &str = "not deployed on chain 40204 yet";
const RAW: &str = r#"chain 40204 "quoted""#;
const N: u64 = 402040;
const LIFETIME: Option<&'static str> = None;
#[cfg(test)]
mod tests {
    const T: u64 = 40204;
    const U: &str = "https://rpc.citrate.ai";
}
const AFTER: u64 = 40204;
const HTTP: &str = "http://rpc.citrate.ai";
const BARE: &str = "rpc.citrate.ai";
const JSON: &str = r#"{"chainId":40204}"#;
const JSON_HEX: &str = "{\"chain_id\": \"0x9d0c\"}";
const OTHER: u64 = 140204 + 402041;
"##;
    let found = k6_network_literals_in_rust(src);
    let mut lines: Vec<usize> = found
        .iter()
        .filter_map(|f| f.split(':').next().and_then(|n| n.parse().ok()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    assert_eq!(
        lines,
        vec![4, 5, 6, 7, 8, 9, 10, 20, 21, 22, 23, 24],
        "{found:#?}"
    );
}

#[test]
fn k6_no_chain_id_or_rpc_literal_outside_the_profile() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    k6_production_rust_files(&manifest.join("src"), &mut files);
    k6_production_rust_files(&manifest.join("../kit/src"), &mut files);
    assert!(
        files.len() > 50,
        "positive control: the scan reads the app and the kit ({} files)",
        files.len()
    );
    let mut offenders = Vec::new();
    let mut pinned_seen = 0;
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        let rel = f
            .strip_prefix(manifest)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| f.display().to_string());
        let rel = rel.trim_start_matches("../").to_string();
        let rel = if rel.starts_with("kit/") { rel } else { format!("src-tauri/{rel}") };
        let lines: Vec<&str> = src.lines().collect();
        for finding in k6_network_literals_in_rust(&src) {
            let n: usize = finding.split(':').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let line = lines.get(n.saturating_sub(1)).map(|l| l.trim()).unwrap_or("");
            if K6_PINNED_DEFINITIONS
                .iter()
                .any(|(file, def)| *file == rel && *def == line)
            {
                pinned_seen += 1;
                continue;
            }
            offenders.push(format!("{rel}:{finding}"));
        }
    }
    assert_eq!(
        pinned_seen, 2,
        "positive control: the scan sees the two pinned kit definitions"
    );
    assert!(
        offenders.is_empty(),
        "network values outside the profile; read rpc::CITRATE_CHAIN_ID / rpc::CITRATE_RPC_URL \
         instead:\n{}",
        offenders.join("\n")
    );
}

// ---- r1006 (RC52 D-2): the rerolled testnet the 0.5.2 RC targets ----

const R1006: &str = "r1006";
const FIXTURE_GENESIS: &str = "0x6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a";

fn r1006() -> &'static NetworkProfile {
    profile(R1006).expect("the r1006 profile exists")
}

/// r1006 with the reroll values filled by test fixtures (what `scripts/fill-network-profile.sh`
/// writes after the reroll).
fn r1006_filled() -> NetworkProfile {
    let mut p = r1006().clone();
    p.genesis_hash = Some(FIXTURE_GENESIS.into());
    p.address_book_sha256 = Some("5c".repeat(32));
    p.min_chain_rev = Some("1f".repeat(20));
    p.pending.clear();
    p
}

fn signs(gate: &crate::rpc::ExpectedNetwork, scope: crate::rpc::SignatureScope) -> bool {
    crate::rpc::with_added_network_gate(gate.clone(), || {
        crate::rpc::check_wallet_signing(scope)
    })
    .is_ok()
}

/// Every value known before the reroll is set: chain id 40204 (the reroll keeps it), the
/// existing fleet's RPC, explorer and faucet, and its bootnodes (citrate-chain
/// node/config/testnet-beta.toml; the Noise keys stay stable across a reroll). Only the values
/// the reroll produces are unset, each with a reason.
#[test]
fn r1006_has_the_values_known_today_and_only_the_reroll_values_unset() {
    let p = r1006();
    assert_eq!(p.chain_id, Some(40204));
    assert_eq!(p.node_network.as_deref(), Some("testnet"));
    assert_eq!(p.rpc_url.as_deref(), Some("https://rpc.citrate.ai"));
    assert_eq!(p.explorer_url.as_deref(), Some("https://explorer.citrate.ai"));
    assert_eq!(p.faucet_url.as_deref(), Some("https://faucet.citrate.ai"));
    assert_eq!(p.address_book.as_deref(), Some("addresses/40204.json"));
    let nodes = p.bootnodes.clone().expect("bootnodes set");
    assert_eq!(nodes.len(), 4);
    assert!(nodes.iter().all(|b| is_bootnode(b)), "{nodes:?}");
    let missing: Vec<String> = p.missing().into_iter().map(|(k, _)| k).collect();
    assert_eq!(missing, ["genesisHash", "addressBookSha256", "minChainRev"]);
}

/// Red first (K6, RC52): until the reroll values are filled, r1006 refuses every wallet
/// signature (unbound and on its own chain) and every RPC read.
#[test]
fn r1006_unfilled_refuses_every_signature() {
    let p = r1006();
    match p.ready() {
        Err(ProfileError::NotSet { profile, missing }) => {
            assert_eq!(profile, R1006);
            assert_eq!(missing.len(), 3, "{missing:?}");
        }
        other => panic!("unfilled r1006 must not be ready: {other:?}"),
    }
    let gate = p.expected_network();
    assert!(!gate.is_set(), "{gate:?}");
    assert!(!signs(&gate, crate::rpc::SignatureScope::Unbound));
    assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(40204)));
    assert!(crate::rpc::with_added_network_gate(gate, crate::rpc::check_rpc_allowed).is_err());
}

/// Each reroll value on its own keeps r1006 refusing: the genesis hash alone is not enough.
#[test]
fn r1006_refuses_while_any_one_reroll_value_is_unset() {
    type Unset = fn(&mut NetworkProfile);
    let cases: [(&str, Unset); 3] = [
        ("genesisHash", |p| p.genesis_hash = None),
        ("addressBookSha256", |p| p.address_book_sha256 = None),
        ("minChainRev", |p| p.min_chain_rev = None),
    ];
    for (field, unset) in cases {
        let mut p = r1006_filled();
        unset(&mut p);
        match p.ready() {
            Err(ProfileError::NotSet { missing, .. }) => {
                assert_eq!(missing.len(), 1, "{field}: {missing:?}");
                assert_eq!(missing[0].0, field);
            }
            other => panic!("{field} unset: expected NotSet, got {other:?}"),
        }
        let gate = p.expected_network();
        assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(40204)), "{field}");
    }
}

/// Filled with fixture values, r1006 is ready and its gate signs on chain 40204 only.
#[test]
fn r1006_filled_with_fixture_values_signs_on_its_chain_only() {
    let p = r1006_filled();
    let r = p.ready().expect("filled r1006 is ready");
    assert_eq!(r.genesis_hash, FIXTURE_GENESIS);
    assert_eq!(r.min_chain_rev, "1f".repeat(20));
    let gate = p.expected_network();
    assert!(gate.is_set());
    assert_eq!(gate.chain_id, Some(40204));
    assert_eq!(gate.genesis_hash.as_deref(), Some(FIXTURE_GENESIS));
    assert!(signs(&gate, crate::rpc::SignatureScope::Unbound));
    assert!(signs(&gate, crate::rpc::SignatureScope::Chain(40204)));
    assert!(!signs(&gate, crate::rpc::SignatureScope::Chain(1)));
}

/// The reroll is a new genesis: r1006 must never reuse r1005's block-0 hash.
#[test]
fn r1006_is_a_different_network_from_r1005() {
    let alpha = profile("alpha-r1005").unwrap();
    assert_ne!(r1006().genesis_hash, alpha.genesis_hash);
    assert_ne!(Some(FIXTURE_GENESIS.to_string()), alpha.genesis_hash);
}

/// The bundled node must contain the active network's minimum chain rev: build-sidecar.sh pins
/// the same commit (re-pinned together after the reroll).
#[test]
fn the_sidecar_min_chain_rev_is_the_active_profiles() {
    let net = ready_alpha();
    assert_eq!(net.min_chain_rev.len(), 40);
    let sidecar = include_str!("../../scripts/build-sidecar.sh");
    assert!(
        sidecar.contains(&format!("MIN_CHAIN_REV=\"{}\"", net.min_chain_rev)),
        "scripts/build-sidecar.sh MIN_CHAIN_REV must equal the active profile's minChainRev {}",
        net.min_chain_rev
    );
}
