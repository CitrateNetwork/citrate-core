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
        "genesisProfile",
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
            assert_eq!(missing.len(), 11);
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
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("r9999".into())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some(String::new())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("R1006".into())),
        ),
        (
            "genesisProfile",
            Box::new(|p| p.genesis_profile = Some("r1006\"\nx = 1".into())),
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
        t.contains(&format!("genesis_profile = \"{GENESIS_PROFILE_PLACEHOLDER}\"")),
        "genesis profile placeholder"
    );
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

/// #511: a send record written before entries named a genesis belongs to r1005. That genesis is
/// the `alpha-r1005` profile's, also after the active profile moves to a new genesis.
#[test]
fn the_send_record_legacy_genesis_is_the_r1005_profile() {
    assert_eq!(
        profile("alpha-r1005").unwrap().genesis_hash.as_deref(),
        Some(citrate_core_kit::submitted_log::LEGACY_GENESIS)
    );
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
    // The r1007 cut retired r1006 (0.5.3); these cases test r1006's own rules, so not retired.
    p.retired = None;
    p.genesis_hash = Some(FIXTURE_GENESIS.into());
    p.address_book_sha256 = Some("5c".repeat(32));
    p.min_chain_rev = Some("1f".repeat(20));
    p.pending.clear();
    p
}

