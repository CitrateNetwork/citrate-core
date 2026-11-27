//! L0-WP02 SEAM-CMDS (planset v060): the command registration, the main-window ACL and the
//! lib.rs module set, recorded at a826e441 in `tests/fixtures/seams/`.
//!
//! L0-WP03 split the `generate_handler!` list into per-lane lists (`src/commands/l<N>.rs`) and
//! `permissions/main-window.toml` into per-lane ACLs; the registration and ACL snapshots stayed
//! byte-identical, and the module set gained exactly `mod commands;`. L0-WP04 moves the module
//! tree and must keep these snapshots byte-identical. A lane PR that drops a command fails here
//! by name.
//! A fixture changes only in an L0-reviewed PR that says why; record it again with
//! `node scripts/seams-snapshot.mjs` (`SEAMS_SNAPSHOT_WRITE=1`). A missing fixture fails and prints
//! `SEAMS_SNAPSHOT_MISSING <path> <hex>` with the exact bytes it would record.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

const BASE: &str = "a826e441";
const LIB_RS: &str = include_str!("lib.rs");

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

/// The command paths of every lane list in `src` (one or more lane files, or
/// `commands::registry_source()`), sorted. Each list opens after the accumulator, `LANE_LIST_OPEN`.
fn lane_list_paths(src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in src.match_indices(LANE_LIST_OPEN) {
        let start = at + LANE_LIST_OPEN.len();
        let Some(len) = src[start..].find("])") else {
            continue;
        };
        out.extend(
            src[start..start + len]
                .lines()
                .map(|l| l.split("//").next().unwrap_or_default())
                .flat_map(|l| l.split(','))
                .map(str::trim)
                .filter(|s| s.contains("::"))
                .map(|s| s.trim_start_matches("crate::").to_string()),
        );
    }
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
        // A CI job must never pass by recording: fixtures are recorded locally and reviewed.
        assert!(std::env::var_os("CI").is_none(), "write mode under CI");
        let dir = path.parent().expect("fixture dir");
        fs::create_dir_all(dir).expect("create fixture dir");
        fs::write(&path, &body).expect("write fixture");
        return;
    }
    let Ok(raw) = fs::read_to_string(&path) else {
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
    let paths = lane_list_paths(&crate::commands::registry_source());
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
    // Two parsers, written apart, read the same registry.
    let mut from_helper = crate::commands::registered_paths();
    from_helper.sort();
    assert_eq!(paths, from_helper);
    check_snapshot("commands", &paths);
}

#[test]
fn acl_allows_exactly_the_registered_commands() {
    let mut acl: Vec<String> = Vec::new();
    for (id, toml) in crate::commands::lane_acls() {
        assert!(
            toml.contains(&format!("identifier = \"{id}\"")),
            "{id}-commands.toml names its permission"
        );
        acl.extend(acl_commands(toml));
    }
    let allowed: BTreeSet<String> = acl.iter().cloned().collect();
    assert_eq!(
        allowed.len(),
        acl.len(),
        "the lane ACLs list a command twice"
    );
    let registered = command_names(&lane_list_paths(&crate::commands::registry_source()));
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
    let reg = crate::commands::registry_source();
    let src = reg.replacen("comms::groups_assign_role,", "", 1);
    let (missing, added) = line_diff(&lane_list_paths(&reg), &lane_list_paths(&src));
    assert_eq!(missing, vec!["comms::groups_assign_role".to_string()]);
    assert!(added.is_empty());
}

