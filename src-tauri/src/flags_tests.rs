//! L0-WP09: `release/flags.json` agrees with the `// @flag` consts in the code, and a flag that
//! needs a sign-off is `true` only with a verified record (`signoff::gate`).
//!
//! Gherkin (L0_INTEGRATION_SEAMS 5, L0-WP09):
//!   Scenario: a sign-off flag is flipped without a record
//!     Given HERMES_NODE_WRITE_TOOLS is set to true in hermes_mcp.rs
//!     And release/signoffs/HERMES_NODE_WRITE_TOOLS.json does not verify
//!     When cargo test runs
//!     Then flags_requiring_signoff_have_a_verified_record fails

use crate::flags::{self, Signoff, Table};
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
    let rust_rows = table().flags.iter().filter(|r| !r.table_only()).count();
    assert_eq!(marked, rust_rows, "one marked const per Rust row");
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
    for (id, value) in compiled {
        let row = t.flags.iter().find(|r| r.id == id).expect("a row");
        assert_eq!(row.default, value, "{id}");
    }
    let rust_rows = t.flags.iter().filter(|r| !r.table_only()).count();
    assert_eq!(rust_rows, compiled.len(), "a new Rust flag joins this list");
}

#[test]
fn flags_requiring_signoff_have_a_verified_record() {
    let t = table();
    let gated: Vec<_> = t
        .flags
        .iter()
        .filter(|r| r.signoff == Signoff::Required)
        .collect();
    assert!(!gated.is_empty(), "no sign-off rows; the table is broken");
    for row in gated {
        let id = row.signoff_id().expect("a required row has a sign-off id");
        let spec = signoff::spec(&id).expect("spec from the table");
        if let Err(e) = signoff::gate(&spec, row.default, &read_repo) {
            panic!(
                "{} is {} but its sign-off {id} does not hold: {e}",
                row.id, row.default
            );
        }
    }
}

/// The 3.7 initial rows, as the lane file lists them.
#[test]
fn the_initial_rows_are_the_spec_rows() {
    let t = table();
    assert_eq!(t.version, 1);
    let rows: Vec<_> = t
        .flags
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.lane.as_str(),
                r.where_.as_str(),
                r.default,
                r.signoff,
                r.record_path(),
            )
        })
        .collect();
    let rec = |id: &str| Some(format!("release/signoffs/{id}.json"));
    assert_eq!(
        rows,
        vec![
            (
                "HERMES_NODE_WRITE_TOOLS",
                "L1",
                "src-tauri/src/hermes_mcp.rs",
                false,
                Signoff::Required,
                rec("HERMES_NODE_WRITE_TOOLS")
            ),
            (
                "TRANSPORT_SIGNED_OFF",
                "L7",
                "src-tauri/src/cluster_mesh.rs",
                false,
                Signoff::Required,
                rec("CL-S4")
            ),
            (
                "MULTI_GROUP_DAEMON",
                "L7",
                "src-tauri/src/cluster_mesh.rs",
                true,
                Signoff::NotNeeded,
                None
            ),
            (
                "PQC_CLAIMS_ALLOWED",
                "L9",
                flags::TABLE_PATH,
                false,
                Signoff::Required,
                rec("PQC_CLAIMS_ALLOWED")
            ),
        ]
    );
    // CL-S4's packet stays in citrate-cluster; the table cannot move it.
    let cl = signoff::spec("CL-S4").expect("CL-S4 is in the table");
    assert_eq!(cl.packet_repo, "CitrateNetwork/citrate-cluster");
    let hermes = signoff::spec("HERMES_NODE_WRITE_TOOLS").expect("in the table");
    assert_eq!(hermes.packet_repo, "CitrateNetwork/citrate-core");
    assert!(signoff::spec("NOT_A_SIGNOFF").is_err());
}

// --- Mutation proofs: each check fails on the broken input it exists for. ---

const SOURCE: &str =
    "/// doc\n// @flag ALPHA\npub const ALPHA: bool = false;\n#[test]\nfn alpha_gate() {}\n";

fn table_of(rows: serde_json::Value) -> Table {
    flags::parse(&serde_json::json!({ "version": 1, "flags": rows }).to_string())
        .expect("test table")
}

fn alpha_row(default: bool) -> serde_json::Value {
    serde_json::json!({
        "id": "ALPHA", "lane": "L1", "where": "src-tauri/src/alpha.rs", "default": default,
        "signoff": "none",
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
        SOURCE.replace("// @flag ALPHA\n", ""),
    )];
    let no_const = flags::mismatches(&table_of(serde_json::json!([alpha_row(false)])), &unmarked);
    assert!(
        no_const.iter().any(|m| m.contains("points at no")),
        "{no_const:?}"
    );
    // A table-only row needs no const, and a marked const cannot hide behind one.
    let mut only = alpha_row(false);
    only["where"] = flags::TABLE_PATH.into();
    assert!(flags::mismatches(&table_of(serde_json::json!([only.clone()])), &unmarked).is_empty());
    let hidden = flags::mismatches(&table_of(serde_json::json!([only])), &src);
    assert!(
        hidden.iter().any(|m| m.contains("ALPHA is marked")),
        "{hidden:?}"
    );
}

