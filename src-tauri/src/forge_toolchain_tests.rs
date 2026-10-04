// HUP-S6.3 — the toolchain switch: default off changes nothing; on sets the sidecar flag, the
// search path (installed components first) and the pinned solc; the Hermes manager passes these
// keys and nothing else.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

struct Tmp(PathBuf);
impl Tmp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "citrate-ftc-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("mkdir");
        Tmp(p)
    }
    fn places(&self) -> ToolchainPlaces {
        ToolchainPlaces {
            components_root: self.0.join("components"),
            home: self.0.join("home"),
        }
    }
    fn program(&self, rel: &str) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(&p, "#!/bin/sh\n").expect("write");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn off_by_default_and_off_emits_nothing() {
    let t = Tmp::new();
    assert_eq!(load(&t.0).expect("load"), ToolchainSettings::default());
    assert!(!ToolchainSettings::default().enabled);
    assert!(sidecar_env(&ToolchainSettings::default(), &t.places()).is_empty());
}

#[test]
fn on_sets_the_flag_the_search_path_and_the_pinned_solc() {
    let t = Tmp::new();
    t.program(&format!(
        "home/Library/Application Support/svm/{PINNED_SOLC}/solc-{PINNED_SOLC}"
    ));
    let env = sidecar_env(&ToolchainSettings { enabled: true }, &t.places());
    let get = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
    assert_eq!(get(TOOLCHAIN_ENV).as_deref(), Some("1"));
    let path = get(TOOLCHAIN_PATH_ENV).expect("path");
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    assert_eq!(
        dirs[0],
        t.0.join("home/.foundry/bin"),
        "no component installed: user dirs first"
    );
    assert!(dirs.contains(&t.0.join("home/.local/pipx/venvs/slither-analyzer/bin")));
    assert!(dirs.contains(&PathBuf::from("/usr/bin")));
    assert!(dirs.iter().all(|d| d.is_absolute()));
    assert!(get(SOLC_ENV)
        .expect("solc")
        .ends_with(&format!("solc-{PINNED_SOLC}")));
    // Every key it emits is one the Hermes manager lets through.
    assert!(env
        .iter()
        .all(|(k, _)| SIDECAR_ENV_KEYS.contains(&k.as_str())));
}

#[test]
fn no_solc_means_no_solc_variable_and_a_notice() {
    let t = Tmp::new();
    let env = sidecar_env(&ToolchainSettings { enabled: true }, &t.places());
    assert!(!env.iter().any(|(k, _)| k == SOLC_ENV));
    let st = status_for(ToolchainSettings { enabled: true }, &t.places(), None);
    assert!(st.notices.iter().any(|n| n.contains("solc")));
}

#[test]
fn status_reports_each_program_honestly() {
    let t = Tmp::new();
    t.program("home/.foundry/bin/forge");
    let st = status_for(ToolchainSettings::default(), &t.places(), None);
    let by = |p: &str| {
        st.programs
            .iter()
            .find(|x| x.program == p)
            .cloned()
            .expect("program")
    };
    assert!(by("forge")
        .path
        .expect("forge")
        .ends_with("home/.foundry/bin/forge"));
    assert_eq!(by("forge").tool, "forge_test");
    // medusa and aderyn are absent in this scratch home, so they read "not installed"
    // unless the machine has them in a system folder.
    let missing = ["slither", "aderyn", "medusa"]
        .iter()
        .filter(|p| by(p).path.is_none())
        .count();
    if missing > 0 {
        assert!(st.notices.iter().any(|n| n.contains("Not installed")));
    }
    assert!(st.applies_on_restart);
}

#[test]
fn installed_components_come_first_on_the_search_path() {
    use citrate_components::install::{InstalledComponent, InstalledVersion, StoreState};
    use citrate_components::platform::Platform;
    let Some(platform) = Platform::current() else {
        return;
    };
    let t = Tmp::new();
    let root = t.0.join("components");
    std::fs::create_dir_all(root.join("medusa/1.5.1")).expect("mkdir");
    let v = InstalledVersion {
        version: "1.5.1".into(),
        sha256: "00".repeat(32),
        dir: "1.5.1".into(),
        platform,
        installed_at: 1,
        manifest_sequence: 1,
    };
    let mut st = StoreState::default();
    st.components.insert(
        "medusa".into(),
        InstalledComponent {
            current: v,
            previous: None,
        },
    );
    std::fs::write(
        root.join("state.json"),
        serde_json::to_vec(&st).expect("json"),
    )
    .expect("write");
    let path = search_path(&t.places());
    // medusa's entry point is `medusa` at the top of its version folder.
    assert_eq!(path[0], root.join("medusa/1.5.1"));
    assert_eq!(path[1], t.0.join("home/.foundry/bin"));
}

#[test]
fn settings_round_trip_and_a_corrupt_file_stays_off() {
    let t = Tmp::new();
    save(&t.0, &ToolchainSettings { enabled: true }).expect("save");
    assert!(load(&t.0).expect("load").enabled);
    std::fs::write(t.0.join(SETTINGS_FILE), "{not json").expect("write");
    assert!(load(&t.0).is_err());
    let src = file_env_source(t.0.clone(), t.places());
    assert!(src().is_empty(), "a corrupt settings file means off");
}

#[test]
fn which_finds_the_first_match_only_for_files() {
    let t = Tmp::new();
    t.program("a/forge");
    t.program("b/forge");
    std::fs::create_dir_all(t.0.join("c/medusa")).expect("mkdir");
    let path = vec![t.0.join("a"), t.0.join("b"), t.0.join("c")];
    assert_eq!(which("forge", &path), Some(t.0.join("a/forge")));
    assert_eq!(which("medusa", &path), None, "a folder is not a program");
}
