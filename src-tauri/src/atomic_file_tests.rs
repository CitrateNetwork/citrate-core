// #420: the unique-temp-name writer, and the tripwire that keeps fixed temp names (a temp file
// shared by every concurrent writer of the same file) out of production code in src-tauri/src.

use super::*;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-atomic-file-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp dir");
    d
}

#[test]
fn temp_paths_are_unique_and_beside_the_target() {
    let p = Path::new("/data/escalation/route.json");
    let a = unique_temp_path(p);
    let b = unique_temp_path(p);
    assert_ne!(a, b, "two writes never share a temp name");
    assert_eq!(a.parent(), p.parent(), "the temp file is in the target's directory");
    let name = a.file_name().expect("name").to_string_lossy().to_string();
    assert!(name.starts_with("route.json.tmp-"), "{name}");
}

#[test]
fn concurrent_replaces_land_one_whole_value_and_leave_no_temp_file() {
    let d = tmp_dir("concurrent");
    let target = d.join("value.json");
    std::thread::scope(|s| {
        for t in 0..8u8 {
            let target = target.clone();
            s.spawn(move || {
                for _ in 0..30 {
                    replace(&target, &[b'a' + t; 64]).expect("replace");
                }
            });
        }
    });
    let body = std::fs::read(&target).expect("target");
    assert_eq!(body.len(), 64, "a whole value, never a cut one");
    assert!(body.iter().all(|b| *b == body[0]), "one writer's value, never a mix");
    let names: Vec<String> = std::fs::read_dir(&d)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["value.json".to_string()], "no temp file is left");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_failed_replace_keeps_the_target_and_removes_its_temp() {
    let d = tmp_dir("failed");
    // The target is a non-empty directory, so the rename over it fails after the temp is written.
    let target = d.join("value.json");
    std::fs::create_dir_all(target.join("inner")).expect("seed");
    assert!(replace(&target, b"new").is_err(), "the rename over a directory fails");
    assert!(target.join("inner").is_dir(), "the target is untouched");
    let names: Vec<String> = std::fs::read_dir(&d)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec!["value.json".to_string()], "the temp file is removed");
    let _ = std::fs::remove_dir_all(d);
}

// ---------------------------------------------------------------------------
// Tripwire: fixed temp-file names in production code.
// ---------------------------------------------------------------------------

/// The production part of a source file: comment lines are dropped, and everything from a
/// `#[cfg(test)]` that opens a `mod` onwards is test code.
fn production_part(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == "#[cfg(test)]" {
            let next = lines[i + 1..].iter().find(|x| !x.trim().is_empty());
            if next.is_some_and(|x| x.trim_start().starts_with("mod ")) {
                break;
            }
        }
        if l.trim_start().starts_with("//") {
            continue;
        }
        out.push(*l);
    }
    out.join("\n")
}

/// How many fixed temp names the production part of `src` builds: a string literal ending in
/// `.tmp` (`"json.tmp"`, `"{FILE}.tmp"`, `"pins.json.tmp"`) or `with_extension("tmp")`. A name
/// with anything unique after `.tmp` (`"{name}.tmp-{pid}-{n}"`) is not counted.
fn fixed_temp_sites(src: &str) -> usize {
    let p = production_part(src);
    p.matches(".tmp\"").count() + p.matches("(\"tmp\")").count()
}

