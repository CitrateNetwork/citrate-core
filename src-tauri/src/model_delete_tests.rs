// Deleting a downloaded model: only a verified model file directly in the models folder, never a
// path, link, bundled or in-use model, never during its download; everything it left is removed
// and the freed bytes are returned.
use super::*;
use std::cell::RefCell;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("model-delete-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    std::fs::canonicalize(&d).expect("canon")
}

/// A verified model as a download leaves it: the GGUF, its verified status and (optionally)
/// a leftover marker and `.part`.
fn verified_model(dir: &Path, file: &str, bytes: usize) {
    std::fs::write(dir.join(file), vec![7u8; bytes]).expect("gguf");
    std::fs::write(dir.join(format!("{file}.status.json")), r#"{"verified":true}"#).expect("status");
}

struct NotUsed;
impl ModelUsage for NotUsed {
    fn in_use(&self, _: &Path) -> Option<String> {
        None
    }
}

/// Reports `path` in use and records what it was asked about.
struct UsedBy {
    path: PathBuf,
    asked: RefCell<Vec<PathBuf>>,
}
impl ModelUsage for UsedBy {
    fn in_use(&self, canonical: &Path) -> Option<String> {
        self.asked.borrow_mut().push(canonical.to_path_buf());
        (canonical == self.path).then(|| "the local model server is running on it".to_string())
    }
}

#[test]
fn success_removes_the_model_and_every_side_file_and_returns_the_bytes() {
    let d = tmp("ok");
    verified_model(&d, "m.gguf", 1000);
    std::fs::write(d.join("m.gguf.download.json"), r#"{"id":"hf:a/b/m.gguf","sizeBytes":1000}"#)
        .expect("marker");
    std::fs::write(d.join("m.gguf.part"), vec![1u8; 10]).expect("part");
    verified_model(&d, "other.gguf", 5);
    let status_len = std::fs::metadata(d.join("m.gguf.status.json")).expect("m").len();
    let marker_len = std::fs::metadata(d.join("m.gguf.download.json")).expect("m").len();

    let out = delete(&d, None, "m.gguf", &NotUsed).expect("deleted");
    assert_eq!(out.file, "m.gguf");
    assert_eq!(out.freed_bytes, 1000 + 10 + status_len + marker_len);
    for f in ["m.gguf", "m.gguf.part", "m.gguf.status.json", "m.gguf.download.json"] {
        assert!(!d.join(f).exists(), "{f} removed");
    }
    // Another model is untouched, and the deleted one is no longer listed.
    assert!(d.join("other.gguf").exists() && d.join("other.gguf.status.json").exists());
    let listed: Vec<String> = crate::model_catalog::read_local_models(&d)
        .expect("list")
        .into_iter()
        .map(|m| m.file)
        .collect();
    assert_eq!(listed, ["other.gguf"]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_list_id_form_is_accepted() {
    let d = tmp("id");
    verified_model(&d, "m.gguf", 3);
    assert!(delete(&d, None, "local:m.gguf", &NotUsed).expect("ok").freed_bytes > 0);
    assert!(!d.join("m.gguf").exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn path_traversal_and_non_model_names_are_refused() {
    let d = tmp("trav");
    let models = d.join("models");
    std::fs::create_dir_all(&models).expect("models");
    // A real verified-looking model OUTSIDE the models folder.
    verified_model(&d, "outside.gguf", 4);
    for bad in [
        "../outside.gguf",
        "..%2Foutside.gguf",
        "/etc/passwd",
        "sub/m.gguf",
        "..\\outside.gguf",
        "..",
        ".hidden.gguf",
        "",
        "m.gguf\0x",
        "m.gguf.status.json",
        "m.part",
        "local:../outside.gguf",
    ] {
        let e = delete(&models, None, bad, &NotUsed).expect_err(bad);
        assert!(
            matches!(e, DeleteRefusal::BadName(_) | DeleteRefusal::Unknown(_)),
            "{bad}: {e:?}"
        );
    }
    assert!(d.join("outside.gguf").exists(), "nothing outside the models folder is touched");
    let _ = std::fs::remove_dir_all(d);
}

#[cfg(unix)]
#[test]
fn a_symlink_is_refused_and_its_target_left_alone() {
    let d = tmp("link");
    let models = d.join("models");
    std::fs::create_dir_all(&models).expect("models");
    verified_model(&d, "target.gguf", 9);
    std::os::unix::fs::symlink(d.join("target.gguf"), models.join("link.gguf")).expect("link");
    std::fs::write(models.join("link.gguf.status.json"), r#"{"verified":true}"#).expect("status");
    assert_eq!(
        delete(&models, None, "link.gguf", &NotUsed).expect_err("link"),
        DeleteRefusal::Symlink
    );
    assert!(d.join("target.gguf").exists());
    assert!(std::fs::symlink_metadata(models.join("link.gguf")).is_ok());
    let _ = std::fs::remove_dir_all(d);
}

#[cfg(unix)]
#[test]
fn a_models_folder_that_is_a_link_still_only_deletes_inside_its_real_folder() {
    let d = tmp("dirlink");
    let real = d.join("real-models");
    std::fs::create_dir_all(&real).expect("real");
    verified_model(&real, "m.gguf", 6);
    std::os::unix::fs::symlink(&real, d.join("models")).expect("link");
    let out = delete(&d.join("models"), None, "m.gguf", &NotUsed).expect("deleted");
    assert!(out.freed_bytes > 6);
    assert!(!real.join("m.gguf").exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_unknown_or_unverified_file_is_refused() {
    let d = tmp("unknown");
    assert_eq!(
        delete(&d, None, "missing.gguf", &NotUsed).expect_err("missing"),
        DeleteRefusal::Unknown("missing.gguf".into())
    );
    // Present but never verified (a hand-placed or quarantined file): not a downloaded model.
    std::fs::write(d.join("raw.gguf"), b"GGUF").expect("raw");
    assert_eq!(
        delete(&d, None, "raw.gguf", &NotUsed).expect_err("unverified"),
        DeleteRefusal::Unknown("raw.gguf".into())
    );
    assert!(d.join("raw.gguf").exists());
    // A folder named like a model.
    std::fs::create_dir_all(d.join("dir.gguf")).expect("dir");
    std::fs::write(d.join("dir.gguf.status.json"), r#"{"verified":true}"#).expect("status");
    assert!(matches!(
        delete(&d, None, "dir.gguf", &NotUsed).expect_err("dir"),
        DeleteRefusal::Unknown(_)
    ));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_in_use_model_is_refused_with_the_reason() {
    let d = tmp("inuse");
    verified_model(&d, "busy.gguf", 8);
    verified_model(&d, "free.gguf", 8);
    let usage = UsedBy {
        path: d.join("busy.gguf"),
        asked: RefCell::new(Vec::new()),
    };
    let e = delete(&d, None, "busy.gguf", &usage).expect_err("in use");
    assert!(matches!(&e, DeleteRefusal::InUse(r) if r.contains("running")), "{e:?}");
    assert!(e.to_string().starts_with("in use: "));
    assert!(d.join("busy.gguf").exists() && d.join("busy.gguf.status.json").exists());
    // The usage probe is asked about the canonical file, and a different model still deletes.
    assert_eq!(usage.asked.borrow()[0], d.join("busy.gguf"));
    delete(&d, None, "free.gguf", &usage).expect("free deletes");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_model_shipped_in_the_bundle_is_refused() {
    let d = tmp("bundled");
    let models = d.join("models");
    let bundle = d.join("resources-models");
    std::fs::create_dir_all(&models).expect("models");
    std::fs::create_dir_all(&bundle).expect("bundle");
    verified_model(&models, "gemma.gguf", 5);
    std::fs::write(bundle.join("gemma.gguf"), b"x").expect("bundled copy");
    assert_eq!(
        delete(&models, Some(&bundle), "gemma.gguf", &NotUsed).expect_err("bundled"),
        DeleteRefusal::Bundled
    );
    assert!(models.join("gemma.gguf").exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_download_in_progress_is_refused_and_a_delete_blocks_a_download() {
    let d = tmp("dl");
    verified_model(&d, "m.gguf", 4);
    let part = d.join("m.gguf.part");
    {
        let _download = crate::model::DownloadGuard::acquire(&part).expect("download lock");
        assert_eq!(
            delete(&d, None, "m.gguf", &NotUsed).expect_err("downloading"),
            DeleteRefusal::Downloading
        );
        assert!(crate::model::download_in_flight(&part));
        let st = states(&d, None, &NotUsed).expect("states");
        assert_eq!(st.len(), 1);
        assert!(!st[0].deletable);
        assert!(st[0].reason.as_deref().unwrap_or_default().contains("download"));
    }
    assert!(!crate::model::download_in_flight(&part));
    assert!(d.join("m.gguf").exists());
    delete(&d, None, "m.gguf", &NotUsed).expect("after the download");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn states_list_each_verified_model_with_why_not() {
    let d = tmp("states");
    verified_model(&d, "a.gguf", 3);
    verified_model(&d, "b.gguf", 4);
    let usage = UsedBy {
        path: d.join("b.gguf"),
        asked: RefCell::new(Vec::new()),
    };
    let st = states(&d, None, &usage).expect("states");
    assert_eq!(
        st,
        vec![
            DeleteState {
                file: "a.gguf".into(),
                size_bytes: 3,
                deletable: true,
                reason: None
            },
            DeleteState {
                file: "b.gguf".into(),
                size_bytes: 4,
                deletable: false,
                reason: Some("in use: the local model server is running on it".into())
            },
        ]
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_chat_server_selection_falls_back_when_its_model_is_deleted() {
    let d = tmp("select");
    let default = d.join("default.gguf");
    let chosen = d.join("chosen.gguf");
    let serve = crate::serve::LlamaServerManager::new(
        PathBuf::from("/nonexistent/llama-server"),
        chosen.clone(),
        d.join("crash.jsonl"),
        0,
    );
    assert!(!serve.forget_model_if(&d.join("unrelated.gguf"), default.clone()));
    assert_eq!(serve.current_model_path(), chosen);
    assert!(serve.forget_model_if(&chosen, default.clone()));
    assert_eq!(serve.current_model_path(), default);
    // Deleting the default model itself: reported, and the path stays (it shows not downloaded).
    assert!(serve.forget_model_if(&default, default.clone()));
    assert_eq!(serve.current_model_path(), default);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn delete_is_member_only_never_an_agent_tool() {
    let acl = include_str!("../permissions/main-window.toml");
    let lib = include_str!("lib.rs");
    for cmd in ["model_delete", "model_delete_states"] {
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
        assert!(lib.contains(&format!("model_delete::{cmd}")), "{cmd} registered");
    }
    // Not in the popout ACL, the node MCP tool list, or the chat/Hermes tool lists.
    let popout = include_str!("../capabilities/popout.json");
    assert!(!popout.contains("model_delete"));
    for (name, src) in [
        ("node_mcp_protocol.rs", include_str!("node_mcp_protocol.rs")),
        ("node_mcp_live.rs", include_str!("node_mcp_live.rs")),
        ("hermes_headless.rs", include_str!("hermes_headless.rs")),
        ("harness.ts", include_str!("../../src/agent/harness.ts")),
    ] {
        assert!(!src.contains("model_delete"), "{name} must not expose model_delete");
    }
}
