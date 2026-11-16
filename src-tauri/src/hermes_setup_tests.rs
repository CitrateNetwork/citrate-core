// 0.5.4: Hermes is complete on install. Provisioning is deterministic and idempotent, and the
// self-check names every missing item instead of leaving it to a log line.
use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "core-hermes-setup-{tag}-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The real bundled resources this build ships (`src-tauri/capsules`, `src-tauri/skills`).
fn real_resources() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn node_ok() -> NodeMcpFacts {
    NodeMcpFacts {
        enabled: true,
        running: true,
        endpoint: "http://127.0.0.1:47204/mcp".into(),
        last_error: None,
    }
}

/// A complete install: the bundled capsules and skills, a binary on disk, the node server up.
fn complete(dir: &Path) -> SetupFacts {
    let bin = dir.join("hermes-bin");
    std::fs::write(&bin, b"#!/bin/sh\n").unwrap();
    let model = dir.join("bge.gguf");
    std::fs::write(&model, b"gguf").unwrap();
    let hermes = dir.join("hermes");
    let dir_errors = provision_dirs(&hermes);
    let seed = provision_capsules(&hermes, Some(&real_resources()));
    SetupFacts {
        hermes_dir: hermes,
        resources: Some(real_resources()),
        hermes_bin: Ok(bin.clone()),
        embed_model: Some(model),
        llama_bin: Some(bin),
        seed,
        mcp_settings: McpSettings {
            node: true,
            ..McpSettings::default()
        },
        node_mcp: Some(node_ok()),
        dir_errors,
        mcp_settings_error: None,
        node_config_error: None,
        install: Some(InstallKind::New),
    }
}

fn state_of<'a>(items: &'a [SetupItem], id: &str) -> &'a SetupItem {
    items.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("no {id} row"))
}

#[test]
fn provisioning_twice_changes_nothing_the_second_time() {
    let d = tmp("idem");
    let hermes = d.join("hermes");
    assert!(provision_dirs(&hermes).is_empty());
    for sub in DATA_SUBDIRS {
        assert!(hermes.join(sub).is_dir(), "{sub}");
    }
    let first = provision_capsules(&hermes, Some(&real_resources())).expect("resources");
    assert!(first.refused.is_empty(), "{:?}", first.refused);
    assert_eq!(first.seeded.len(), BUNDLED_CAPSULES.len());
    let second = provision_capsules(&hermes, Some(&real_resources())).expect("resources");
    assert!(second.seeded.is_empty() && second.repaired.is_empty(), "{second:?}");
    assert_eq!(second.kept.len(), BUNDLED_CAPSULES.len());
    assert!(provision_dirs(&hermes).is_empty());
}

#[test]
fn a_real_first_run_offers_the_node_mcp_read_tools_once() {
    let d = tmp("new");
    let hermes = d.join("hermes");
    let node = d.join("node-mcp");
    let (kind, wrote) = first_run(&hermes, Some(&node), false);
    assert_eq!(kind, InstallKind::New);
    assert_eq!(wrote.len(), 2, "{wrote:?}");
    let s = crate::hermes_mcp::load_settings(&hermes);
    assert!(s.node && !s.mem && !s.scan, "{s:?}");
    let cfg: crate::node_mcp::NodeMcpConfig =
        serde_json::from_str(&std::fs::read_to_string(node.join("config.json")).unwrap()).unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.port, None, "the default port");
    // Read tools only, whatever the default.
    const { assert!(!crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS) };
    assert_eq!(read_marker(&hermes), Some(InstallKind::New));

    // The member turns both off; later launches keep their choice and decide nothing again.
    crate::hermes_mcp::save_settings(&hermes, &McpSettings::default()).unwrap();
    std::fs::write(node.join("config.json"), br#"{"enabled":false}"#).unwrap();
    std::fs::remove_file(node.join("config.json")).unwrap(); // even a file removed later
    for had in [false, true] {
        let (k, w) = first_run(&hermes, Some(&node), had);
        assert_eq!((k, w.len()), (InstallKind::New, 0));
    }
    assert!(!crate::hermes_mcp::load_settings(&hermes).node);
    assert!(!node.join("config.json").exists(), "nothing rewritten after the first run");
}

