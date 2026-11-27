//! L0-WP02 SEAM-CMDS (planset v060): the command registration, the main-window ACL and the
//! lib.rs module set, recorded at a826e441 in `tests/fixtures/seams/`.
//!
//! L0-WP03 split the `generate_handler!` list into per-lane lists (`src/commands/l<N>.rs`) and
//! `permissions/main-window.toml` into per-lane ACLs; the registration and ACL snapshots stayed
//! byte-identical, and the module set gained exactly `mod commands;`. L0-WP04 added the per-lane
//! module tree (`src/lanes/`): the module set gained exactly `mod lanes;`, and a lane declares its
//! new modules in `lanes/l<N>/mod.rs`, so a new lane module leaves this snapshot alone. A lane PR
//! that drops or re-points a recorded command fails here by name. A lane adds a command in its
//! own `commands/l<N>.rs` and `permissions/l<N>-commands.toml` only: those lane-owned files are
//! the reviewed record of the addition, and the commands fixture stays the a826e441 baseline
//! (`baseline_problems`).
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
    let (_, fixture_path) = fixture("commands");
    if std::env::var("SEAMS_SNAPSHOT_WRITE").as_deref() == Ok("1") || !fixture_path.exists() {
        // Record the whole registry, or print the bytes a missing fixture would hold.
        check_snapshot("commands", &paths);
        return;
    }
    // Every recorded command is still registered at its path; an addition sits in one lane's
    // list and that lane's ACL. What is left is the baseline, byte for byte.
    let (problems, added) = baseline_problems(&fixture_lines("commands"), &live_lanes());
    assert!(
        problems.is_empty(),
        "the registry left the a826e441 baseline: {problems:?}"
    );
    for line in &added {
        println!("SEAMS_COMMAND_ADDED {line}");
    }
    let added: BTreeSet<&str> = added
        .iter()
        .filter_map(|line| line.split_once(' ').map(|(_, path)| path))
        .collect();
    let kept: Vec<String> = paths
        .iter()
        .filter(|p| !added.contains(p.as_str()))
        .cloned()
        .collect();
    check_snapshot("commands", &kept);
}