#[test]
fn a_repointed_or_added_command_is_named() {
    let reg = crate::commands::registry_source();
    let src = reg.replacen(
        "comms::groups_assign_role,",
        "comms_v2::groups_assign_role, seams::extra_cmd,",
        1,
    );
    let (missing, added) = line_diff(&lane_list_paths(&reg), &lane_list_paths(&src));
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

// ---------------------------------------------------------------------------------------------
// L0-WP03: per-lane command registration (`src/commands/l<N>.rs`) and per-lane ACL
// (`permissions/l<N>-commands.toml`), L0_INTEGRATION_SEAMS section 3.1. These read the files from
// disk, so a lane file that is missing, extra or not wired into the chain fails by name.
// ---------------------------------------------------------------------------------------------

const LANES: [&str; 10] = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7", "l8", "l9"];
/// Where each lane's list starts inside its macro: the accumulator, then the lane's own paths.
const LANE_LIST_OPEN: &str = "[$($acc)*";

fn tauri_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_rel(rel: &str) -> String {
    fs::read_to_string(tauri_dir().join(rel)).unwrap_or_else(|e| panic!("src-tauri/{rel}: {e}"))
}

fn lane_rs(lane: &str) -> String {
    read_rel(&format!("src/commands/{lane}.rs"))
}

fn lane_toml(lane: &str) -> String {
    read_rel(&format!("permissions/{lane}-commands.toml"))
}

/// The command paths a lane file appends, in file order (comments dropped).
fn lane_paths(lane: &str, src: &str) -> Vec<String> {
    assert_eq!(
        src.matches(&format!("macro_rules! v60_commands_{lane} "))
            .count(),
        1,
        "commands/{lane}.rs defines its macro v60_commands_{lane} exactly once"
    );
    assert_eq!(
        src.matches(LANE_LIST_OPEN).count(),
        1,
        "commands/{lane}.rs has exactly one list"
    );
    let Some(at) = src.find(LANE_LIST_OPEN) else {
        return Vec::new();
    };
    let start = at + LANE_LIST_OPEN.len();
    let len = src[start..]
        .find("])")
        .unwrap_or_else(|| panic!("commands/{lane}.rs: end of the list"));
    src[start..start + len]
        .lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_start_matches("crate::").to_string())
        .collect()
}

fn fixture_lines(name: &str) -> Vec<String> {
    let (rel, path) = fixture(name);
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn lane_lists_partition_the_registry() {
    let mut owner: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    let mut all: Vec<String> = Vec::new();
    for lane in LANES {
        for path in lane_paths(lane, &lane_rs(lane)) {
            assert!(
                path.contains("::"),
                "commands/{lane}.rs: `{path}` is not a module path"
            );
            let name = path.rsplit("::").next().unwrap_or_default().to_string();
            if let Some(first) = owner.insert(name.clone(), lane) {
                panic!("{name} is registered by both {first} and {lane}");
            }
            all.push(path);
        }
    }
    all.sort();
    let (missing, added) = line_diff(&fixture_lines("commands"), &all);
    assert!(
        missing.is_empty() && added.is_empty(),
        "the lane lists are not the registry: missing {missing:?}, added {added:?}"
    );
    // lib.rs keeps no list of its own: one invocation of the chain, no generate_handler.
    assert!(
        registered_paths(LIB_RS).is_empty(),
        "lib.rs still has a generate_handler! list"
    );
    assert_eq!(
        LIB_RS
            .matches(".invoke_handler(crate::v60_all_commands!())")
            .count(),
        1,
        "lib.rs registers the commands through the lane chain exactly once"
    );
    assert_eq!(LIB_RS.matches(".invoke_handler(").count(), 1);
}

#[test]
fn lane_acl_matches_lane_list() {
    let mut union: BTreeSet<String> = BTreeSet::new();
    for lane in LANES {
        let toml = lane_toml(lane);
        let id = format!("identifier = \"{lane}-commands\"");
        assert_eq!(toml.matches(&id).count(), 1, "{lane}-commands.toml: {id}");
        assert_eq!(
            toml.matches("[[permission]]").count(),
            1,
            "{lane}-commands.toml holds one permission"
        );
        assert!(
            !toml.contains("commands.deny"),
            "{lane}-commands.toml denies nothing"
        );
        let acl = acl_commands(&toml);
        let allowed: BTreeSet<String> = acl.iter().cloned().collect();
        assert_eq!(
            allowed.len(),
            acl.len(),
            "{lane}-commands.toml lists a command twice"
        );
        let listed = command_names(&lane_paths(lane, &lane_rs(lane)));
        let missing: Vec<_> = listed.difference(&allowed).collect();
        let extra: Vec<_> = allowed.difference(&listed).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{lane}: registered but not allowed {missing:?}, allowed but not registered {extra:?}"
        );
        union.extend(allowed);
    }
    // The union is exactly the old main-window-commands set (the recorded registry's names).
    let old = command_names(&fixture_lines("commands"));
    let missing: Vec<_> = old.difference(&union).collect();
    let extra: Vec<_> = union.difference(&old).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "the lane ACLs differ from main-window-commands: missing {missing:?}, extra {extra:?}"
    );
    // The main window holds every lane's permission and no longer the single one.
    let cap: serde_json::Value =
        serde_json::from_str(&read_rel("capabilities/default.json")).expect("default.json is JSON");
    let perms: Vec<&str> = cap["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .filter_map(|p| p.as_str())
        .collect();
    for lane in LANES {
        let want = format!("{lane}-commands");
        assert_eq!(
            perms.iter().filter(|p| **p == want).count(),
            1,
            "default.json holds {want} once"
        );
    }
    assert!(
        !perms.contains(&"main-window-commands"),
        "default.json still holds main-window-commands"
    );
    assert!(
        !tauri_dir().join("permissions/main-window.toml").exists(),
        "permissions/main-window.toml is split into the lane files"
    );
}

