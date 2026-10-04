// HUP-S6.2 / US-6.4 — contract templates for members: the catalog form, the folder-grant check,
// rendering through the real citrate-templates renderer, the pinned libraries, and the bundle
// resources.
use super::*;
use crate::agent_grants::{Access, Grant, GrantKind, GrantState, GrantStore};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

const OWNER: &str = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

fn root() -> PathBuf {
    templates_root(None).expect("the source templates in a debug build")
}

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "citrate-tf-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("mkdir");
        Tmp(p.canonicalize().expect("canonical"))
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn grant(root: &Path, access: Access, scope: &str) -> Grant {
    Grant {
        id: "g-1".into(),
        kind: GrantKind::Folder,
        root: root.display().to_string(),
        access,
        scope: scope.into(),
        granted_at: 100,
        expires_at: None,
        granted_by: "member".into(),
        reason: "test".into(),
        revoked_at: None,
    }
}

fn state(grants: Vec<Grant>) -> GrantState {
    GrantState {
        version: 1,
        next_id: 2,
        grants,
    }
}

fn input(template: &str, out: &Path) -> TemplateRenderInput {
    TemplateRenderInput {
        template: template.into(),
        params: BTreeMap::from([
            ("name".to_string(), "Lemon Drops".to_string()),
            ("symbol".to_string(), "LEMON".to_string()),
            ("owner".to_string(), OWNER.to_string()),
        ]),
        out_dir: out.display().to_string(),
    }
}

// ------------------------------------------------------------------------ catalog

#[test]
fn the_catalog_lists_every_template_with_its_form() {
    let set = TemplateSet::open(&root()).expect("open");
    let cat = catalog(&set, TemplateTier::T1);
    let ids: Vec<&str> = cat.templates.iter().map(|t| t.id.as_str()).collect();
    for want in [
        "erc20",
        "erc721",
        "erc721-solady",
        "erc1155",
        "governor",
        "hello-mint",
    ] {
        assert!(ids.contains(&want), "{want} missing from {ids:?}");
    }
    assert_eq!(cat.tier, "T1");
    assert_eq!(cat.medusa_budget.test_limit, 50_000);
    let erc20 = cat
        .templates
        .iter()
        .find(|t| t.id == "erc20")
        .expect("erc20");
    let keys: Vec<&str> = erc20.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, ["name", "symbol", "supply", "owner"]);
    let supply = &erc20.fields[2];
    assert_eq!(supply.default.as_deref(), Some("1000000"));
    assert_eq!((supply.min, supply.max), (Some(1), Some(1_000_000_000_000)));
    assert!(!supply.required);
    assert!(erc20.fields[0].required, "name has no default");
    assert_eq!(erc20.kind, "contract");
    let hm = cat
        .templates
        .iter()
        .find(|t| t.id == "hello-mint")
        .expect("hello-mint");
    assert_eq!(hm.kind, "dapp");
    assert!(hm.fields.iter().any(|f| f.key == "price"));
}

#[test]
fn each_tier_carries_its_own_budget() {
    let set = TemplateSet::open(&root()).expect("open");
    let limits: Vec<u64> = [TemplateTier::T0, TemplateTier::T1, TemplateTier::T2]
        .into_iter()
        .map(|t| catalog(&set, t).medusa_budget.test_limit)
        .collect();
    assert_eq!(limits, [10_000, 50_000, 200_000]);
    assert_eq!(renderer_tier(crate::tier::Tier::T2), TemplateTier::T2);
}

// ------------------------------------------------------------------------ grants

#[test]
fn only_an_active_matching_folder_grant_covers_a_target() {
    let t = Tmp::new("grant");
    let target = t.0.join("proj");
    let now = 1_000;
    assert!(grant_covers(
        &state(vec![grant(&t.0, Access::Write, "subtree")]),
        &target,
        Access::Write,
        now
    ));
    // Read does not imply write, and write does not imply read.
    assert!(!grant_covers(
        &state(vec![grant(&t.0, Access::Read, "subtree")]),
        &target,
        Access::Write,
        now
    ));
    assert!(!grant_covers(
        &state(vec![grant(&t.0, Access::Write, "subtree")]),
        &target,
        Access::Read,
        now
    ));
    // Revoked, expired, not yet active, or a sibling folder: no.
    let mut g = grant(&t.0, Access::Write, "subtree");
    g.revoked_at = Some(500);
    assert!(!grant_covers(&state(vec![g]), &target, Access::Write, now));
    let mut g = grant(&t.0, Access::Write, "subtree");
    g.expires_at = Some(now);
    assert!(!grant_covers(&state(vec![g]), &target, Access::Write, now));
    let mut g = grant(&t.0, Access::Write, "subtree");
    g.granted_at = now + 1;
    assert!(!grant_covers(&state(vec![g]), &target, Access::Write, now));
    assert!(!grant_covers(
        &state(vec![grant(&t.0.join("other"), Access::Write, "subtree")]),
        &target,
        Access::Write,
        now
    ));
    // A prefix that is not a path ancestor does not count (`/a/proj2` is not under `/a/proj`).
    assert!(!grant_covers(
        &state(vec![grant(&t.0.join("pro"), Access::Write, "subtree")]),
        &target,
        Access::Write,
        now
    ));
    // Shallow: the folder and its direct children only.
    let shallow = state(vec![grant(&t.0, Access::Write, "shallow")]);
    assert!(grant_covers(&shallow, &target, Access::Write, now));
    assert!(!grant_covers(
        &shallow,
        &target.join("deeper"),
        Access::Write,
        now
    ));
    // The read-only full-access window never covers a write.
    let mut full = grant(&t.0, Access::Read, "subtree");
    full.kind = GrantKind::FullAccess;
    assert!(!grant_covers(
        &state(vec![full]),
        &target,
        Access::Write,
        now
    ));
}