#[test]
fn acl_allows_exactly_the_registered_commands() {
    let mut acl: Vec<String> = Vec::new();
    for (id, toml) in crate::commands::lane_acls() {
        assert!(
            toml.contains(&format!("identifier = \"{id}\"")),
            "the {id} ACL names its permission"
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

/// One lane as the baseline check sees it: its id, its list's paths (file order) and the command
/// names its ACL allows.
struct LaneReg {
    id: String,
    paths: Vec<String>,
    allowed: BTreeSet<String>,
}

/// The registry against the recorded a826e441 baseline, as `(problems, additions)`, each
/// addition written `<lane> <path>`. Every baseline path must still be registered at that path, so
/// a drop or a re-point is named. A path added since is allowed when the same lane's ACL allows
/// it: the lane's own list and ACL record the addition, so a lane never edits the L0 fixture.
fn baseline_problems(baseline: &[String], lanes: &[LaneReg]) -> (Vec<String>, Vec<String>) {
    let base: BTreeSet<&str> = baseline.iter().map(String::as_str).collect();
    let mut now: BTreeSet<&str> = BTreeSet::new();
    let mut problems = Vec::new();
    let mut added = Vec::new();
    for lane in lanes {
        for path in &lane.paths {
            if !now.insert(path.as_str()) {
                problems.push(format!("{path} is registered twice"));
            }
            if base.contains(path.as_str()) {
                continue;
            }
            let name = path.rsplit("::").next().unwrap_or_default();
            if lane.allowed.contains(name) {
                added.push(format!("{} {path}", lane.id));
            } else {
                problems.push(format!(
                    "{path} is added by {id} without its {id}-commands ACL entry",
                    id = lane.id
                ));
            }
        }
    }
    for path in base.difference(&now) {
        problems.push(format!(
            "{path} is no longer registered (dropped or re-pointed)"
        ));
    }
    (problems, added)
}

/// The lanes as built: each lane's list and ACL, exactly as `commands` includes them.
fn live_lanes() -> Vec<LaneReg> {
    crate::commands::lane_sources()
        .into_iter()
        .zip(crate::commands::lane_acls())
        .map(|((lane, src), (_, toml))| LaneReg {
            id: lane.to_string(),
            paths: crate::commands::lane_paths(src)
                .iter()
                .map(|p| p.trim_start_matches("crate::").to_string())
                .collect(),
            allowed: acl_commands(toml).into_iter().collect(),
        })
        .collect()
}

fn lane_reg(id: &str, paths: &[&str], allowed: &[&str]) -> LaneReg {
    LaneReg {
        id: id.to_string(),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        allowed: allowed.iter().map(|a| a.to_string()).collect(),
    }
}

fn two_command_baseline() -> Vec<String> {
    vec![
        "comms::comms_send".to_string(),
        "config::config_read".to_string(),
    ]
}

// L0-WP03 Gherkin "two lanes add commands the same day": each lane edits only its own list and
// its own ACL; neither touches the L0 fixture, and both commands are registered and allowed.
#[test]
fn two_lanes_add_commands_without_touching_the_fixture() {
    let lanes = [
        lane_reg("l0", &["config::config_read"], &["config_read"]),
        lane_reg("l1", &["acp::acp_session_new"], &["acp_session_new"]),
        lane_reg("l3", &["shell::notify_permission"], &["notify_permission"]),
        lane_reg("l4", &["comms::comms_send"], &["comms_send"]),
    ];
    let (problems, added) = baseline_problems(&two_command_baseline(), &lanes);
    assert!(problems.is_empty(), "{problems:?}");
    let want = ["l1 acp::acp_session_new", "l3 shell::notify_permission"];
    assert_eq!(added, want);
}

// Another lane's ACL entry does not count: the addition is named against its own lane.
#[test]
fn an_added_command_without_its_lane_acl_is_named() {
    let lanes = [
        lane_reg("l0", &["config::config_read"], &["config_read"]),
        lane_reg("l3", &["shell::notify_permission"], &[]),
        lane_reg(
            "l4",
            &["comms::comms_send"],
            &["comms_send", "notify_permission"],
        ),
    ];
    let (problems, added) = baseline_problems(&two_command_baseline(), &lanes);
    let want = "shell::notify_permission is added by l3 without its l3-commands ACL entry";
    assert_eq!(problems, [want]);
    assert!(added.is_empty());
}

// A re-point is a drop plus an addition: the addition is allowed, the drop is still named.
#[test]
fn a_dropped_command_is_named_while_a_lane_adds_one() {
    let lanes = [
        lane_reg("l0", &["config::config_read"], &["config_read"]),
        lane_reg("l4", &["comms_v2::comms_send"], &["comms_send"]),
    ];
    let (problems, added) = baseline_problems(&two_command_baseline(), &lanes);
    let want = "comms::comms_send is no longer registered (dropped or re-pointed)";
    assert_eq!(problems, [want]);
    assert_eq!(added, ["l4 comms_v2::comms_send"]);
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
    // The lane files on disk are the whole built registry (the a826e441 baseline is checked once,
    // by registered_commands_are_unchanged, so recording it never races this test).
    let built = lane_list_paths(&crate::commands::registry_source());
    let (missing, added) = line_diff(&built, &all);
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
    // The union allows exactly the registered commands (at the split, the old
    // main-window-commands set; registered_commands_are_unchanged holds the baseline).
    let registered = crate::commands::registered_names();
    let missing: Vec<_> = registered.difference(&union).collect();
    let extra: Vec<_> = union.difference(&registered).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "the lane ACLs differ from the registry: missing {missing:?}, extra {extra:?}"
    );
    // The main window holds every lane's permission and no longer the single one.
    let cap: serde_json::Value =
        serde_json::from_str(&read_rel("capabilities/default.json")).expect("default.json is JSON");
    assert_eq!(
        cap["windows"],
        serde_json::json!(["main"]),
        "the lane permissions are granted to the main window only"
    );
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

// ---------------------------------------------------------------------------------------------
// L0-WP04: the per-lane module tree and start-up hooks (`src/lanes/l<N>/{mod.rs,hooks.rs}`),
// L0_INTEGRATION_SEAMS section 3.2. A lane adds a module, a plugin, a setup step or a quit step in
// its own files; lib.rs calls `crate::lanes::{plugins_all, setup_all, on_exit_all}` once each, at
// points that keep the existing start-up and quit order. These read the sources, so a hook that
// moves ahead of the single-instance plugin or the network gate fails by name.
// ---------------------------------------------------------------------------------------------

/// The three hook phases, in the order a launch runs them. The compiler checks each hook's
/// signature (lanes/mod.rs takes it as a typed function pointer); this checks the name and shape.
const HOOK_PHASES: [&str; 3] = ["plugins", "setup", "on_exit"];

/// lib.rs with every `//` comment dropped, so a comment that names a call never counts as one.
fn lib_code() -> String {
    LIB_RS
        .lines()
        .map(|l| match l.find("//") {
            Some(at) => &l[..at],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of `run()`: from its signature to the next top-level item.
fn run_body(code: &str) -> &str {
    let start = code
        .find("pub fn run() {")
        .expect("lib.rs has pub fn run()");
    let len = code[start..].find("\n}\n").expect("end of run()");
    &code[start..start + len]
}

/// The setup closure: from `.setup(|app| {` to the invoke handler that follows it.
fn setup_body(run: &str) -> &str {
    let start = run
        .find(".setup(|app| {")
        .expect("run() has the setup closure");
    let len = run[start..]
        .find(".invoke_handler(")
        .expect("the invoke handler follows the setup closure");
    &run[start..start + len]
}

/// Byte offsets of every occurrence of `needle` in `hay`.
fn offsets(hay: &str, needle: &str) -> Vec<usize> {
    hay.match_indices(needle).map(|(at, _)| at).collect()
}

#[test]
fn single_instance_is_first_plugin() {
    let code = lib_code();
    let run = run_body(&code);
    let builder = run
        .find("tauri::Builder::default()")
        .expect("run() builds the app with tauri::Builder::default()");
    let plugins = offsets(&run[builder..], ".plugin(");
    assert!(
        plugins.len() >= 2,
        "run() registers its plugins on the builder"
    );
    assert!(
        run[builder + plugins[0]..].starts_with(".plugin(tauri_plugin_single_instance::init("),
        "tauri requires tauri_plugin_single_instance to be the first plugin registered"
    );
    assert_eq!(
        code.matches("tauri_plugin_single_instance::init(").count(),
        1,
        "single-instance is registered once"
    );
}

#[test]
fn lane_plugins_follow_existing_plugins() {
    let code = lib_code();
    let run = run_body(&code);
    let calls = offsets(run, "crate::lanes::plugins_all(builder)");
    assert_eq!(
        calls.len(),
        1,
        "run() calls lanes::plugins_all(builder) once"
    );
    let last_plugin = *offsets(run, ".plugin(")
        .last()
        .expect("run() registers plugins");
    let setup = run.find(".setup(|app| {").expect("setup closure");
    assert!(
        last_plugin < calls[0] && calls[0] < setup,
        "the lane plugins come after every existing .plugin( and before .setup"
    );
    // lib.rs registers no plugin after the lanes': a new plugin goes in a lane's hooks.rs.
    assert!(
        !run[calls[0]..].contains(".plugin("),
        "no .plugin( after lanes::plugins_all in lib.rs"
    );
}

/// The gate installs, in lib.rs code. `run()` installs both before it creates the builder.
const GATES: [&str; 2] = [
    "network_profile::install_network_gate();",
    "network_profile::install_payouts_gate();",
];

/// Problems with where lib.rs installs the network and payouts gates (empty when right): each
/// gate is installed once, in `run()`, before `tauri::Builder::default()`, so before any plugin
/// initializes (a plugin's init runs while the app is built), before `lanes::plugins_all` and
/// before the setup closure and `lanes::setup_all`.
fn gate_problems(code: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let run = run_body(code);
    let Some(builder) = run.find("tauri::Builder::default()") else {
        return vec!["run() has no tauri::Builder::default()".into()];
    };
    for gate in GATES {
        let at = offsets(code, gate);
        if at.len() != 1 {
            problems.push(format!("lib.rs runs {gate} {} times, want 1", at.len()));
            continue;
        }
        match run.find(gate) {
            Some(at) if at < builder => {}
            Some(_) => problems.push(format!("{gate} runs after the builder is created")),
            None => problems.push(format!("{gate} is not in run()")),
        }
    }
    problems
}

#[test]
fn lane_plugins_run_after_network_gate() {
    let code = lib_code();
    assert_eq!(gate_problems(&code), Vec::<String>::new());
    let run = run_body(&code);
    let plugins = run
        .find("crate::lanes::plugins_all(builder)")
        .expect("run() calls lanes::plugins_all");
    for gate in GATES {
        assert!(
            run.find(gate).expect("gate in run()") < plugins,
            "lane plugins are added after {gate}"
        );
    }
    // Mutation proofs: each regression below is caught.
    let gate_in_setup = code.replacen(GATES[0], "", 1).replacen(
        ".setup(|app| {",
        &format!(".setup(|app| {{ {}", GATES[0]),
        1,
    );
    let gate_twice = code.replacen(GATES[1], &format!("{} {}", GATES[1], GATES[1]), 1);
    let gate_lost = code.replacen(GATES[1], "", 1);
    for (name, mutant) in [
        ("network gate moved into setup", gate_in_setup),
        ("payouts gate installed twice", gate_twice),
        ("payouts gate lost", gate_lost),
    ] {
        assert_ne!(mutant, code, "{name}: the mutation applies");
        assert!(!gate_problems(&mutant).is_empty(), "{name} is caught");
    }
}

#[test]
fn lane_setup_runs_after_network_gate() {
    let code = lib_code();
    assert_eq!(gate_problems(&code), Vec::<String>::new());
    let run = run_body(&code);
    let setup = setup_body(run);
    let calls = offsets(setup, "crate::lanes::setup_all(app)?;");
    assert_eq!(
        calls.len(),
        1,
        "the setup closure calls lanes::setup_all(app)? once"
    );
    let setup_at = run.find(".setup(|app| {").expect("setup closure");
    for gate in GATES {
        assert!(
            run.find(gate).expect("gate in run()") < setup_at,
            "lane setup hooks run after {gate}"
        );
    }
    // Last: the existing setup body runs in full, then the lanes, then the closure returns.
    let after = &setup[calls[0] + "crate::lanes::setup_all(app)?;".len()..];
    assert!(
        after.trim_start().starts_with("Ok(())"),
        "lanes::setup_all is the last step of the setup closure"
    );
}

/// The span of the block that starts at the `{` at `open` (inclusive of both braces).
fn block_at(code: &str, open: usize) -> Option<&str> {
    let mut depth = 0usize;
    for (i, c) in code[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(&code[open..=open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Problems with the quit path's lane hooks (empty when right): the run-event handler has one
/// exit branch, `if matches!(event, ExitRequested { .. } | Exit) { .. }`, and inside it, and only
/// there, `shutdown_all_sidecars(app_handle);` then `crate::lanes::on_exit_all(app_handle);`,
/// each once. A call after the branch's closing brace would run on every run event.
fn exit_problems(code: &str) -> Vec<String> {
    const OPEN: &str = ".run(|app_handle, event| {";
    const TEARDOWN: &str = "shutdown_all_sidecars(app_handle);";
    const LANES_EXIT: &str = "crate::lanes::on_exit_all(app_handle);";
    let Some(handler_at) = code.find(OPEN) else {
        return vec!["lib.rs has no run-event handler".into()];
    };
    let Some(handler) = block_at(code, handler_at + OPEN.len() - 1) else {
        return vec!["the run-event handler is not closed".into()];
    };
    let mut problems = Vec::new();
    if offsets(code, "on_exit_all(").len() != 1 {
        problems.push("lib.rs must call lanes::on_exit_all exactly once".into());
    }
    let Some(cond_at) = handler.find("if matches!(") else {
        return [problems, vec!["the handler has no exit branch".into()]].concat();
    };
    // The branch condition is the macro call's parentheses; its body is the next block.
    let mut depth = 0usize;
    let mut cond_end = None;
    for (i, c) in handler[cond_at..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    cond_end = Some(cond_at + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(cond_end) = cond_end else {
        return [problems, vec!["the exit branch condition is not closed".into()]].concat();
    };
    let cond: String = handler[cond_at..cond_end].split_whitespace().collect();
    if cond != "ifmatches!(event,tauri::RunEvent::ExitRequested{..}|tauri::RunEvent::Exit)" {
        problems.push(format!("exit branch condition changed: {cond}"));
    }
    let rest = &handler[cond_end..];
    let open = cond_end + (rest.len() - rest.trim_start().len());
    let body = match handler[open..].starts_with('{') {
        true => block_at(handler, open),
        false => None,
    };
    let Some(body) = body else {
        return [problems, vec!["the exit branch has no body".into()]].concat();
    };
    let inner: Vec<&str> = body[1..body.len() - 1].split_whitespace().collect();
    if inner != [TEARDOWN, LANES_EXIT] {
        problems.push(format!(
            "the exit branch must run {TEARDOWN} then {LANES_EXIT}, got {}",
            inner.join(" ")
        ));
    }
    let outside = handler.replacen(body, "", 1);
    for call in [TEARDOWN, LANES_EXIT] {
        if outside.contains(call) {
            problems.push(format!("{call} runs outside the exit branch"));
        }
    }
    problems
}

#[test]
fn lane_exit_hooks_run_after_sidecar_teardown() {
    let code = lib_code();
    assert_eq!(exit_problems(&code), Vec::<String>::new());
    let teardown = "                shutdown_all_sidecars(app_handle);\n";
    let lanes_exit = "crate::lanes::on_exit_all(app_handle);";
    let branch_end = format!("{lanes_exit}\n            }}");
    // Mutation proofs (review findings on 92bb72ea and 637b9200): each regression below is caught.
    let after_branch = code.replacen(&branch_end, &format!("}}\n            {lanes_exit}"), 1);
    let before_teardown = code
        .replacen(&format!("{lanes_exit}\n"), "\n", 1)
        .replacen(teardown, &format!("                {lanes_exit}\n{teardown}"), 1);
    let exit_only = code.replacen(
        "tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit",
        "tauri::RunEvent::Exit",
        1,
    );
    let twice = code.replacen(lanes_exit, &format!("{lanes_exit} {lanes_exit}"), 1);
    let lost = code.replacen(lanes_exit, "", 1);
    for (name, mutant) in [
        ("quit hooks after the exit branch (every run event)", after_branch),
        ("quit hooks before the sidecar teardown", before_teardown),
        ("ExitRequested no longer handled", exit_only),
        ("quit hooks called twice", twice),
        ("quit hooks lost", lost),
    ] {
        assert_ne!(mutant, code, "{name}: the mutation applies");
        assert!(!exit_problems(&mutant).is_empty(), "{name} is caught");
    }
}

#[test]
fn lane_tree_has_every_lane() {
    assert!(
        LIB_RS.lines().any(|l| l == "mod lanes;"),
        "lib.rs declares mod lanes"
    );
    let dir = tauri_dir().join("src/lanes");
    let mut got: BTreeSet<String> = BTreeSet::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("src-tauri/src/lanes: {e}")) {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() {
            got.insert(name);
        } else {
            assert_eq!(
                name, "mod.rs",
                "src/lanes holds mod.rs and the lane folders only"
            );
        }
    }
    let want: BTreeSet<String> = LANES.iter().map(|l| l.to_string()).collect();
    assert_eq!(got, want, "src/lanes holds exactly the ten lane folders");

    let modrs = read_rel("src/lanes/mod.rs");
    for lane in LANES {
        assert!(
            modrs.contains(&format!("pub(crate) mod {lane};")),
            "lanes/mod.rs declares pub(crate) mod {lane}"
        );
        assert!(
            read_rel(&format!("src/lanes/{lane}/mod.rs")).contains("pub(crate) mod hooks;"),
            "lanes/{lane}/mod.rs declares its hooks module"
        );
        let hooks = read_rel(&format!("src/lanes/{lane}/hooks.rs"));
        for phase in HOOK_PHASES {
            assert_eq!(
                hooks
                    .matches(&format!("pub(crate) fn {phase}<R: Runtime>("))
                    .count(),
                1,
                "lanes/{lane}/hooks.rs defines {phase} once, generic over the runtime"
            );
        }
    }
    // Each phase calls l0..l9 in that order, each lane's own hook (never another lane's).
    for phase in HOOK_PHASES {
        let mut last = 0;
        for lane in LANES {
            let row = format!("(\"{lane}\", {lane}::hooks::{phase})");
            let at = offsets(&modrs, &row);
            assert_eq!(at.len(), 1, "lanes/mod.rs calls {row} once");
            assert!(
                at[0] > last,
                "lanes/mod.rs calls the {phase} hooks in lane order at {lane}"
            );
            last = at[0];
        }
    }
}

// The hooks as built, run through tauri's mock runtime: every phase runs l0 to l9, each once.
#[cfg(not(windows))] // tauri's mock runtime (`test` feature) is a non-Windows dev-dependency
#[test]
fn lane_hooks_run_in_lane_order() {
    let _ = crate::lanes::take_recorded();
    let _builder = crate::lanes::plugins_all(tauri::test::mock_builder());
    let app = tauri::test::mock_app();
    crate::lanes::setup_all(&app).expect("an empty setup hook returns Ok");
    crate::lanes::on_exit_all(app.handle());
    let want: Vec<(&str, &str)> = HOOK_PHASES
        .iter()
        .flat_map(|phase| LANES.iter().map(move |lane| (*lane, *phase)))
        .collect();
    assert_eq!(
        crate::lanes::take_recorded(),
        want,
        "plugins, then setup, then on_exit, each l0 to l9"
    );
}

// Mutation case: a failing lane stops start-up with its lane id, and no later lane runs.
#[cfg(not(windows))]
#[test]
fn setup_all_stops_at_the_first_failing_lane() {
    fn ok<R: tauri::Runtime>(_app: &tauri::App<R>) -> Result<(), crate::lanes::HookError> {
        Ok(())
    }
    fn fails<R: tauri::Runtime>(_app: &tauri::App<R>) -> Result<(), crate::lanes::HookError> {
        Err("store folder is read-only".into())
    }
    let hooks: [(
        &'static str,
        crate::lanes::SetupHook<tauri::test::MockRuntime>,
    ); 3] = [("l0", ok), ("l1", fails), ("l2", ok)];
    let app = tauri::test::mock_app();
    let _ = crate::lanes::take_recorded();
    let err = crate::lanes::run_setup(&app, &hooks).expect_err("l1 fails");
    assert_eq!(
        err.to_string(),
        "lane l1 setup hook failed: store folder is read-only"
    );
    assert_eq!(
        crate::lanes::take_recorded(),
        vec![("l0", "setup"), ("l1", "setup")],
        "l2 does not run after l1 fails"
    );
}
