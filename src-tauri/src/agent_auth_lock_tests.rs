// L1-WP13a (planset v060): `release/agents.lock.json`, the pinned ACP agents. Every row names its
// version, license, source and a checksum per artifact with dated evidence; its id is an agent id
// with a home layout; nothing is installable until it is vetted and signed delivery exists; a row
// whose ACP support is not plain "supported", or whose license is not clearly redistributable, is
// never installable; launch arguments and fixed variables carry no credential.

use super::*;
use std::collections::BTreeSet;

const LOCK: &str = include_str!("../../release/agents.lock.json");

fn lock() -> serde_json::Value {
    serde_json::from_str(LOCK).expect("agents.lock.json parses")
}

fn agents() -> Vec<serde_json::Value> {
    lock()["agents"].as_array().expect("agents array").clone()
}

fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v[k].as_str()
        .unwrap_or_else(|| panic!("{k} is a string in {v}"))
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[test]
fn the_lock_is_dated_and_pins_the_acp_crate() {
    let l = lock();
    assert_eq!(l["version"], 1);
    let d = s(&l, "checked_at");
    assert!(
        d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-',
        "{d}"
    );
    let c = &l["evidence"]["acp_crate"];
    assert_eq!(s(c, "name"), "agent-client-protocol");
    assert_eq!(s(c, "version"), "5.6.1", "the lane's exact pin (WP02)");
    assert!(is_hex(s(c, "sha256"), 64));
    assert!(is_hex(s(&l["evidence"]["acp_registry"], "commit"), 40));
}

#[test]
fn every_row_is_pinned_with_a_license_a_source_and_a_checksum_per_artifact() {
    for a in agents() {
        let id = s(&a, "id");
        for k in [
            "label",
            "version",
            "license",
            "acp_evidence",
            "installable_reason",
        ] {
            assert!(!s(&a, k).trim().is_empty(), "{id}: {k}");
        }
        assert!(s(&a, "source").starts_with("https://"), "{id}: source");
        let arts = a["artifacts"].as_array().expect("artifacts");
        if id == "hermes" {
            assert!(
                is_hex(s(&a, "version"), 40),
                "Hermes is pinned to a source commit"
            );
        } else {
            assert!(!arts.is_empty(), "{id}: at least one artifact");
        }
        for art in arts {
            let url = s(art, "url");
            assert!(
                url.starts_with("https://") || url.starts_with("npm:"),
                "{id}: {url}"
            );
            let sha = art["sha256"].as_str().map(|h| is_hex(h, 64));
            let sri = art["npm_integrity"]
                .as_str()
                .map(|i| i.strip_prefix("sha512-").is_some_and(|b| b.len() == 88));
            assert!(
                sha == Some(true) || (sha.is_none() && sri == Some(true)),
                "{id}: {url} has a checksum"
            );
            assert!(
                sri != Some(false),
                "{id}: {url} npm_integrity is sha512 SRI"
            );
        }
    }
}

#[test]
fn every_id_is_an_agent_id_with_a_home_layout_and_every_layout_has_a_row() {
    let ids: BTreeSet<String> = agents().iter().map(|a| s(a, "id").to_string()).collect();
    assert_eq!(ids.len(), agents().len(), "ids are unique");
    for id in &ids {
        assert!(AgentId::parse(id).is_ok(), "{id}");
    }
    let layouts: BTreeSet<String> = LAYOUTS.iter().map(|l| l.id.to_string()).collect();
    assert_eq!(
        ids, layouts,
        "the lock and agent_auth::LAYOUTS name the same agents"
    );
    for want in [
        "hermes", "codex", "claude", "gemini", "goose", "grok", "muse",
    ] {
        assert!(ids.contains(want), "the F-2 agent {want} has a row");
    }
}

#[test]
fn nothing_is_installable_without_a_vetting_record_supported_acp_and_a_clear_license() {
    for a in agents() {
        let id = s(&a, "id");
        let vetted = s(&a, "vetted");
        assert!(
            ["vetting", "shipped", "refused"].contains(&vetted),
            "{id}: {vetted}"
        );
        if vetted == "shipped" {
            assert!(
                a["vetting_record"].as_str().is_some_and(|r| !r.is_empty()),
                "{id}: shipped needs its record"
            );
        }
        let acp = s(&a, "acp");
        assert!(
            ["supported", "experimental", "pending", "none"].contains(&acp),
            "{id}: {acp}"
        );
        let kind = s(&a, "license_kind");
        assert!(
            ["osi", "mixed", "proprietary", "unclear"].contains(&kind),
            "{id}: {kind}"
        );
        let wire = s(&a, "wire");
        assert!(
            ["chat", "responses", "messages", "native"].contains(&wire),
            "{id}: {wire}"
        );
        let installable = a["installable"].as_bool().expect("installable is a bool");
        if installable {
            assert_eq!(vetted, "shipped", "{id}: installable only once vetted");
            assert_eq!(
                acp, "supported",
                "{id}: installable only with plain ACP support"
            );
            assert!(
                kind == "osi" || kind == "mixed",
                "{id}: installable only with a clear license"
            );
        }
    }
}