#[test]
fn a_marker_must_sit_on_one_bool_literal_const() {
    let f = "src-tauri/src/alpha.rs";
    assert_eq!(flags::marked_consts(f, SOURCE).expect("ok").len(), 1);
    let gap = SOURCE.replace("// @flag ALPHA\n", "// @flag ALPHA\n\n");
    assert!(
        flags::marked_consts(f, &gap).is_err(),
        "blank line under the marker"
    );
    let expr = SOURCE.replace("= false;", "= cfg!(test);");
    assert!(flags::marked_consts(f, &expr).is_err(), "not a literal");
    let twin = format!("{SOURCE}#[cfg(feature = \"x\")]\npub const ALPHA: bool = true;\n");
    assert!(flags::marked_consts(f, &twin).is_err(), "a cfg twin");
    // Prose that quotes the declaration is not a twin.
    let prose = format!("{SOURCE}// was `pub const ALPHA: bool = true;` before\n");
    assert_eq!(flags::marked_consts(f, &prose).expect("ok").len(), 1);
}

#[test]
fn a_marker_must_name_its_flag() {
    let f = "src-tauri/src/alpha.rs";
    for bad in [
        "// @flag",
        "// @flag BETA",
        "//@flag ALPHA",
        "//  @flag ALPHA",
        "// @flag  ALPHA",
        "// @flag alpha",
        "// @flag ALPHA extra",
    ] {
        let src = SOURCE.replace("// @flag ALPHA", bad);
        assert!(flags::marked_consts(f, &src).is_err(), "{bad:?} is refused");
    }
    // Doc comments may mention the marker; they are not markers.
    let doc = format!("/// a `// @flag <ID>` line marks a flag\n//! @flag X\n{SOURCE}");
    assert_eq!(flags::marked_consts(f, &doc).expect("ok").len(), 1);
}

#[test]
fn a_malformed_table_is_refused() {
    let bad = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut row = alpha_row(false);
        edit(&mut row);
        let text = serde_json::json!({ "version": 1, "flags": [row] }).to_string();
        flags::parse(&text).is_err()
    };
    assert!(!bad(&|_| {}), "the good row parses");
    assert!(
        !bad(&|r| r["signoff"] = "required".into()),
        "required with the default record"
    );
    assert!(
        bad(&|r| r["where"] = "src-tauri/src/../x.rs".into()),
        "a .. path"
    );
    assert!(
        bad(&|r| r["where"] = "kit/src/alpha.rs".into()),
        "outside src-tauri/src"
    );
    assert!(
        bad(&|r| r["where"] = "scripts/cx-copy-lint.sh".into()),
        "a script is not a where; table-only flags use the table"
    );
    assert!(bad(&|r| r["lane"] = "l1".into()), "lowercase lane");
    assert!(bad(&|r| r["lane"] = "L10".into()), "unknown lane");
    assert!(bad(&|r| r["extra"] = true.into()), "unknown field");
    assert!(bad(&|r| r["signoff"] = "maybe".into()), "unknown signoff");
    assert!(bad(&|r| r["signoff"] = true.into()), "signoff is a word");
    assert!(
        bad(&|r| r["record"] = "release/signoffs/ALPHA.json".into()),
        "a record on a none row"
    );
    let required = |record: &str| {
        move |r: &mut serde_json::Value| {
            r["signoff"] = "required".into();
            r["record"] = record.into();
        }
    };
    assert!(!bad(&required("release/signoffs/CL-S9.json")), "a named record");
    assert!(bad(&required("release/signoffs/../x.json")), "record is a path");
    assert!(bad(&required("release/signoffs/a/b.json")), "a nested record");
    assert!(bad(&required("release/x.json")), "outside release/signoffs");
    assert!(bad(&required("release/signoffs/X.md")), "not a .json");
    let table = |rows: serde_json::Value, version: u32| {
        flags::parse(&serde_json::json!({ "version": version, "flags": rows }).to_string())
    };
    assert!(
        table(serde_json::json!([alpha_row(false)]), 2).is_err(),
        "version 2"
    );
    assert!(
        flags::parse(&serde_json::json!({ "flags": [] }).to_string()).is_err(),
        "no version"
    );
    assert!(
        table(serde_json::json!([alpha_row(false), alpha_row(false)]), 1).is_err(),
        "an id listed twice"
    );
    let mut beta = alpha_row(false);
    beta["id"] = "BETA".into();
    let shared = |mut r: serde_json::Value| {
        r["signoff"] = "required".into();
        r["record"] = "release/signoffs/SHARED.json".into();
        r
    };
    assert!(
        table(serde_json::json!([shared(alpha_row(false)), shared(beta)]), 1).is_err(),
        "two flags on one record"
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
