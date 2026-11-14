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
fn first_run_defaults_offer_the_node_mcp_read_tools_and_never_override_a_choice() {
    let d = tmp("defaults");
    let hermes = d.join("hermes");
    let node = d.join("node-mcp");
    let wrote = provision_defaults(&hermes, Some(&node));
    assert_eq!(wrote.len(), 2, "{wrote:?}");
    let s = crate::hermes_mcp::load_settings(&hermes);
    assert!(s.node && !s.mem && !s.scan, "{s:?}");
    let cfg: crate::node_mcp::NodeMcpConfig =
        serde_json::from_str(&std::fs::read_to_string(node.join("config.json")).unwrap()).unwrap();
    assert!(cfg.enabled);
    assert_eq!(cfg.port, None, "the default port");
    // Read tools only, whatever the default.
    const { assert!(!crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS) };

    // The member turns both off; the next launch keeps their choice.
    crate::hermes_mcp::save_settings(&hermes, &McpSettings::default()).unwrap();
    std::fs::write(node.join("config.json"), br#"{"enabled":false}"#).unwrap();
    assert!(provision_defaults(&hermes, Some(&node)).is_empty());
    assert!(!crate::hermes_mcp::load_settings(&hermes).node);
    assert!(std::fs::read_to_string(node.join("config.json")).unwrap().contains("false"));
}

#[cfg(unix)]
#[test]
fn first_run_default_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("mode");
    provision_defaults(&d.join("hermes"), Some(&d.join("node-mcp")));
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
    let r = report(items(&f), vec![]);
    for i in &r.items {
        if i.need == Need::Required {
            assert_eq!(i.state, ItemState::Ok, "{i:?}");
        }
    }
    assert_eq!(r.missing_required, 0);
    assert!(state_of(&r.items, "node-mcp").detail.contains("127.0.0.1:47204/mcp"));
    assert!(state_of(&r.items, "first-party-skills").detail.contains("bundled"));
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
    let r = report(items(&f), vec![]);
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
    let r = report(items(&f), vec![]);
    assert_eq!(state_of(&r.items, "node-mcp").state, ItemState::Off);
    assert_eq!(r.missing_required, 0);
    f.mcp_settings.node = true;
    f.node_mcp = Some(NodeMcpFacts {
        enabled: false,
        running: false,
        ..node_ok()
    });
    let r = report(items(&f), vec![]);
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
    assert!(skill_md_count(&dir.join("skills")) > 0);
}

#[test]
fn hermes_setup_check_is_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    assert!(lib.contains("hermes_setup::hermes_setup_check"));
    assert!(acl.contains("\"hermes_setup_check\""));
    // Launch provisioning runs before the node MCP server's state is built (so its default applies).
    let p = lib.find("hermes_setup::provision(").expect("provision is called in setup");
    let n = lib.find("node_mcp::build_node_mcp_state(").expect("node mcp state");
    assert!(p < n, "provision must run before build_node_mcp_state");
}
