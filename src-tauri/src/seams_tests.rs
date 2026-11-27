//! L0-WP02 SEAM-CMDS (planset v060): the command registration, the main-window ACL and the
//! lib.rs module set, recorded at a826e441 in `tests/fixtures/seams/`.
//!
//! L0-WP03 splits `generate_handler!` into per-lane lists and L0-WP04 moves the module tree; both
//! must keep these snapshots byte-identical. A lane PR that drops a command fails here by name.
//! A fixture changes only in an L0-reviewed PR that says why; record it again with
//! `node scripts/seams-snapshot.mjs` (`SEAMS_SNAPSHOT_WRITE=1`). A missing fixture fails and prints
//! `SEAMS_SNAPSHOT_MISSING <path> <hex>` with the exact bytes it would record.

use std::collections::BTreeSet;
use std::path::PathBuf;

const BASE: &str = "a826e441";
const LIB_RS: &str = include_str!("lib.rs");
const MAIN_WINDOW_TOML: &str = include_str!("../permissions/main-window.toml");
const MAIN_ID: &str = "identifier = \"main-window-commands\"";

/// The command paths inside `generate_handler![...]`, sorted, with any `crate::` prefix dropped.
fn registered_paths(src: &str) -> Vec<String> {
    let open = "generate_handler![";
    let Some(at) = src.find(open) else {
        return Vec::new();
    };
    let start = at + open.len();
    let Some(len) = src[start..].find("])") else {
        return Vec::new();
    };
    let mut out: Vec<String> = src[start..start + len]
        .lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|s| s.contains("::"))
        .map(|s| s.trim_start_matches("crate::").to_string())
        .collect();
    out.sort();
    out
}

/// The command names (the last path segment): what the webview invokes and the ACL lists.
fn command_names(paths: &[String]) -> BTreeSet<String> {
    paths
        .iter()
        .filter_map(|p| p.rsplit("::").next().map(str::to_string))
        .collect()
}