#[test]
fn registry_source_sees_every_lane() {
    let want: BTreeSet<String> = LANES.iter().map(|l| format!("{l}.rs")).collect();
    let mut got: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(tauri_dir().join("src/commands"))
        .unwrap_or_else(|e| panic!("src-tauri/src/commands: {e}"))
    {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .to_string();
        if name != "mod.rs" {
            got.insert(name);
        }
    }
    assert_eq!(
        got, want,
        "src/commands holds mod.rs and exactly the ten lane files"
    );
    let mut tomls: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(tauri_dir().join("permissions")).expect("permissions dir") {
        let name = entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .to_string();
        if name.ends_with("-commands.toml") {
            tomls.insert(name);
        }
    }
    let want_tomls: BTreeSet<String> = LANES.iter().map(|l| format!("{l}-commands.toml")).collect();
    assert_eq!(
        tomls, want_tomls,
        "permissions holds exactly the ten lane ACL files"
    );

    let modrs = read_rel("src/commands/mod.rs");
    assert!(
        LIB_RS.lines().any(|l| l == "mod commands;"),
        "lib.rs declares mod commands"
    );
    // The chain runs l0 to l9, then the one generate_handler!, in this order and nothing else.
    let chain: Vec<String> = LANES[1..]
        .iter()
        .map(|l| format!("v60_commands_{l}"))
        .chain(["v60_commands_finish".to_string(), "[]".to_string()])
        .collect();
    let flat: String = modrs.split_whitespace().collect::<Vec<_>>().join(" ");
    let call = format!("$crate::v60_commands_l0!( {} )", chain.join(" "));
    assert_eq!(
        flat.matches(&call).count(),
        1,
        "mod.rs chains l0..l9 then finish: {call}"
    );
    assert_eq!(
        modrs.matches("generate_handler!").count(),
        1,
        "exactly one generate_handler! in mod.rs"
    );
    for lane in LANES {
        assert!(
            modrs.contains(&format!("mod {lane};")),
            "mod.rs declares mod {lane}"
        );
        // The test-only sources every structural test reads include every lane's list and ACL.
        assert!(
            modrs.contains(&format!("include_str!(\"{lane}.rs\")")),
            "registry_source() includes commands/{lane}.rs"
        );
        assert!(
            modrs.contains(&format!(
                "include_str!(\"../../permissions/{lane}-commands.toml\")"
            )),
            "acl_source() includes permissions/{lane}-commands.toml"
        );
        // A lane macro only appends to the accumulator and hands it to the next macro.
        let src = lane_rs(lane);
        assert!(
            src.contains("$crate::v60_commands_next!($($rest)* [$($acc)*"),
            "commands/{lane}.rs passes the accumulator on"
        );
        assert!(
            !src.contains("generate_handler"),
            "commands/{lane}.rs must not call generate_handler! itself"
        );
    }
}