#[test]
fn an_upgraded_install_keeps_node_tools_off() {
    // 0.5.3 data on disk (any entry in the app data folders), never a saved MCP choice.
    let d = tmp("upgrade");
    let data = d.join("app-data");
    std::fs::create_dir_all(data.join("node")).unwrap();
    std::fs::write(data.join("config.json"), b"{}").unwrap();
    assert!(had_existing_data(&[data.clone(), d.join("absent")]));
    let hermes = data.join("hermes");
    let node = data.join("node-mcp");
    let (kind, wrote) = first_run(&hermes, Some(&node), true);
    assert_eq!(kind, InstallKind::Upgrade);
    assert!(wrote.is_empty(), "{wrote:?}");
    assert!(!crate::hermes_mcp::settings_path(&hermes).exists());
    assert!(!node.join("config.json").exists());
    assert!(!crate::hermes_mcp::load_settings(&hermes).node);
    assert_eq!(read_marker(&hermes), Some(InstallKind::Upgrade));
    // Decided once: a later launch that looks "fresh" still does not turn anything on.
    let (kind, wrote) = first_run(&hermes, Some(&node), false);
    assert_eq!((kind, wrote.len()), (InstallKind::Upgrade, 0));
    assert!(!node.join("config.json").exists());
    // The self-check reports the node tools as off (the Setup card's "Turn on node tools").
    let mut f = complete(&d);
    f.mcp_settings = crate::hermes_mcp::load_settings(&hermes);
    f.install = read_marker(&hermes);
    let r = report(items(&f), vec![], f.install);
    assert_eq!(state_of(&r.items, "node-mcp").state, ItemState::Off);
    assert_eq!(r.install, Some(InstallKind::Upgrade));
}

#[test]
fn fresh_and_existing_data_are_told_apart_and_doubt_counts_as_existing() {
    let d = tmp("probe");
    assert!(!had_existing_data(&[d.join("absent")]), "no folder at all: a real first run");
    std::fs::create_dir_all(d.join("empty")).unwrap();
    assert!(!had_existing_data(&[d.join("empty")]), "an empty folder: a real first run");
    std::fs::write(d.join("file"), b"x").unwrap();
    // A path that is a file, not a folder, cannot be listed: counted as existing data.
    assert!(had_existing_data(&[d.join("file")]));
}