/// Known fixed temp names on main when the tripwire landed, with the count per file and why each
/// is allowed. A file may go down (fix one, then lower or drop its entry), never up. New code
/// writes through [`replace`] (or a unique name under a lock).
const KNOWN_FIXED_TEMP_NAMES: &[(&str, usize, &str)] = &[
    ("agent_grants.rs", 2, "save + restore_raw run only under the static store_lock"),
    ("chain_agent.rs", 1, "InFlightAnchors::save runs under its map Mutex on the process singleton"),
    ("daemons.rs", 1, "store_book runs only under the static BOOK_LOCK"),
    ("device_link.rs", 1, "unguarded: DeviceLinkStore::save from several commands; known, follow-up"),
    ("device_link_share.rs", 1, "unguarded: PeerLinkStore::save from device_links_ingest; known, follow-up"),
    ("escalation.rs", 1, "write_atomic runs only inside with_book under the EscalationState Mutex"),
    ("fl_rounds.rs", 1, "write_atomic runs under FlRounds.inner / consent_lock on the managed instance"),
    ("fl_trajectories.rs", 1, "unguarded: settings save from trajectories_settings_set; known, follow-up"),
    ("fleet.rs", 1, "unguarded: save_roster from pairing thread and commands; known, follow-up"),
    ("forge_toolchain.rs", 1, "unguarded: settings save from toolchain_settings_set; known, follow-up"),
    ("hermes_learn.rs", 1, "Ledger::save runs only inside with_ledger under the static LEDGER_LOCK"),
    ("hermes_mcp.rs", 1, "write_private: the effective allowlist is written under the Hermes start lock; config/settings saves are unguarded; known, follow-up"),
    ("hermes_schedule.rs", 1, "unguarded: ScheduleStore built per command; known, follow-up"),
    ("hermes_web.rs", 1, "unguarded: settings save from hermes_web_settings_set; known, follow-up"),
    ("hic_records.rs", 1, "HicOutbox::write_atomic runs under its guard() on the process singleton"),
    ("knowledge_import.rs", 2, "progress + marker writes run only while the per-store IN_PROGRESS import slot is held"),
    ("local_data.rs", 1, "a name in the list of wallet files to remove, not a writer"),
    ("media.rs", 1, "unguarded: GalleryStore::record_item, store built per call; known, follow-up"),
    ("memory.rs", 1, "unguarded: save_seeded_chunks from memory_ingest_docs; known, follow-up"),
    ("node_genesis.rs", 1, "durable_tmp: write_durable (genesis marker, K6 migration intent) runs under the NodeManager.sup Mutex in start(); clear_intent removes the same leftover"),
    ("node_mcp_token.rs", 1, "write_private: tokens and faucet saves run under their store locks; node MCP save_config is unguarded; known, follow-up"),
    ("owned_browser.rs", 1, "unguarded: the child-watch thread can race start(); known, follow-up"),
    ("storage.rs", 1, "pins.json: replaced by a unique name under PIN_INDEX_LOCK in the #461 fix (fix/pin-index-writers)"),
    ("store_catalog.rs", 1, "write_atomic for state/catalog/sig files runs under the static STORE_LOCK"),
    ("widgets.rs", 1, "save_in holds the static STORE_LOCK; the name is fixed per widget id"),
];

#[test]
fn tripwire_the_scanner_flags_fixed_temp_names_and_skips_unique_ones() {
    // Negative control: each fixed form is flagged ...
    assert_eq!(fixed_temp_sites(r#"let t = p.with_extension("json.tmp");"#), 1);
    assert_eq!(fixed_temp_sites(r#"let t = p.with_extension("tmp");"#), 1);
    assert_eq!(fixed_temp_sites(r#"let t = d.join(format!("{FILE}.tmp"));"#), 1);
    assert_eq!(fixed_temp_sites(r#"let t = p.with_file_name("pins.json.tmp");"#), 1);
    // ... and a unique name, a comment, or test code is not.
    assert_eq!(fixed_temp_sites(r#"format!("{name}.tmp-{}-{n}", std::process::id())"#), 0);
    assert_eq!(fixed_temp_sites("// was: d.join(\"route.json.tmp\")"), 0);
    let with_tests = "fn a() {}\n#[cfg(test)]\nmod tests {\n    let t = p.with_extension(\"tmp\");\n}\n";
    assert_eq!(fixed_temp_sites(with_tests), 0);
}

#[test]
fn tripwire_no_new_fixed_temp_names_in_production_code() {
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut scanned = 0usize;
    let mut over: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&src_dir).expect("src dir") {
        let path = entry.expect("entry").path();
        let name = path.file_name().expect("name").to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        scanned += 1;
        let found = fixed_temp_sites(&std::fs::read_to_string(&path).expect("read"));
        let allowed = KNOWN_FIXED_TEMP_NAMES
            .iter()
            .find(|(f, _, _)| *f == name)
            .map_or(0, |(_, n, _)| *n);
        if found > allowed {
            over.push(format!("{name}: {found} fixed temp name(s), {allowed} allowed"));
        }
    }
    assert!(scanned > 100, "the scan reached src-tauri/src ({scanned} files)");
    assert!(
        over.is_empty(),
        "a temp file with a fixed name is shared by concurrent writers (#420, #461); write \
         through atomic_file::replace or a unique name under a lock: {over:?}"
    );
}

#[test]
fn tripwire_the_fixed_settings_writers_stay_fixed() {
    // #420: these two writers were fixed in this change and are not on the known list.
    for f in ["chat_route.rs", "hermes_terminal.rs"] {
        assert!(
            KNOWN_FIXED_TEMP_NAMES.iter().all(|(n, _, _)| *n != f),
            "{f} must not be allowlisted"
        );
    }
}
