//! L0-WP09: `release/flags.json` agrees with the `// @flag` consts in the code, and a flag that
//! needs a sign-off is `true` only with a verified record (`signoff::gate`).
//!
//! Gherkin (L0_INTEGRATION_SEAMS 5, L0-WP09):
//!   Scenario: a sign-off flag is flipped without a record
//!     Given HERMES_NODE_WRITE_TOOLS is set to true in hermes_mcp.rs
//!     And release/signoffs/HERMES_NODE_WRITE_TOOLS.json does not verify
//!     When cargo test runs
//!     Then flags_requiring_signoff_have_a_verified_record fails

use crate::flags::{self, Table};
use crate::signoff::{self, Spec};
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Every Rust file under `src-tauri/src/`, as (repo-relative path, text).
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("read src dir")
            .map(|e| e.expect("dir entry").path())
            .collect();
        entries.sort();
        for p in entries {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .expect("utf-8 name")
                .to_string();
            let rel = format!("{rel}/{name}");
            if p.is_dir() {
                walk(&p, &rel, out);
            } else if name.ends_with(".rs") {
                out.push((rel, std::fs::read_to_string(&p).expect("read source")));
            }
        }
    }
    let mut out = Vec::new();
    walk(
        &repo_root().join("src-tauri/src"),
        "src-tauri/src",
        &mut out,
    );
    assert!(
        out.len() > 150,
        "found {} Rust files; the walk is broken",
        out.len()
    );
    out
}

fn table() -> Table {
    flags::load().expect("release/flags.json parses")
}

fn read_repo(rel: &str) -> Option<Vec<u8>> {
    std::fs::read(repo_root().join(rel)).ok()
}

/// The problems of one kind on the real tree.
fn real_mismatches(kind: &str) -> Vec<String> {
    flags::mismatches(&table(), &sources())
        .into_iter()
        .filter(|m| m.contains(kind))
        .collect()
}

#[test]
fn every_marked_const_has_a_row() {
    let problems = real_mismatches("has no row");
    assert!(problems.is_empty(), "{problems:#?}");
    let marked: usize = sources()
        .iter()
        .map(|(f, s)| {
            flags::marked_consts(f, s)
                .expect("well-formed markers")
                .len()
        })
        .sum();
    assert_eq!(marked, table().flags.len(), "one marked const per row");
}

#[test]
fn every_row_points_at_a_marked_const() {
    let problems = real_mismatches("points at no");
    assert!(problems.is_empty(), "{problems:#?}");
    let all = flags::mismatches(&table(), &sources());
    assert!(all.is_empty(), "{all:#?}");
}

#[test]
fn defaults_match_the_code() {
    let problems = real_mismatches(" says ");
    assert!(problems.is_empty(), "{problems:#?}");
    // The compiled values, so a parser that reads the wrong line cannot pass.
    let compiled = [
        (
            "HERMES_NODE_WRITE_TOOLS",
            crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS,
        ),
        (
            "TRANSPORT_SIGNED_OFF",
            crate::cluster_mesh::TRANSPORT_SIGNED_OFF,
        ),
        (
            "MULTI_GROUP_DAEMON",
            crate::cluster_mesh::MULTI_GROUP_DAEMON,
        ),
    ];
    let t = table();
    for (name, value) in compiled {
        let row = t.flags.iter().find(|r| r.name == name).expect("a row");
        assert_eq!(row.default, value, "{name}");
    }
}

#[test]
fn flags_requiring_signoff_have_a_verified_record() {
    let t = table();
    let gated: Vec<_> = t.flags.iter().filter(|r| r.signoff.is_some()).collect();
    assert!(!gated.is_empty(), "no sign-off rows; the table is broken");
    for row in gated {
        let Some(s) = &row.signoff else { continue };
        let spec = signoff::spec(&s.id).expect("spec from the table");
        if let Err(e) = signoff::gate(&spec, row.default, &read_repo) {
            panic!(
                "{} is {} but its sign-off {} does not hold: {e}",
                row.name, row.default, s.id
            );
        }
    }
}