#[test]
fn a_default_never_replaces_a_file_written_first() {
    // The member's save lands between the check and the write: theirs is kept, byte for byte.
    let d = tmp("race");
    let p = d.join("mcp-settings.json");
    std::fs::write(&p, br#"{"node":false}"#).unwrap();
    assert!(!write_new_file(&p, br#"{"node":true}"#).unwrap());
    assert_eq!(std::fs::read(&p).unwrap(), br#"{"node":false}"#);
    // No temp file is left behind either way.
    assert!(write_new_file(&d.join("fresh.json"), b"{}").unwrap());
    let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
    // provision_defaults keeps an existing file too.
    let hermes = d.join("hermes");
    crate::hermes_mcp::save_settings(&hermes, &McpSettings::default()).unwrap();
    let wrote = provision_defaults(&hermes, None);
    assert!(wrote.is_empty());
    assert!(!crate::hermes_mcp::load_settings(&hermes).node);
}

#[test]
fn a_corrupt_settings_file_is_reported_and_never_rewritten() {
    let d = tmp("corrupt");
    let hermes = d.join("hermes");
    let node = d.join("node-mcp");
    std::fs::create_dir_all(&hermes).unwrap();
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(crate::hermes_mcp::settings_path(&hermes), b"{not json").unwrap();
    std::fs::write(node.join("config.json"), b"\x00garbage").unwrap();
    // A first run with corrupt files already there replaces neither.
    first_run(&hermes, Some(&node), false);
    assert_eq!(std::fs::read(crate::hermes_mcp::settings_path(&hermes)).unwrap(), b"{not json");
    // The check says so, by file.
    let mut f = complete(&d);
    f.mcp_settings_error = parse_error::<McpSettings>(&crate::hermes_mcp::settings_path(&hermes));
    f.node_config_error = parse_error::<crate::node_mcp::NodeMcpConfig>(&node.join("config.json"));
    assert!(f.mcp_settings_error.is_some() && f.node_config_error.is_some());
    let row = state_of(&items(&f), "node-mcp").clone();
    assert_eq!(row.state, ItemState::Missing);
    assert!(row.detail.contains("mcp-settings.json does not parse"), "{row:?}");
    assert!(row.detail.contains("config.json does not parse"), "{row:?}");
    // A marker that does not parse is not rewritten and turns nothing on.
    let h2 = d.join("h2");
    std::fs::create_dir_all(&h2).unwrap();
    std::fs::write(h2.join(MARKER_FILE), b"??").unwrap();
    let (k, w) = first_run(&h2, None, false);
    assert_eq!((k, w.len()), (InstallKind::Upgrade, 0));
    assert_eq!(std::fs::read(h2.join(MARKER_FILE)).unwrap(), b"??");
}

#[test]
fn the_self_check_verifies_capsules_without_writing() {
    let d = tmp("readonly");
    let hermes = d.join("hermes");
    // Nothing installed: every capsule reported, and nothing created.
    let r = verify_capsules(&hermes);
    assert_eq!(r.refused.len(), BUNDLED_CAPSULES.len());
    assert!(!hermes.exists(), "the check created {}", hermes.display());
    // Installed by the setup path: all verify.
    provision_capsules(&hermes, Some(&real_resources()));
    let r = verify_capsules(&hermes);
    assert_eq!((r.kept.len(), r.refused.len()), (BUNDLED_CAPSULES.len(), 0));
    // A tampered capsule is reported, not repaired.
    let (name, _) = BUNDLED_CAPSULES[0];
    let cps = hermes.join("capsules").join(name).join(format!("{name}.cps"));
    std::fs::write(&cps, b"tampered").unwrap();
    let r = verify_capsules(&hermes);
    assert!(r.refused.iter().any(|(n, why)| n == name && why.contains("does not match")), "{r:?}");
    assert_eq!(std::fs::read(&cps).unwrap(), b"tampered");
}

#[test]
fn every_first_party_skill_is_checked_by_name() {
    // The list matches what this repo bundles.
    let mut bundled: Vec<String> = std::fs::read_dir(real_resources().join("skills"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("SKILL.md").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    bundled.sort();
    assert_eq!(bundled, FIRST_PARTY_SKILLS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    // One of them missing fails the row by name (one present is not enough).
    let d = tmp("skills");
    let res = d.join("res");
    for n in &FIRST_PARTY_SKILLS[..1] {
        std::fs::create_dir_all(res.join("skills").join(n)).unwrap();
        std::fs::write(res.join("skills").join(n).join("SKILL.md"), b"# s").unwrap();
    }
    let mut f = complete(&d);
    f.resources = Some(res);
    let row = state_of(&items(&f), "first-party-skills").clone();
    assert_eq!(row.state, ItemState::Missing);
    assert!(row.detail.contains(FIRST_PARTY_SKILLS[1]), "{row:?}");
    assert!(!row.detail.contains(FIRST_PARTY_SKILLS[0]), "{row:?}");
}

#[cfg(unix)]
#[test]
fn first_run_default_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("mode");
    first_run(&d.join("hermes"), Some(&d.join("node-mcp")), false);
    for p in [
        crate::hermes_mcp::settings_path(&d.join("hermes")),
        d.join("node-mcp").join("config.json"),
    ] {
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", p.display());
    }
}

#[test]
fn a_complete_install_reports_every_required_item_ok() {
    let d = tmp("complete");
    let f = complete(&d);
    let r = report(items(&f), vec![], None);
    for i in &r.items {
        if i.need == Need::Required {
            assert_eq!(i.state, ItemState::Ok, "{i:?}");
        }
    }
    assert_eq!(r.missing_required, 0);
    assert!(state_of(&r.items, "node-mcp").detail.contains("127.0.0.1:47204/mcp"));
    assert!(state_of(&r.items, "first-party-skills").detail.contains("all 11 bundled"));
}

#[test]
fn every_missing_piece_is_named_not_logged() {
    let d = tmp("missing");
    let mut f = complete(&d);
    f.hermes_bin = Err("CITRATE_HERMES_BIN set but not found: /nope".into());
    f.resources = None;
    f.seed = None;
    f.embed_model = None;
    f.node_mcp = Some(NodeMcpFacts {
        running: false,
        last_error: Some("address in use".into()),
        ..node_ok()
    });
    std::fs::remove_dir_all(f.hermes_dir.join("anchor")).unwrap();
    f.dir_errors = vec![("anchor".into(), "permission denied".into())];
    let r = report(items(&f), vec![], None);
    let bin = state_of(&r.items, "hermes-binary");
    assert_eq!(bin.state, ItemState::Missing);
    assert!(bin.detail.contains("/nope"));
    assert!(state_of(&r.items, "data-folders").detail.contains("anchor (permission denied)"));
    assert!(state_of(&r.items, "starter-capsules").detail.contains("resource folder"));
    assert_eq!(state_of(&r.items, "first-party-skills").state, ItemState::Missing);
    assert!(state_of(&r.items, "node-mcp").detail.contains("address in use"));
    // Optional items are reported but never counted as a failed install.
    let emb = state_of(&r.items, "embedding");
    assert_eq!((emb.need, emb.state), (Need::Optional, ItemState::Missing));
    assert!(emb.detail.contains("ranks memory by words"));
    assert_eq!(r.missing_required, 5, "{:#?}", r.items);
}

#[test]
fn a_refused_capsule_is_reported_with_its_reason() {
    let d = tmp("refused");
    let mut f = complete(&d);
    let mut seed = f.seed.take().unwrap();
    let (name, _) = BUNDLED_CAPSULES[0];
    seed.seeded.retain(|n| n != name);
    seed.kept.retain(|n| n != name);
    seed.refused.push((name.to_string(), "digest does not match the pin".into()));
    f.seed = Some(seed);
    let row = items(&f).into_iter().find(|i| i.id == "starter-capsules").unwrap();
    assert_eq!(row.state, ItemState::Missing);
    assert!(row.detail.contains(&format!("{name}: digest does not match the pin")), "{row:?}");
}

#[test]
fn a_member_choice_is_off_not_missing() {
    let d = tmp("off");
    let mut f = complete(&d);
    f.mcp_settings.node = false;
    let r = report(items(&f), vec![], None);
    assert_eq!(state_of(&r.items, "node-mcp").state, ItemState::Off);
    assert_eq!(r.missing_required, 0);
    f.mcp_settings.node = true;
    f.node_mcp = Some(NodeMcpFacts {
        enabled: false,
        running: false,
        ..node_ok()
    });
    let r = report(items(&f), vec![], None);
    assert_eq!(state_of(&r.items, "node-mcp").state, ItemState::Off);
    assert!(state_of(&r.items, "node-mcp").detail.contains("Settings"));
}

#[test]
fn every_bundle_config_ships_what_the_self_check_requires() {
    // The capsules and first-party skills the check requires are in every bundle config's
    // resources, so a release build always passes those rows.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("tauri.conf.json")).unwrap()).unwrap();
    let res: Vec<String> = base["bundle"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    assert!(res.iter().any(|r| r.starts_with("skills/")), "{res:?}");
    for (name, _) in BUNDLED_CAPSULES {
        assert!(res.iter().any(|r| r.contains(&format!("capsules/{name}"))), "{name} in {res:?}");
    }
    for n in FIRST_PARTY_SKILLS {
        assert!(dir.join("skills").join(n).join("SKILL.md").is_file(), "{n}");
    }
}

#[test]
fn hermes_setup_check_is_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in ["hermes_setup_check", "hermes_setup_repair", "hermes_setup_enable_node_tools"] {
        assert!(lib.contains(&format!("hermes_setup::{cmd}")), "{cmd}");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd}");
    }
    // The existing-data probe is the first thing setup does, before anything writes.
    let setup = lib.find(".setup(|app| {").expect("setup");
    let probe = lib.find("hermes_setup::had_existing_data(").expect("probe");
    let gate = lib.find("network_profile::install_network_gate();").expect("first step");
    assert!(setup < probe && probe < gate, "the probe must run first in setup");
    assert!(lib.contains("on for new installs only"));
    assert!(include_str!("hermes_mcp.rs").contains("Owner 2026-11-14: on for new installs"));
    // Launch provisioning runs before the node MCP server's state is built (so its default applies).
    let p = lib.find("hermes_setup::provision(").expect("provision is called in setup");
    let n = lib.find("node_mcp::build_node_mcp_state(").expect("node mcp state");
    assert!(p < n, "provision must run before build_node_mcp_state");
}