/// r1006 as shipped before the reroll fill: the three reroll values unset, each with a reason.
fn r1006_unfilled() -> NetworkProfile {
    let mut p = r1006().clone();
    p.retired = None;
    p.genesis_hash = None;
    p.address_book_sha256 = None;
    p.min_chain_rev = None;
    for k in ["genesisHash", "addressBookSha256", "minChainRev"] {
        p.pending.insert(k.into(), "set at the reroll".into());
    }
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
/// node/config/testnet-beta.toml; the Noise keys stay stable across a reroll; owner decision:
/// boot1-3 + rpc.citrate.ai). Since the RC52 fill the reroll values are set too (next test).
#[test]
fn r1006_has_the_values_known_today_and_only_the_reroll_values_unset() {
    let p = r1006_unfilled();
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

/// RC52 fill (scripts/fill-network-profile.sh): r1006 carries the reroll values and is the network
/// this build targets. Genesis: DGX fleet_agree + G4 cold-sync (fed#331), = citrate-chain
/// `R1006_GENESIS`; book: citrate-chain contracts/addresses/r1006/40204.json at a857443e; rev: the
/// node the fleet runs.
#[test]
fn r1006_is_filled_with_the_reroll_values_and_retired_by_the_r1007_cut() {
    let p = r1006();
    assert_eq!(p.genesis_hash.as_deref(), Some(R1006_GENESIS));
    assert_eq!(
        p.address_book_sha256.as_deref(),
        Some("724e3843f677588a6aac680554dd1f2db768d5c3ca2bea18bdc0c173c100b90b")
    );
    // The node rev is not repeated here: it may move (a node-local fix on top of 9a32147e), and
    // then only profiles.json minChainRev and build-sidecar.sh MIN_CHAIN_REV change, kept equal by
    // `the_sidecar_min_chain_rev_is_the_active_profiles`.
    let rev = p.min_chain_rev.as_deref().unwrap_or_default();
    assert!(is_lower_hex_nonzero(rev, 40), "{rev:?}");
    assert!(p.pending.is_empty(), "{:?}", p.pending);
    // 0.5.3: the r1007 cut retired r1006 (a 0.5.2 data dir on it is named and set aside) and made
    // r1007 active.
    assert!(p.retired.as_deref().unwrap_or_default().contains("r1007"), "{:?}", p.retired);
    assert!(matches!(p.ready(), Err(ProfileError::Retired { .. })));
    assert_ne!(active_id().unwrap(), R1006);
}

/// Red first (K6, RC52): until the reroll values are filled, r1006 refuses every wallet
/// signature (unbound and on its own chain) and every RPC read.
#[test]
fn r1006_unfilled_refuses_every_signature() {
    let p = r1006_unfilled();
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

// ---- the genesis the bundled node builds (RC52: the 0.5.2 RC runs its node on r1006) ----

/// Block 0 of r1006 (citrate-chain 9a32147e `R1006_GENESIS`; DGX G4 cold-sync, fed#331).
const R1006_GENESIS: &str = "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4";

/// The `[chain] genesis_profile` of the member node config rendered for `net` (None: not set).
fn rendered_genesis_profile(net: &ReadyNetwork) -> Option<String> {
    let out = render_node_network(include_str!("../config/member-node.toml"), net)
        .replace("{{DATA_DIR}}", "/tmp/x");
    let t: toml::Table = out.parse().expect("rendered config is valid TOML");
    t["chain"]
        .get("genesis_profile")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// The embedded profiles file with `edit` applied to its parsed JSON.
fn profiles_json_with(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(PROFILES_JSON).expect("profiles.json");
    edit(&mut v);
    v.to_string()
}

/// The shipped profiles.json edited, with r1006 not retired (the r1007 cut retired it in 0.5.3):
/// for the cases that test r1006's own rules on a ready r1006.
fn profiles_json_with_r1006_live(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    profiles_json_with(|v| {
        if let Some(o) = v["profiles"]["r1006"].as_object_mut() {
            o.remove("retired");
        }
        edit(v)
    })
}

/// Red first (RC52): without `[chain] genesis_profile` the node builds the chain-id default
/// genesis, which on 40204 is r1005's (0x1dcfc490…), not r1006's. The node config the app
/// writes must name the genesis profile of the network it targets.
#[test]
fn the_node_config_names_the_genesis_profile_of_its_network() {
    let r1006 = r1006_filled()
        .ready()
        .expect("filled r1006 is ready");
    assert_eq!(
        rendered_genesis_profile(&r1006).as_deref(),
        Some("r1006"),
        "an r1006 node must be told to build the r1006 genesis"
    );
    let alpha = profile("alpha-r1005")
        .unwrap()
        .ready()
        .expect("alpha-r1005 is ready");
    assert_eq!(
        rendered_genesis_profile(&alpha).as_deref(),
        Some("testnet_beta"),
        "the r1005 network names the genesis profile its block 0 comes from"
    );
}

/// Red first (RC52), fail closed: a profile naming a genesis profile the bundled node does not
/// build (a typo, or a network whose genesis is not in this node) is not ready, so no node starts
/// and nothing is signed. An unset one is "not set". A known one is ready.
#[test]
fn a_genesis_profile_the_bundled_node_does_not_build_is_refused() {
    let fill = |v: &mut serde_json::Value, gp: serde_json::Value| {
        let p = &mut v["profiles"]["r1006"];
        p["genesisHash"] = R1006_GENESIS.into();
        p["addressBookSha256"] = "5c".repeat(32).into();
        p["minChainRev"] = "9a32147e2aac60d388356ecc67488d717aa84c41".into();
        p["genesisProfile"] = gp;
        p["pending"] = serde_json::json!({});
    };
    let r1006_of = |json: String| -> Result<ReadyNetwork, ProfileError> {
        let (_, ps) = parse(&json)?;
        ps.into_iter()
            .find(|p| p.id == "r1006")
            .ok_or_else(|| ProfileError::UnknownProfile("r1006".into()))?
            .ready()
    };
    let unknown = r1006_of(profiles_json_with_r1006_live(|v| fill(v, "r9999".into())));
    assert!(
        matches!(&unknown, Err(ProfileError::Invalid { field: "genesisProfile", .. })),
        "a genesis profile the node does not build must be refused: {unknown:?}"
    );
    let unset = r1006_of(profiles_json_with_r1006_live(|v| {
        fill(v, serde_json::Value::Null);
        v["profiles"]["r1006"]["pending"] =
            serde_json::json!({"genesisProfile": "set at the reroll"});
    }));
    assert!(
        matches!(&unset, Err(ProfileError::NotSet { missing, .. })
            if missing.iter().any(|(k, _)| k == "genesisProfile")),
        "an unset genesis profile is not set: {unset:?}"
    );
    let known = r1006_of(profiles_json_with_r1006_live(|v| fill(v, "r1006".into())));
    assert!(
        matches!(&known, Ok(r) if r.genesis_hash == R1006_GENESIS),
        "r1006 naming the r1006 genesis profile is ready: {known:?}"
    );
}

/// Tripwire (RC52): every shipped profile whose genesis is set names the genesis profile that
/// builds exactly that block 0, so the node can never be told to build another network's genesis
/// (alpha-r1005 → testnet_beta 0x1dcfc490…, r1006 → r1006 0xa3651a27… once filled).
#[test]
fn every_shipped_genesis_is_the_block_0_its_genesis_profile_builds() {
    let table = node_genesis_profiles().expect("nodeGenesisProfiles parses");
    assert!(!table.is_empty());
    for g in table {
        assert!(is_genesis_profile_name(&g.name), "{g:?}");
    }
    let mut checked = 0;
    for p in profiles().unwrap() {
        let (Some(gh), Some(gp)) = (&p.genesis_hash, &p.genesis_profile) else {
            continue;
        };
        let g = node_genesis(gp)
            .unwrap_or_else(|| panic!("profile {}: genesis profile {gp:?} is not in the table", p.id));
        assert_eq!(
            gh.to_ascii_lowercase(),
            g.genesis_hash,
            "profile {}: its genesis is not the block 0 genesis profile {gp} builds",
            p.id
        );
        checked += 1;
    }
    assert!(checked >= 1, "the active profile at least");
    let built = node_genesis("r1006").expect("the bundled node builds r1006");
    assert_eq!(built.genesis_hash, R1006_GENESIS);
    assert!(built.compiled_activations && !built.node_default);
    assert_eq!(r1006().genesis_profile.as_deref(), Some("r1006"));
}

/// The node genesis table is checked when it is read: a bad name or hash is a build defect.
#[test]
fn a_malformed_node_genesis_table_is_refused() {
    for (what, edit) in [
        ("a name the TOML string cannot carry as is", r#"{"r1006\"x": {"genesisHash": "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4", "nodeDefault": false, "compiledActivations": true}}"#),
        ("a hash that is not a block hash", r#"{"r1006": {"genesisHash": "0x1234", "nodeDefault": false, "compiledActivations": true}}"#),
        ("an unknown key", r#"{"r1006": {"genesisHash": "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4", "nodeDefault": false, "compiledActivations": true, "x": 1}}"#),
    ] {
        let json = profiles_json_with(|v| {
            v["nodeGenesisProfiles"] = serde_json::from_str(edit).expect("edit is JSON");
        });
        assert!(
            matches!(parse_node_genesis(&json), Err(ProfileError::Malformed(_))),
            "{what}"
        );
    }
    assert!(parse_node_genesis(PROFILES_JSON).is_ok());
}

// ---- r1007 (0.5.3, CORE-N2): known now, pending until the genesis cut ----

const R1007: &str = "r1007";

fn r1007() -> &'static NetworkProfile {
    profile(R1007).expect("the r1007 profile exists")
}

/// CORE-N2: the r1007 cut (release/network-cut.json, 2026-11-12) is applied: r1007 is the active,
/// ready profile, with the values the cut wrote and nothing pending.
#[test]
fn r1007_is_the_active_profile_after_the_genesis_cut() {
    let p = r1007();
    assert!(p.missing().is_empty(), "{:?}", p.missing());
    assert!(p.pending.is_empty(), "{:?}", p.pending);
    assert_eq!(p.chain_id, Some(40204));
    assert_eq!(p.node_network.as_deref(), Some("testnet"));
    assert_eq!(p.rpc_url.as_deref(), Some("https://rpc.citrate.ai"));
    assert_eq!(p.explorer_url.as_deref(), Some("https://explorer.citrate.ai"));
    assert_eq!(p.faucet_url.as_deref(), Some("https://faucet.citrate.ai"));
    assert_eq!(p.address_book.as_deref(), Some("addresses/40204.json"));
    assert_eq!(p.member_prune_retain, Some(10_000), "CORE-N5 / CV-7a");
    assert_eq!(p.retired, None);
    assert_eq!(active_id().unwrap(), R1007);
    let r = active_ready().expect("the active r1007 is ready");
    assert_eq!(
        r.genesis_hash,
        "0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631"
    );
    assert_eq!(r.node_genesis.name, "r1007");
    assert_eq!(r.min_chain_rev, "a53bac644665a992bf871831250af7354fd69512");
    assert_eq!(crate::addresses::genesis_hash(), r.genesis_hash);
    let gate = p.expected_network();
    assert_eq!(gate.chain_id, Some(40204));
    assert_eq!(gate.genesis_hash.as_deref(), Some(r.genesis_hash.as_str()));
}

/// CORE-N2: a retired profile stays known (a data dir from it can be named and set aside), but
/// it is refused like an unset one: no node starts on it and the gate refuses every signature,
/// even with every value set.
#[test]
fn a_retired_profile_is_refused_even_with_every_value_set() {
    let json = profiles_json_with(|v| {
        v["profiles"]["r1006"]["retired"] = "replaced by r1007 (0.5.3)".into();
    });
    let (_, ps) = parse(&json).expect("a retired profile parses");
    let p = ps.into_iter().find(|p| p.id == "r1006").expect("r1006");
    assert!(p.missing().is_empty(), "every value is set");
    match p.ready() {
        Err(ProfileError::Retired { profile, why }) => {
            assert_eq!(profile, "r1006");
            assert_eq!(why, "replaced by r1007 (0.5.3)");
        }
        other => panic!("a retired profile must be refused: {other:?}"),
    }
    let gate = p.expected_network();
    assert_eq!((gate.chain_id, gate.genesis_hash), (None, None));
    let msg = format!("{}", p.ready().unwrap_err());
    assert!(msg.contains("r1006") && msg.contains("retired"), "{msg}");
    // An empty reason is a malformed value, not "not retired".
    let mut empty = p.clone();
    empty.retired = Some("  ".into());
    assert!(matches!(
        empty.ready(),
        Err(ProfileError::Invalid { field: "retired", .. })
    ));
}

#[test]
fn the_active_profile_is_not_retired() {
    assert_eq!(active().unwrap().retired, None);
}

/// CORE-N5 / CV-7a: a member keeps at least the 10,000-height undo window; less is refused.
#[test]
fn a_member_retention_below_the_undo_window_is_refused() {
    for bad in [0, 1_000, 9_999] {
        let mut p = active().unwrap().clone();
        p.member_prune_retain = Some(bad);
        assert!(
            matches!(
                p.ready(),
                Err(ProfileError::Invalid { field: "memberPruneRetain", .. })
            ),
            "{bad}"
        );
    }
    let mut p = active().unwrap().clone();
    p.member_prune_retain = Some(10_000);
    assert_eq!(p.ready().expect("ready").member_prune_retain, Some(10_000));
}

/// CORE-N5: a profile with a member retention renders a pruned chain store keeping that many
/// blocks; a profile without one keeps the template's full history.
#[test]
fn a_pruned_profile_renders_a_pruned_node_config_and_others_keep_full_history() {
    let template = include_str!("../config/member-node.toml");
    let storage = |net: &ReadyNetwork| -> (bool, i64) {
        let t: toml::Table = render_node_network(template, net)
            .replace("{{DATA_DIR}}", "/tmp/x")
            .parse()
            .expect("valid TOML");
        (
            t["storage"]["pruning"].as_bool().expect("pruning"),
            t["storage"]["keep_blocks"].as_integer().expect("keep_blocks"),
        )
    };
    let mut p = active().unwrap().clone();
    p.member_prune_retain = Some(10_000);
    assert_eq!(storage(&p.ready().expect("ready")), (true, 10_000));
    let mut archive = active().unwrap().clone();
    archive.member_prune_retain = None;
    assert_eq!(storage(&archive.ready().expect("ready")), (false, 100_000));
}

// ---- r1007: the node config a filled cut produces, against the chain's compiled parameters ----
//
// The r1007 node compiles chain parameters for its block 0 and refuses to start when the node
// config's `[checkpoint]` interval, committee size or vote depth differ from them (citrate-chain
// `node/src/genesis.rs` `check_checkpoint_params`, run at start-up on that genesis). The values
// here are CANDIDATE fixtures (fixtures/network-cut/r1007-candidate.json), recorded before the
// chain's integration step; scripts/network-cut.test.mjs applies the same candidate cut to a copy
// of the repo and proves it writes exactly fixtures/network-cut/r1007-candidate.applied.json,
// which is what these tests render.

const R1007_CANDIDATE: &str = include_str!("../../fixtures/network-cut/r1007-candidate.json");
const R1007_CANDIDATE_APPLIED: &str =
    include_str!("../../fixtures/network-cut/r1007-candidate.applied.json");

/// A value of the candidate chain parameters (fixture), by the chain's field name.
fn candidate_param(name: &str) -> u64 {
    let v: serde_json::Value = serde_json::from_str(R1007_CANDIDATE).expect("candidate fixture");
    assert_eq!(v["status"], "candidate", "the fixture says it is a candidate");
    v["chainParams"][name]
        .as_u64()
        .unwrap_or_else(|| panic!("candidate chainParams.{name}"))
}

/// profiles.json as the cut writes it: the embedded file with the applied r1007 entries.
fn applied_r1007_json() -> String {
    let applied: serde_json::Value =
        serde_json::from_str(R1007_CANDIDATE_APPLIED).expect("applied fixture");
    profiles_json_with(|v| {
        v["active"] = applied["active"].clone();
        v["nodeGenesisProfiles"]["r1007"] = applied["nodeGenesisProfiles"]["r1007"].clone();
        v["profiles"]["r1007"] = applied["profiles"]["r1007"].clone();
    })
}

/// The r1007 network after the candidate cut, readied against its own nodeGenesisProfiles table
/// (the cut adds the r1007 entry the bundled node builds).
fn applied_r1007() -> ReadyNetwork {
    let json = applied_r1007_json();
    let (active, ps) = parse(&json).expect("applied profiles parse");
    assert_eq!(active, R1007);
    let table = parse_node_genesis(&json).expect("applied node genesis table parses");
    ps.into_iter()
        .find(|p| p.id == R1007)
        .expect("r1007")
        .ready_with(&table)
        .unwrap_or_else(|e| panic!("the applied r1007 profile must be ready: {e}"))
}

fn rendered_toml(net: &ReadyNetwork) -> toml::Table {
    render_node_network(include_str!("../config/member-node.toml"), net)
        .replace("{{DATA_DIR}}", "/tmp/x")
        .parse()
        .expect("rendered config is valid TOML")
}

/// The comparison the r1007 node makes at start-up, on a rendered config: `[checkpoint]` interval,
/// committee_size and vote_depth must equal the compiled values, where an absent key is the
/// node's default (citrate-chain `node/src/config.rs` `CheckpointNodeConfig`: 50, 100 and
/// `checkpoint::DEFAULT_VOTE_DEPTH` = 3). Returns the names that differ.
fn checkpoint_differences(cfg: &toml::Table, interval: u64, committee: u64, depth: u64) -> Vec<&'static str> {
    let cp = cfg.get("checkpoint").and_then(|v| v.as_table());
    let get = |k: &str, default: u64| -> u64 {
        cp.and_then(|t| t.get(k))
            .map(|v| v.as_integer().and_then(|n| u64::try_from(n).ok()).expect("an integer"))
            .unwrap_or(default)
    };
    let mut bad = Vec::new();
    if get("interval", 50) != interval {
        bad.push("interval");
    }
    if get("committee_size", 100) != committee {
        bad.push("committee_size");
    }
    if get("vote_depth", 3) != depth {
        bad.push("vote_depth");
    }
    bad
}

/// Red first: a filled r1007 cut gives a profile whose node config the r1007 node accepts: it
/// names the r1007 genesis profile on chain 40204, its `[checkpoint]` repeats the compiled chain
/// parameters exactly (explicitly, so a node default can never decide it), and it keeps at least
/// the undo window when it prunes.
#[test]
fn a_filled_r1007_cut_renders_a_node_config_the_r1007_genesis_accepts() {
    let net = applied_r1007();
    let candidate: serde_json::Value = serde_json::from_str(R1007_CANDIDATE).unwrap();
    assert_eq!(net.chain_id, 40204);
    assert_eq!(net.genesis_hash, candidate["genesisHash"].as_str().unwrap());
    assert_eq!(net.node_genesis.name, "r1007");
    assert_eq!(net.node_genesis.genesis_hash, net.genesis_hash);
    assert!(net.node_genesis.compiled_activations && !net.node_genesis.node_default);
    assert_eq!(net.rpc_url, candidate["rpcUrl"].as_str().unwrap());
    let cp = net
        .node_checkpoint
        .expect("a filled r1007 cut sets the profile's [checkpoint] values");
    let (interval, committee, depth) = (
        candidate_param("checkpoint_interval"),
        candidate_param("committee_size"),
        candidate_param("checkpoint_vote_depth"),
    );
    assert_eq!((cp.interval, cp.committee_size, cp.vote_depth), (interval, committee, depth));

    let cfg = rendered_toml(&net);
    assert_eq!(cfg["chain"]["chain_id"].as_integer(), Some(40204));
    assert_eq!(cfg["chain"]["genesis_profile"].as_str(), Some("r1007"));
    let boot = cfg["network"]["bootstrap_nodes"].as_array().expect("bootnodes");
    assert_eq!(boot.len(), 4);
    assert_eq!(
        checkpoint_differences(&cfg, interval, committee, depth),
        Vec::<&str>::new(),
        "the r1007 node refuses this [checkpoint]"
    );
    // Explicit, not left to the node's defaults.
    let section = cfg["checkpoint"].as_table().expect("[checkpoint]");
    for k in ["interval", "committee_size", "vote_depth"] {
        assert!(section.contains_key(k), "[checkpoint] {k} is written");
    }
    // CORE-N5 / CV-7a: a pruning member keeps at least the undo window and the pruning depth.
    assert_eq!(cfg["storage"]["pruning"].as_bool(), Some(true));
    let keep = u64::try_from(cfg["storage"]["keep_blocks"].as_integer().unwrap()).unwrap();
    assert!(keep >= candidate_param("undo_window") && keep >= candidate_param("pruning_depth"));
    // The template's `[chain] ghostdag_k` agrees with the compiled K (informational for the node,
    // which takes K from its compiled parameters).
    assert_eq!(
        cfg["chain"]["ghostdag_k"].as_integer().map(|k| k as u64),
        Some(candidate_param("ghostdag_k"))
    );
}

/// The check above is not vacuous: each `[checkpoint]` value that differs from the compiled one
/// is what the r1007 node refuses, and the rendered config carries whatever the profile says.
#[test]
fn a_differing_r1007_checkpoint_value_is_what_the_node_refuses() {
    let (interval, committee, depth) = (
        candidate_param("checkpoint_interval"),
        candidate_param("committee_size"),
        candidate_param("checkpoint_vote_depth"),
    );
    for (name, edit) in [
        ("interval", (2, committee, depth)),
        ("committee_size", (interval, 10, depth)),
        ("vote_depth", (interval, committee, 1)),
    ] {
        let mut net = applied_r1007();
        net.node_checkpoint = Some(NodeCheckpoint {
            interval: edit.0,
            committee_size: edit.1,
            vote_depth: edit.2,
        });
        assert_eq!(
            checkpoint_differences(&rendered_toml(&net), interval, committee, depth),
            vec![name]
        );
    }
}

/// No behaviour change before the cut: a profile without `nodeCheckpoint` (alpha-r1005, r1006)
/// renders the template's `[checkpoint]` section byte for byte, with no vote_depth line added.
#[test]
fn a_profile_without_node_checkpoint_keeps_the_template_checkpoint_section() {
    let template = include_str!("../config/member-node.toml");
    let section = |t: &str| -> String {
        let start = t.find("\n[checkpoint]").expect("[checkpoint]");
        t[start..].to_string()
    };
    for id in ["alpha-r1005", "r1006"] {
        let mut p = profile(id).unwrap().clone();
        // r1006 is retired by the r1007 cut (0.5.3); its render rule is what is tested here.
        p.retired = None;
        assert_eq!(p.node_checkpoint, None, "{id}");
        let net = p.ready().expect("ready");
        let out = render_node_network(template, &net);
        assert_eq!(section(&out), section(template), "{id}");
        assert!(!out.contains("vote_depth"), "{id}");
    }
    assert_eq!(
        r1007().node_checkpoint,
        Some(NodeCheckpoint { interval: 50, committee_size: 100, vote_depth: 3 }),
        "set by the r1007 cut"
    );
}

/// `nodeCheckpoint` is checked when it is read (an unknown key is a build defect) and when the
/// profile is readied (a zero value is refused).
#[test]
fn a_malformed_node_checkpoint_is_refused() {
    let with = |cp: serde_json::Value| {
        profiles_json_with_r1006_live(|v| v["profiles"]["r1006"]["nodeCheckpoint"] = cp)
    };
    let r1006_of = |json: String| -> Result<ReadyNetwork, ProfileError> {
        let (_, ps) = parse(&json)?;
        ps.into_iter().find(|p| p.id == "r1006").expect("r1006").ready()
    };
    let ok = r1006_of(with(serde_json::json!({"interval": 50, "committeeSize": 100, "voteDepth": 3})));
    assert!(
        matches!(&ok, Ok(r) if r.node_checkpoint == Some(NodeCheckpoint { interval: 50, committee_size: 100, vote_depth: 3 })),
        "{ok:?}"
    );
    for bad in [
        serde_json::json!({"interval": 0, "committeeSize": 100, "voteDepth": 3}),
        serde_json::json!({"interval": 50, "committeeSize": 0, "voteDepth": 3}),
        serde_json::json!({"interval": 50, "committeeSize": 100, "voteDepth": 0}),
    ] {
        let r = r1006_of(with(bad.clone()));
        assert!(
            matches!(&r, Err(ProfileError::Invalid { field: "nodeCheckpoint", .. })),
            "{bad}: {r:?}"
        );
    }
    for bad in [
        serde_json::json!({"interval": 50, "committeeSize": 100}),
        serde_json::json!({"interval": 50, "committeeSize": 100, "voteDepth": 3, "quorum": 67}),
        serde_json::json!({"interval": -1, "committeeSize": 100, "voteDepth": 3}),
    ] {
        assert!(
            matches!(r1006_of(with(bad.clone())), Err(ProfileError::Malformed(_))),
            "{bad}"
        );
    }
}

/// core#611: a template with no `[checkpoint]` (or no `[storage]`) table still gets the profile's
/// values. They used to be dropped silently, so the node would fall back to its defaults: the
/// thing `nodeCheckpoint` exists to prevent. The missing table is written at the end with every key.
#[test]
fn a_template_without_the_table_still_gets_the_checkpoint_and_retention_values() {
    let net = applied_r1007();
    let cp = net.node_checkpoint.expect("the cut sets [checkpoint]");
    let full = include_str!("../config/member-node.toml");
    let cut_table = |t: &str, table: &str| -> String {
        let start = t.find(&format!("\n[{table}]")).expect("the template has the table");
        let rest = &t[start + 1..];
        let end = rest[1..].find("\n[").map_or(rest.len(), |i| i + 2);
        format!("{}{}", &t[..start + 1], &rest[end..])
    };
    for table in ["checkpoint", "storage"] {
        let template = cut_table(full, table);
        assert!(!template.contains(&format!("[{table}]")), "{table} removed from the fixture");
        let cfg: toml::Table = render_node_network(&template, &net)
            .replace("{{DATA_DIR}}", "/tmp/x")
            .parse()
            .expect("valid TOML");
        let sec = cfg.get(table).and_then(|v| v.as_table()).unwrap_or_else(|| panic!("[{table}] written"));
        if table == "checkpoint" {
            assert_eq!(sec["interval"].as_integer(), Some(cp.interval as i64));
            assert_eq!(sec["committee_size"].as_integer(), Some(cp.committee_size as i64));
            assert_eq!(sec["vote_depth"].as_integer(), Some(cp.vote_depth as i64));
        } else {
            assert_eq!(sec["pruning"].as_bool(), Some(true));
            assert!(sec["keep_blocks"].as_integer().is_some());
        }
    }
}