#[test]
fn require_grant_refuses_relative_dotdot_and_ungranted_folders() {
    let t = Tmp::new("req");
    let store = GrantStore::new(t.0.join("agent"), t.0.clone());
    std::fs::create_dir_all(t.0.join("agent")).expect("mkdir");
    let out = t.0.join("work/proj");
    assert!(require_grant(&store, Path::new("rel/proj"), Access::Write, 1_000).is_err());
    assert!(
        require_grant(&store, &t.0.join("work/../proj"), Access::Write, 1_000)
            .unwrap_err()
            .contains("..")
    );
    let err = require_grant(&store, &out, Access::Write, 1_000).unwrap_err();
    assert!(err.contains("Folder access"), "{err}");
    std::fs::create_dir_all(t.0.join("work")).expect("mkdir");
    store
        .save(&state(vec![grant(
            &t.0.join("work"),
            Access::Write,
            "subtree",
        )]))
        .expect("save");
    assert_eq!(
        require_grant(&store, &out, Access::Write, 1_000).expect("granted"),
        out
    );
}

// ------------------------------------------------------------------------ render

#[test]
fn rendering_an_erc20_writes_the_project_and_reports_missing_libraries_honestly() {
    let t = Tmp::new("render");
    let out = t.0.join("lemon");
    let v =
        render_into(&root(), &input("erc20", &out), TemplateTier::T0, &out, None).expect("render");
    assert_eq!(v.template, "erc20");
    assert_eq!(v.tier, "T0");
    assert_eq!(
        v.params.get("contract").map(String::as_str),
        Some("LemonDrops")
    );
    assert_eq!(v.medusa_budget.as_ref().map(|b| b.test_limit), Some(10_000));
    assert_eq!(v.contract_dir, out.display().to_string());
    assert!(out.join("src/Token.sol").is_file());
    assert!(out.join("citrate-template.lock.json").is_file());
    let token = std::fs::read_to_string(out.join("src/Token.sol")).expect("read");
    assert!(token.contains("contract LemonDrops"));
    let medusa: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("medusa.json")).expect("read"))
            .expect("json");
    assert_eq!(medusa["fuzzing"]["testLimit"], 10_000);
    // No library cache: every pinned library is reported missing, none silently skipped.
    assert_eq!(v.deps.len(), 3);
    assert!(v.deps.iter().all(|d| !d.installed && d.note.is_some()));
}

#[test]
fn a_bad_parameter_or_a_used_folder_writes_nothing() {
    let t = Tmp::new("refuse");
    let out = t.0.join("x");
    let mut bad = input("erc20", &out);
    bad.params.insert("name".into(), "Robert\"); drop".into());
    assert!(render_into(&root(), &bad, TemplateTier::T0, &out, None).is_err());
    assert!(!out.exists(), "a refusal leaves nothing behind");
    let mut extra = input("erc20", &out);
    extra.params.insert("price".into(), "1".into());
    assert!(render_into(&root(), &extra, TemplateTier::T0, &out, None).is_err());
    std::fs::create_dir_all(&out).expect("mkdir");
    std::fs::write(out.join("keep.txt"), "mine").expect("write");
    assert!(render_into(&root(), &input("erc20", &out), TemplateTier::T0, &out, None).is_err());
    assert_eq!(
        std::fs::read_to_string(out.join("keep.txt")).expect("read"),
        "mine"
    );
    assert!(render_into(
        &root(),
        &input("nope", &t.0.join("y")),
        TemplateTier::T0,
        &t.0.join("y"),
        None
    )
    .is_err());
}