// --- Mutation proofs: each check fails on the broken input it exists for. ---

const SOURCE: &str =
    "/// doc\n// @flag\npub const ALPHA: bool = false;\n#[test]\nfn alpha_gate() {}\n";

fn table_of(rows: serde_json::Value) -> Table {
    flags::parse(&serde_json::json!({ "$comment": ["test"], "flags": rows }).to_string())
        .expect("test table")
}

fn alpha_row(default: bool) -> serde_json::Value {
    serde_json::json!({
        "name": "ALPHA", "file": "src-tauri/src/alpha.rs", "lane": "l1", "default": default,
        "signoff": null, "gate": "alpha_gate", "summary": "turns alpha on",
    })
}

#[test]
fn table_and_code_disagreements_are_named() {
    let src = vec![("src-tauri/src/alpha.rs".to_string(), SOURCE.to_string())];
    assert!(flags::mismatches(&table_of(serde_json::json!([alpha_row(false)])), &src).is_empty());
    let flipped = flags::mismatches(&table_of(serde_json::json!([alpha_row(true)])), &src);
    assert!(
        flipped.iter().any(|m| m.contains("says true")),
        "{flipped:?}"
    );
    let no_row = flags::mismatches(&table_of(serde_json::json!([])), &src);
    assert!(
        no_row.iter().any(|m| m.contains("ALPHA is marked")),
        "{no_row:?}"
    );
    let unmarked = vec![(
        "src-tauri/src/alpha.rs".to_string(),
        SOURCE.replace("// @flag\n", ""),
    )];
    let no_const = flags::mismatches(&table_of(serde_json::json!([alpha_row(false)])), &unmarked);
    assert!(
        no_const.iter().any(|m| m.contains("points at no")),
        "{no_const:?}"
    );
    let no_gate = vec![(
        "src-tauri/src/alpha.rs".to_string(),
        SOURCE.replace("alpha_gate", "x"),
    )];
    let gate = flags::mismatches(&table_of(serde_json::json!([alpha_row(false)])), &no_gate);
    assert!(
        gate.iter().any(|m| m.contains("gate test alpha_gate")),
        "{gate:?}"
    );
}

#[test]
fn a_marker_must_sit_on_one_bool_literal_const() {
    let f = "src-tauri/src/alpha.rs";
    assert_eq!(flags::marked_consts(f, SOURCE).expect("ok").len(), 1);
    let gap = SOURCE.replace("// @flag\n", "// @flag\n\n");
    assert!(
        flags::marked_consts(f, &gap).is_err(),
        "blank line under the marker"
    );
    let expr = SOURCE.replace("= false;", "= cfg!(test);");
    assert!(flags::marked_consts(f, &expr).is_err(), "not a literal");
    let twin = format!("{SOURCE}#[cfg(feature = \"x\")]\npub const ALPHA: bool = true;\n");
    assert!(flags::marked_consts(f, &twin).is_err(), "a cfg twin");
}

#[test]
fn a_malformed_table_is_refused() {
    let bad = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut row = alpha_row(false);
        edit(&mut row);
        let text = serde_json::json!({ "$comment": ["t"], "flags": [row] }).to_string();
        flags::parse(&text).is_err()
    };
    assert!(!bad(&|_| {}), "the good row parses");
    assert!(
        bad(&|r| r["gate"] = serde_json::Value::Null),
        "no gate and no sign-off"
    );
    assert!(
        bad(&|r| r["file"] = "src-tauri/src/../x.rs".into()),
        "a .. path"
    );
    assert!(
        bad(&|r| r["file"] = "kit/src/alpha.rs".into()),
        "outside src-tauri/src"
    );
    assert!(bad(&|r| r["lane"] = "l10".into()), "unknown lane");
    assert!(bad(&|r| r["extra"] = true.into()), "unknown field");
    assert!(
        bad(&|r| r["signoff"] =
            serde_json::json!({"id": "../x", "packet_repo": "CitrateNetwork/citrate-core"})),
        "id is a path"
    );
    assert!(
        bad(&|r| r["signoff"] = serde_json::json!({"id": "X", "packet_repo": "someone/else"})),
        "foreign packet repo"
    );
    let dup =
        serde_json::json!({ "$comment": ["t"], "flags": [alpha_row(false), alpha_row(false)] });
    assert!(
        flags::parse(&dup.to_string()).is_err(),
        "a name listed twice"
    );
    let silent = serde_json::json!({ "$comment": [" "], "flags": [alpha_row(false)] });
    assert!(
        flags::parse(&silent.to_string()).is_err(),
        "a table that does not say what it is"
    );
}

