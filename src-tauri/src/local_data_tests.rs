// HUP-S10.5 — "delete my local data": dry-run plan, confirmation, and execution tests.
// Written red-first (module body absent, so these failed to compile), then implemented.

use super::*;
use std::collections::BTreeSet;
use std::sync::Mutex;

fn tmp_root(tag: &str) -> std::path::PathBuf {
    use rand::RngCore;
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    let d = std::env::temp_dir().join(format!("n4-localdata-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A fake app layout under `base`: data dir + cache dir, both named after the bundle id.
fn layout(base: &std::path::Path) -> AppRoots {
    let data = base.join("data").join(BUNDLE_ID);
    let cache = base.join("cache").join(BUNDLE_ID);
    std::fs::create_dir_all(data.join("models")).unwrap();
    std::fs::create_dir_all(data.join("node")).unwrap();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(data.join("custody.enc"), b"Zq9xK").unwrap();
    std::fs::write(data.join("config.json"), b"{}").unwrap();
    std::fs::write(data.join("models").join("m.gguf"), vec![0u8; 4096]).unwrap();
    std::fs::write(data.join("node").join("db"), vec![0u8; 100]).unwrap();
    std::fs::write(cache.join("c.bin"), vec![0u8; 10]).unwrap();
    AppRoots {
        data_dir: data,
        others: vec![(cache, RootKind::Cache)],
    }
}

/// An in-memory keychain across services.
#[derive(Default)]
struct FakeKeychain {
    entries: Mutex<BTreeSet<(String, String)>>,
    texts: Mutex<Vec<(String, String, String)>>,
    down: bool,
}

impl FakeKeychain {
    fn with(entries: &[(&str, &str)]) -> Self {
        let k = FakeKeychain::default();
        for (s, a) in entries {
            k.entries
                .lock()
                .unwrap()
                .insert((s.to_string(), a.to_string()));
        }
        k
    }
    fn has(&self, s: &str, a: &str) -> bool {
        self.entries
            .lock()
            .unwrap()
            .contains(&(s.to_string(), a.to_string()))
    }
}

impl KeychainOps for FakeKeychain {
    fn present(&self, service: &str, account: &str) -> Option<bool> {
        if self.down {
            return None;
        }
        Some(self.has(service, account))
    }
    fn read_text(&self, service: &str, account: &str) -> Option<String> {
        self.texts
            .lock()
            .unwrap()
            .iter()
            .find(|(s, a, _)| s == service && a == account)
            .map(|(_, _, t)| t.clone())
    }
    fn delete(&self, service: &str, account: &str) -> std::result::Result<(), String> {
        if self.down {
            return Err("keychain unreachable".into());
        }
        self.entries
            .lock()
            .unwrap()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

fn entry<'a>(plan: &'a DataPlan, suffix: &str) -> &'a PlanEntry {
    plan.entries
        .iter()
        .find(|e| e.path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no entry ending {suffix}: {:#?}", plan.entries))
}

// ---------- the dry-run plan ----------

#[test]
fn plan_lists_every_top_level_item_with_its_size_and_keeps_the_wallet_by_default() {
    let base = tmp_root("plan");
    let roots = layout(&base);
    let kc = FakeKeychain::default();
    let plan = build_plan(&roots, &kc, DeleteOptions::default());
    assert_eq!(entry(&plan, "custody.enc").action, Action::Keep);
    assert_eq!(entry(&plan, "config.json").action, Action::Delete);
    assert_eq!(entry(&plan, "models").action, Action::Delete);
    assert_eq!(entry(&plan, "models").bytes, 4096);
    assert_eq!(entry(&plan, "node").action, Action::Delete);
    assert_eq!(entry(&plan, "c.bin").action, Action::Delete);
    assert_eq!(plan.delete_bytes, 4096 + 100 + 2 + 10);
    assert_eq!(plan.keep_bytes, 5);
    assert_eq!(plan.confirm_phrase, CONFIRM_PHRASE);
    assert!(plan.wallet_confirm_phrase.is_none());
    // Building the plan changes nothing on disk.
    assert!(roots.data_dir.join("custody.enc").exists());
    assert!(roots.data_dir.join("models").join("m.gguf").exists());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn keep_models_and_include_wallet_change_exactly_their_items() {
    let base = tmp_root("opts");
    let roots = layout(&base);
    let kc = FakeKeychain::default();
    let plan = build_plan(
        &roots,
        &kc,
        DeleteOptions {
            include_wallet: true,
            keep_models: true,
        },
    );
    assert_eq!(entry(&plan, "custody.enc").action, Action::Delete);
    assert_eq!(entry(&plan, "models").action, Action::Keep);
    assert_eq!(plan.wallet_confirm_phrase, Some(WALLET_CONFIRM_PHRASE));
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_root_not_named_for_this_app_is_never_planned() {
    let base = tmp_root("foreign");
    let mut roots = layout(&base);
    let foreign = base.join("Documents");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("precious.txt"), b"mine").unwrap();
    roots.others.push((foreign.clone(), RootKind::Cache));
    let plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    assert!(plan.entries.iter().all(|e| !e.path.contains("Documents")));
    assert!(plan.notes.iter().any(|n| n.contains("skipped")));
    let report = execute_plan(&plan, &FakeKeychain::default());
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(foreign.join("precious.txt").exists());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn a_data_dir_not_named_for_this_app_plans_nothing_there() {
    let base = tmp_root("baddata");
    let data = base.join("home");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("notes.txt"), b"x").unwrap();
    let roots = AppRoots {
        data_dir: data.clone(),
        others: vec![],
    };
    let plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    assert!(plan.entries.is_empty());
    execute_plan(&plan, &FakeKeychain::default());
    assert!(data.join("notes.txt").exists());
    std::fs::remove_dir_all(&base).unwrap();
}

// ---------- the keychain list ----------

#[test]
fn keychain_list_is_core_owned_and_never_touches_the_shared_custody_accounts() {
    let kc = FakeKeychain::with(&[("ai.citrate.core", "node-storage-key")]);
    let plan = build_plan(
        &AppRoots {
            data_dir: tmp_root("kc").join(BUNDLE_ID),
            others: vec![],
        },
        &kc,
        DeleteOptions::default(),
    );
    for k in &plan.keychain {
        assert!(
            !(k.service == "ai.citrate.core" && k.account.starts_with("custody-")),
            "the legacy service's custody accounts belong to another app: {k:?}"
        );
    }
    let node = plan
        .keychain
        .iter()
        .find(|k| k.account == "node-storage-key")
        .unwrap();
    assert_eq!(node.present, Some(true));
    assert_eq!(node.action, Action::Delete);
    let wallet: Vec<_> = plan
        .keychain
        .iter()
        .filter(|k| k.service == "ai.citrate.core.custody")
        .collect();
    assert_eq!(wallet.len(), 4);
    assert!(wallet.iter().all(|k| k.action == Action::Keep));
}

#[test]
fn a_custom_default_ai_provider_is_listed_and_a_malformed_id_is_not() {
    let kc = FakeKeychain::default();
    kc.texts.lock().unwrap().push((
        "ai.citrate.core".into(),
        "ai-default".into(),
        "my-local".into(),
    ));
    let plan = build_plan(
        &AppRoots {
            data_dir: tmp_root("ai").join(BUNDLE_ID),
            others: vec![],
        },
        &kc,
        DeleteOptions::default(),
    );
    assert!(plan.keychain.iter().any(|k| k.account == "ai-my-local"));
    let kc2 = FakeKeychain::default();
    kc2.texts.lock().unwrap().push((
        "ai.citrate.core".into(),
        "ai-default".into(),
        "../custody-master-key".into(),
    ));
    let plan2 = build_plan(
        &AppRoots {
            data_dir: tmp_root("ai2").join(BUNDLE_ID),
            others: vec![],
        },
        &kc2,
        DeleteOptions::default(),
    );
    assert!(plan2
        .keychain
        .iter()
        .all(|k| !k.account.contains("custody") || k.service == "ai.citrate.core.custody"));
}

#[test]
fn an_unreachable_keychain_is_reported_not_guessed() {
    let kc = FakeKeychain {
        down: true,
        ..Default::default()
    };
    let plan = build_plan(
        &AppRoots {
            data_dir: tmp_root("down").join(BUNDLE_ID),
            others: vec![],
        },
        &kc,
        DeleteOptions::default(),
    );
    assert!(plan.keychain.iter().all(|k| k.present.is_none()));
    assert!(plan.notes.iter().any(|n| n.contains("keychain")));
}

// ---------- confirmation ----------

#[test]
fn deletion_needs_the_exact_confirmation_phrase() {
    let o = DeleteOptions::default();
    assert_eq!(
        check_confirmations(o, "", None).unwrap_err(),
        DataError::NotConfirmed
    );
    assert_eq!(
        check_confirmations(o, "delete", None).unwrap_err(),
        DataError::NotConfirmed
    );
    assert!(check_confirmations(o, "delete my local data", None).is_ok());
    assert!(check_confirmations(o, "  Delete   my local DATA ", None).is_ok());
}

#[test]
fn deleting_the_wallet_needs_a_second_phrase() {
    let o = DeleteOptions {
        include_wallet: true,
        keep_models: false,
    };
    assert_eq!(
        check_confirmations(o, "delete my local data", None).unwrap_err(),
        DataError::WalletNotConfirmed
    );
    assert_eq!(
        check_confirmations(o, "delete my local data", Some("delete my local data")).unwrap_err(),
        DataError::WalletNotConfirmed
    );
    assert!(check_confirmations(o, "delete my local data", Some("delete my wallet")).is_ok());
}

// ---------- execution ----------

#[test]
fn execute_deletes_exactly_the_planned_items() {
    let base = tmp_root("exec");
    let roots = layout(&base);
    let kc = FakeKeychain::with(&[
        ("ai.citrate.core", "node-storage-key"),
        ("ai.citrate.core", "memory-store-key"),
        ("ai.citrate.core.custody", "custody-master-key"),
        ("ai.citrate.core", "custody-master-key"), // another app's: must survive
    ]);
    let plan = build_plan(
        &roots,
        &kc,
        DeleteOptions {
            include_wallet: false,
            keep_models: true,
        },
    );
    let report = execute_plan(&plan, &kc);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(roots.data_dir.join("custody.enc").exists());
    assert!(roots.data_dir.join("models").join("m.gguf").exists());
    assert!(!roots.data_dir.join("config.json").exists());
    assert!(!roots.data_dir.join("node").exists());
    assert!(!roots.others[0].0.join("c.bin").exists());
    assert!(!kc.has("ai.citrate.core", "node-storage-key"));
    assert!(!kc.has("ai.citrate.core", "memory-store-key"));
    assert!(kc.has("ai.citrate.core.custody", "custody-master-key"));
    assert!(kc.has("ai.citrate.core", "custody-master-key"));
    assert!(report
        .keychain_deleted
        .contains(&"ai.citrate.core/node-storage-key".to_string()));
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn execute_with_wallet_removes_the_vault_and_its_keys() {
    let base = tmp_root("wallet");
    let roots = layout(&base);
    let kc = FakeKeychain::with(&[
        ("ai.citrate.core.custody", "custody-master-key"),
        ("ai.citrate.core.custody", "custody-auto-passphrase"),
    ]);
    let plan = build_plan(
        &roots,
        &kc,
        DeleteOptions {
            include_wallet: true,
            keep_models: false,
        },
    );
    let report = execute_plan(&plan, &kc);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(!roots.data_dir.join("custody.enc").exists());
    assert!(!kc.has("ai.citrate.core.custody", "custody-master-key"));
    assert!(!kc.has("ai.citrate.core.custody", "custody-auto-passphrase"));
    std::fs::remove_dir_all(&base).unwrap();
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_app_data_is_removed_without_following_it() {
    let base = tmp_root("link");
    let roots = layout(&base);
    let outside = base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), b"not app data").unwrap();
    std::os::unix::fs::symlink(&outside, roots.data_dir.join("linked")).unwrap();
    let plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    // A link's size is not the target's size.
    assert!(entry(&plan, "linked").bytes < 1024);
    let report = execute_plan(&plan, &FakeKeychain::default());
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(!roots.data_dir.join("linked").exists());
    assert!(outside.join("keep.txt").exists());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn keychain_failures_are_reported_not_hidden() {
    let base = tmp_root("kcfail");
    let roots = layout(&base);
    let up = FakeKeychain::with(&[("ai.citrate.core", "node-storage-key")]);
    let plan = build_plan(&roots, &up, DeleteOptions::default());
    let down = FakeKeychain {
        down: true,
        ..Default::default()
    };
    let report = execute_plan(&plan, &down);
    assert!(report
        .keychain_failed
        .iter()
        .any(|f| f.item == "ai.citrate.core/node-storage-key"));
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn plan_serializes_without_secret_values() {
    let base = tmp_root("ser");
    let roots = layout(&base);
    let plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains("confirmPhrase"));
    assert!(!json.contains("Zq9xK")); // file contents are never read into the plan
    std::fs::remove_dir_all(&base).unwrap();
}

// ---------- adversarial review (HUP-S10.5) ----------

/// Defence in depth: even a plan entry that is not a direct child of an app-owned folder
/// is refused at execution time and reported, never deleted.
#[test]
fn execute_refuses_an_entry_outside_an_app_folder() {
    let base = tmp_root("forged");
    let roots = layout(&base);
    let outside = base.join("not-the-app");
    std::fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("keep.txt");
    std::fs::write(&victim, b"member file").unwrap();
    let mut plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    plan.entries.push(PlanEntry {
        path: victim.display().to_string(),
        kind: RootKind::Data,
        bytes: 11,
        action: Action::Delete,
        reason: None,
    });
    let report = execute_plan(&plan, &FakeKeychain::default());
    assert!(
        victim.exists(),
        "a path outside the app folders was deleted"
    );
    assert!(report
        .failed
        .iter()
        .any(|f| f.item == victim.display().to_string()));
    std::fs::remove_dir_all(&base).unwrap();
}

/// The kept wallet vault also holds the sign-in session and connected-account tokens; the
/// dry run must say so, so "keep my wallet" is not mistaken for "every token is gone".
#[test]
fn the_kept_wallet_vault_says_it_also_holds_sign_in_and_connection_tokens() {
    let base = tmp_root("vaultnote");
    let roots = layout(&base);
    let plan = build_plan(&roots, &FakeKeychain::default(), DeleteOptions::default());
    let reason = entry(&plan, "custody.enc")
        .reason
        .clone()
        .unwrap_or_default();
    assert!(reason.contains("sign-in"), "{reason}");
    assert!(reason.contains("connected"), "{reason}");
    std::fs::remove_dir_all(&base).unwrap();
}