#[test]
fn launch_arguments_and_fixed_variables_carry_no_credential() {
    for a in agents() {
        let id = s(&a, "id");
        let args: Vec<String> = a["launch"]["args"]
            .as_array()
            .expect("args")
            .iter()
            .map(|v| v.as_str().expect("arg").to_string())
            .collect();
        assert!(check_argv(&args).is_ok(), "{id}");
        let env: Vec<(String, String)> = a["launch"]["env"]
            .as_object()
            .expect("env")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("value").to_string()))
            .collect();
        assert!(check_env(&env).is_ok(), "{id}");
        assert!(
            !crate::telemetry::has_key_shape(&a.to_string()),
            "{id}: no key shape anywhere in the row"
        );
    }
}

#[test]
fn grok_build_and_muse_code_are_listed_but_not_installable() {
    for a in agents() {
        if ["grok", "muse"].contains(&s(&a, "id")) {
            assert_eq!(a["installable"], false);
            assert_eq!(s(&a, "license_kind"), "proprietary", "xAI and Meta terms");
        }
    }
}

// Owner decisions 2026-11-28: delivery per row, license conditions, vetting record ids, and the
// agents not offered in 0.6.0.

#[test]
fn every_row_names_its_delivery_and_its_vetting_record() {
    for a in agents() {
        let id = s(&a, "id");
        let delivery = s(&a, "delivery");
        assert!(
            [
                "bundled",
                "signed_component",
                "vendor_download",
                "vendor_binary"
            ]
            .contains(&delivery),
            "{id}: {delivery}"
        );
        assert_eq!(
            s(&a, "vetting_record"),
            format!("VET-AGT-2026-11-28-{id}"),
            "{id}: the citrate-security record id"
        );
    }
}

#[test]
fn every_row_without_a_plain_osi_license_states_its_license_condition() {
    for a in agents() {
        let id = s(&a, "id");
        if s(&a, "license_kind") != "osi" {
            let c = a["license_condition"].as_str().unwrap_or_default();
            assert!(!c.trim().is_empty(), "{id}: license_condition");
        }
    }
}

#[test]
fn muse_code_is_a_vendor_download_and_grok_build_the_vendor_binary_without_its_postinstall() {
    let rows = agents();
    let row = |id: &str| {
        rows.iter()
            .find(|a| s(a, "id") == id)
            .cloned()
            .expect("row")
    };
    let muse = row("muse");
    assert_eq!(
        s(&muse, "delivery"),
        "vendor_download",
        "Citrate does not redistribute Muse Code"
    );
    assert!(s(&muse, "license_condition").contains("does not redistribute"));
    let grok = row("grok");
    assert_eq!(
        s(&grok, "delivery"),
        "vendor_binary",
        "xAI's published binary, not a build"
    );
    assert!(s(&grok, "license_condition").contains("never runs the wrapper package's postinstall"));
    for a in [&muse, &grok] {
        assert_eq!(
            a["installable"], false,
            "waits on vetting and the signing ceremony"
        );
    }
}

#[test]
fn refused_and_deferred_agents_are_never_offered() {
    let l = lock();
    let not_offered = l["not_offered"].as_array().expect("not_offered");
    let ids: BTreeSet<String> = agents().iter().map(|a| s(a, "id").to_string()).collect();
    let mut seen = BTreeSet::new();
    for n in not_offered {
        let id = s(n, "id");
        assert!(
            ["refused", "not_in_0.6.0"].contains(&s(n, "decision")),
            "{id}"
        );
        assert!(!ids.contains(id), "{id} has no agent row");
        assert!(
            LAYOUTS.iter().all(|x| x.id != id),
            "{id} has no home layout"
        );
        seen.insert(id.to_string());
    }
    for want in ["openclaw", "oh-my-opencode", "opencode", "cline"] {
        assert!(seen.contains(want), "{want} is recorded");
    }
}