/// The Gherkin above on a scratch tree: a flipped flag fails without a verified record.
#[test]
fn a_flipped_flag_without_a_verified_record_fails() {
    use sha2::{Digest, Sha256};
    let spec = Spec {
        id: "HERMES_NODE_WRITE_TOOLS".into(),
        packet_repo: "CitrateNetwork/citrate-core".into(),
    };
    let (owner_key, sec_key) = ("11".repeat(32), "22".repeat(32));
    let addr = |k: &str| crate::comms::address_from_secret_hex(k).expect("address");
    let packet = b"# packet\nhash of the private review\n".to_vec();
    let pk = serde_json::json!({
        "repo": "CitrateNetwork/citrate-core", "path": "release/signoffs/HERMES_NODE_WRITE_TOOLS_SIGNOFF_PACKET.md",
        "rev": "694e990c".repeat(5), "sha256": hex::encode(Sha256::digest(&packet)),
    });
    let signer = |role: &str, gh: &str, key: &str| {
        let msg = signoff::signing_message(&spec.id, role, gh, "2026-11-28", &pk);
        let sig = crate::device_link::sign_eip191(key, &msg).expect("sign");
        serde_json::json!({ "role": role, "github": gh, "date": "2026-11-28", "sig": sig })
    };
    let record = serde_json::json!({
        "wp": spec.id, "packet": pk,
        "signers": [signer("owner", "example-owner", &owner_key), signer("security-lead", "example-sec", &sec_key)],
    })
    .to_string();
    let signers = serde_json::json!({ "owner": addr(&owner_key), "security-lead": addr(&sec_key) })
        .to_string();
    let tree = |files: Vec<(String, Vec<u8>)>| {
        move |rel: &str| files.iter().find(|(p, _)| p == rel).map(|(_, b)| b.clone())
    };
    let full = || {
        vec![
            (signoff::record_path(&spec.id), record.clone().into_bytes()),
            (signoff::packet_path(&spec.id), packet.clone()),
            (signoff::SIGNERS.to_string(), signers.clone().into_bytes()),
        ]
    };
    assert!(
        signoff::gate(&spec, true, &tree(full())).is_ok(),
        "verified record"
    );
    assert!(
        signoff::gate(&spec, false, &tree(vec![])).is_ok(),
        "off, no record"
    );
    let missing = signoff::gate(&spec, true, &tree(vec![])).expect_err("on, no record");
    assert!(
        missing.contains("HERMES_NODE_WRITE_TOOLS.json is missing"),
        "{missing}"
    );
    let mut wrong_packet = full();
    wrong_packet[1].1 = b"another packet".to_vec();
    assert!(
        signoff::gate(&spec, true, &tree(wrong_packet)).is_err(),
        "packet changed"
    );
    let other =
        serde_json::json!({ "owner": addr(&"33".repeat(32)), "security-lead": addr(&sec_key) });
    let mut unregistered = full();
    unregistered[2].1 = other.to_string().into_bytes();
    assert!(
        signoff::gate(&spec, true, &tree(unregistered)).is_err(),
        "unregistered owner"
    );
    let early = vec![(signoff::record_path(&spec.id), b"{}".to_vec())];
    assert!(
        signoff::gate(&spec, false, &tree(early)).is_err(),
        "off, malformed early record"
    );
    let cl = Spec {
        id: "CL-S4".into(),
        packet_repo: "CitrateNetwork/citrate-cluster".into(),
    };
    assert!(
        signoff::validate(&cl, &record).is_err(),
        "a record for another sign-off"
    );
}