#[test]
fn a_dapp_renders_its_contract_project_into_contracts() {
    let t = Tmp::new("dapp");
    let out = t.0.join("hm");
    let v = render_into(
        &root(),
        &input("hello-mint", &out),
        TemplateTier::T1,
        &out,
        None,
    )
    .expect("render");
    assert_eq!(v.contract_dir, out.join("contracts").display().to_string());
    assert!(out.join("contracts/src").is_dir());
    assert!(out.join("app/package.json").is_file());
}

/// A library cache entry as a git checkout at `commit`.
fn cached(cache: &Path, name: &str, commit: &str) {
    let d = cache.join(name);
    std::fs::create_dir_all(d.join(".git")).expect("mkdir");
    std::fs::create_dir_all(d.join("src")).expect("mkdir");
    std::fs::write(d.join(".git/HEAD"), format!("{commit}\n")).expect("write");
    std::fs::write(d.join("src/Lib.sol"), "// lib\n").expect("write");
}

#[test]
fn pinned_libraries_are_copied_only_at_their_pinned_commit() {
    let t = Tmp::new("deps");
    let cache = t.0.join("cache");
    let deps: serde_json::Value = serde_json::from_str(
        r#"{"forge-std":{"commit":"aaa","dir":"lib/forge-std"},
            "solady":{"commit":"bbb","dir":"lib/solady"},
            "openzeppelin-contracts":{"commit":"ccc","dir":"lib/openzeppelin-contracts"},
            "evil":{"commit":"ddd","dir":"../escape"}}"#,
    )
    .expect("json");
    cached(&cache, "forge-std", "aaa");
    cached(&cache, "solady", "not-bbb");
    let proj = t.0.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir");
    let st = install_deps(&deps, Some(&cache), &proj);
    let by = |n: &str| st.iter().find(|d| d.name == n).cloned().expect("dep");
    assert!(by("forge-std").installed);
    assert!(proj.join("lib/forge-std/src/Lib.sol").is_file());
    assert!(
        !proj.join("lib/forge-std/.git").exists(),
        "history is not copied"
    );
    assert!(!by("solady").installed);
    assert!(by("solady")
        .note
        .unwrap_or_default()
        .contains("not the pinned commit"));
    assert!(!by("openzeppelin-contracts").installed);
    assert!(!by("evil").installed);
    assert!(!t.0.join("escape").exists());
}

#[test]
fn the_pinned_lock_matches_what_the_renderer_records() {
    let deps = deps_lock(&root()).expect("lock");
    for name in ["openzeppelin-contracts", "solady", "forge-std"] {
        assert!(
            deps[name]["commit"].as_str().is_some_and(|c| c.len() == 40),
            "{name}"
        );
    }
}

// ------------------------------------------------------------------------ bundle resources

/// Every bundle config ships the templates, so `template_list` works in a packaged app.
#[test]
fn every_bundle_config_ships_the_templates() {
    let src_tauri = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ids: Vec<String> = TemplateSet::open(&root())
        .expect("open")
        .ids()
        .map(str::to_string)
        .collect();
    let mut checked = 0;
    for entry in std::fs::read_dir(src_tauri).expect("src-tauri").flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if !(file.starts_with("tauri.") && file.ends_with(".conf.json")) {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).expect("read");
        let json: serde_json::Value = serde_json::from_str(&text).expect("json");
        let listed: Vec<&str> = json
            .pointer("/bundle/resources")
            .and_then(|r| r.as_array())
            .expect("resources")
            .iter()
            .filter_map(|r| r.as_str())
            .collect();
        assert!(listed.contains(&"../templates/*.json"), "{file}");
        assert!(listed.contains(&"../templates/_common/**/*"), "{file}");
        for id in &ids {
            let want = format!("../templates/{id}/**/*");
            assert!(listed.contains(&want.as_str()), "{file} must bundle {want}");
        }
        // The renderer's own Rust sources are not app resources.
        assert!(!listed.iter().any(|r| r.contains("renderer")), "{file}");
        checked += 1;
    }
    assert!(
        checked >= 6,
        "expected the base config and the overlays, saw {checked}"
    );
}

#[test]
fn the_bundled_root_is_preferred_and_found_under_up() {
    let t = Tmp::new("res");
    let up = t.0.join("_up_/templates");
    std::fs::create_dir_all(&up).expect("mkdir");
    std::fs::write(up.join("medusa-budgets.json"), "{}").expect("write");
    assert_eq!(templates_root(Some(&t.0)).expect("root"), up);
    // A resource folder without templates falls back to the source tree in a debug build.
    let empty = Tmp::new("res-empty");
    assert_eq!(templates_root(Some(&empty.0)).expect("root"), root());
}