/// The commands a permission file allows (`commands.allow = [...]`), in file order.
fn acl_commands(toml: &str) -> Vec<String> {
    let open = "commands.allow = [";
    let Some(at) = toml.find(open) else {
        return Vec::new();
    };
    let start = at + open.len();
    let Some(len) = toml[start..].find(']') else {
        return Vec::new();
    };
    toml[start..start + len]
        .lines()
        .map(|l| l.split('#').next().unwrap_or_default())
        .flat_map(|l| {
            l.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Every top-level `mod` declaration, with the attributes written on the lines above it, sorted.
fn lib_mods(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut attrs: Vec<&str> = Vec::new();
    for line in src.lines() {
        let line = line.trim_end();
        if line.starts_with("#[") {
            attrs.push(line);
            continue;
        }
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let decl = line
            .strip_prefix("pub(crate) ")
            .or_else(|| line.strip_prefix("pub "))
            .unwrap_or(line);
        let is_mod = decl
            .strip_prefix("mod ")
            .is_some_and(|rest| rest.ends_with(';') || rest.ends_with('{'));
        if is_mod {
            let mut entry = attrs.join(" ");
            if !entry.is_empty() {
                entry.push(' ');
            }
            entry.push_str(line);
            out.push(entry);
        }
        attrs.clear();
    }
    out.sort();
    out
}

/// Lines in `want` and not in `got` (missing), and in `got` and not in `want` (added).
fn line_diff(want: &[String], got: &[String]) -> (Vec<String>, Vec<String>) {
    let w: BTreeSet<&String> = want.iter().collect();
    let g: BTreeSet<&String> = got.iter().collect();
    let missing = w.difference(&g).map(|s| s.to_string()).collect();
    let added = g.difference(&w).map(|s| s.to_string()).collect();
    (missing, added)
}

fn fixture(name: &str) -> (String, PathBuf) {
    let rel = format!("src-tauri/tests/fixtures/seams/{name}.{BASE}.txt");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/seams")
        .join(format!("{name}.{BASE}.txt"));
    (rel, path)
}

/// Compare `got` with the recorded fixture `name`, byte for byte, naming every changed line.
fn check_snapshot(name: &str, got: &[String]) {
    let (rel, path) = fixture(name);
    let body = format!("{}\n", got.join("\n"));
    if std::env::var("SEAMS_SNAPSHOT_WRITE").as_deref() == Ok("1") {
        let dir = path.parent().expect("fixture dir");
        std::fs::create_dir_all(dir).expect("create fixture dir");
        std::fs::write(&path, &body).expect("write fixture");
        return;
    }
    let Ok(raw) = std::fs::read_to_string(&path) else {
        let hex: String = body.bytes().map(|b| format!("{b:02x}")).collect();
        println!("SEAMS_SNAPSHOT_MISSING {rel} {hex}");
        panic!("fixture {rel} is missing; record it with node scripts/seams-snapshot.mjs");
    };
    let want: Vec<String> = raw.lines().map(str::to_string).collect();
    let (missing, added) = line_diff(&want, got);
    assert!(
        missing.is_empty() && added.is_empty(),
        "{rel} changed at the seam: missing {missing:?}, added {added:?}"
    );
    assert_eq!(raw, body, "{rel}: the same lines, written differently");
}

#[test]
fn registered_commands_are_unchanged() {
    let paths = registered_paths(LIB_RS);
    assert!(
        paths.len() > 400,
        "parsed {} commands; the parser is broken",
        paths.len()
    );
    assert_eq!(
        command_names(&paths).len(),
        paths.len(),
        "a command name is registered twice"
    );
    check_snapshot("commands", &paths);
}

#[test]
fn acl_allows_exactly_the_registered_commands() {
    let acl = acl_commands(MAIN_WINDOW_TOML);
    let allowed: BTreeSet<String> = acl.iter().cloned().collect();
    assert_eq!(
        allowed.len(),
        acl.len(),
        "main-window.toml lists a command twice"
    );
    let registered = command_names(&registered_paths(LIB_RS));
    let missing: Vec<_> = registered.difference(&allowed).collect();
    let extra: Vec<_> = allowed.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "registered but not in the ACL: {missing:?}"
    );
    assert!(
        extra.is_empty(),
        "listed in the ACL but not registered: {extra:?}"
    );
    assert!(MAIN_WINDOW_TOML.contains(MAIN_ID));
}

#[test]
fn lib_mod_set_is_unchanged() {
    let mods = lib_mods(LIB_RS);
    assert!(
        mods.len() > 150,
        "parsed {} modules; the parser is broken",
        mods.len()
    );
    check_snapshot("lib-mods", &mods);
}

// Mutation cases: the snapshots catch a dropped, a re-pointed and a new registration, and a
// module that loses its `#[cfg(test)]`.

#[test]
fn a_dropped_command_is_named() {
    let src = LIB_RS.replacen("comms::groups_assign_role,", "", 1);
    let (missing, added) = line_diff(&registered_paths(LIB_RS), &registered_paths(&src));
    assert_eq!(missing, vec!["comms::groups_assign_role".to_string()]);
    assert!(added.is_empty());
}

#[test]
fn a_repointed_or_added_command_is_named() {
    let src = LIB_RS.replacen(
        "comms::groups_assign_role,",
        "comms_v2::groups_assign_role, seams::extra_cmd,",
        1,
    );
    let (missing, added) = line_diff(&registered_paths(LIB_RS), &registered_paths(&src));
    assert_eq!(missing, vec!["comms::groups_assign_role".to_string()]);
    assert_eq!(
        added,
        vec![
            "comms_v2::groups_assign_role".to_string(),
            "seams::extra_cmd".to_string(),
        ]
    );
}

#[test]
fn a_module_losing_its_cfg_is_named() {
    let src = "#[cfg(test)]\nmod t;\nmod b;\n// note\npub(crate) mod c {\n}\n";
    assert_eq!(
        lib_mods(src),
        vec!["#[cfg(test)] mod t;", "mod b;", "pub(crate) mod c {"]
    );
    let moved = "mod t;\nmod b;\npub(crate) mod c {\n}\n";
    let (missing, added) = line_diff(&lib_mods(src), &lib_mods(moved));
    assert_eq!(missing, vec!["#[cfg(test)] mod t;".to_string()]);
    assert_eq!(added, vec!["mod t;".to_string()]);
}
