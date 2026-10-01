// HUP-S3.1 — first-run knowledge-corpus import. Red-first.
//
// CI-safe: the importer process is a small shell script per test that prints
// the mem-corpus JSON-lines contract (the real `mem-mcp import-corpus` is a
// rocksdb build; it is exercised by the `#[ignore]` live test at the bottom,
// against the committed fixture corpus in tests/fixtures/knowledge-corpus).

use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

const FIXTURE_DIGEST: &str = "9dfc0e4e64edf264b92c0a86678d21e32efc86d787526c049997ab01003505fe";

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-s31-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn fixture_corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/knowledge-corpus")
}

/// A fake importer: prints `stdout` and exits with `code`. Records its argv.
#[cfg(unix)]
fn fake_importer(dir: &Path, stdout: &str, code: i32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("fake-mem-mcp.sh");
    let argv = dir.join("argv.txt");
    let body = format!(
        "#!/bin/sh\necho \"$@\" > '{}'\ncat <<'LINES'\n{}\nLINES\nexit {}\n",
        argv.display(),
        stdout.trim_end(),
        code
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn done_lines(digest: &str) -> String {
    format!(
        r#"{{"event":"verified","bundle_digest":"{digest}","tenants":4,"nodes":19,"edges":13}}
{{"event":"tenant_start","tenant":"citrate-docs","nodes":9,"edges":6}}
{{"event":"progress","tenant":"citrate-docs","done":9,"total":9}}
{{"event":"tenant_done","tenant":"citrate-docs"}}
{{"event":"tenant_skipped","tenant":"skills","reason":"already-imported"}}
{{"event":"done","bundle_digest":"{digest}","embed_model":"bge-base-en-v1.5","nodes_added":19,"nodes_merged":0,"edges_added":13,"tenants_imported":["citrate-docs","refs","methodology"],"tenants_skipped":["skills"]}}"#
    )
}

fn plan(dir: &Path, importer: PathBuf) -> ImportPlan {
    std::fs::create_dir_all(dir.join("memory/store.bge.memdag")).unwrap();
    ImportPlan {
        importer,
        store_path: dir.join("memory/store.bge.memdag"),
        corpus_dir: Some(fixture_corpus()),
        semantic: true,
        env: vec![("CITRATE_MEM_EMBED".into(), "bge".into())],
        first_line_timeout: std::time::Duration::from_secs(10),
    }
}

// ---------------------------------------------------------------- parsing

#[test]
fn parses_every_contract_line_and_ignores_noise() {
    let lines: Vec<_> = done_lines(FIXTURE_DIGEST).lines().map(parse_line).collect();
    assert!(matches!(
        lines[0],
        Some(ImportLine::Verified {
            tenants: 4,
            nodes: 19,
            ..
        })
    ));
    assert!(
        matches!(&lines[2], Some(ImportLine::Progress { done: 9, total: 9, tenant }) if tenant == "citrate-docs")
    );
    assert!(
        matches!(&lines[4], Some(ImportLine::TenantSkipped { reason, .. }) if reason == "already-imported")
    );
    assert!(matches!(
        &lines[5],
        Some(ImportLine::Done {
            nodes_added: 19,
            ..
        })
    ));
    assert_eq!(parse_line("mem-mcp: some stderr-ish text"), None);
    assert_eq!(parse_line(r#"{"event":"from-the-future","x":1}"#), None);
    let err = parse_line(r#"{"event":"error","stage":"verify","message":"bad hash"}"#);
    assert!(matches!(err, Some(ImportLine::Error { ref stage, .. }) if stage == "verify"));
}

#[test]
fn reads_the_bundle_digest_from_the_manifest() {
    assert_eq!(
        read_bundle_digest(&fixture_corpus()).unwrap(),
        FIXTURE_DIGEST
    );
    let dir = tmp_dir("digest");
    assert!(
        read_bundle_digest(&dir).is_err(),
        "no manifest is an honest error"
    );
    std::fs::write(dir.join("manifest.json"), r#"{"bundle_digest":"not-hex"}"#).unwrap();
    assert!(
        read_bundle_digest(&dir).is_err(),
        "a malformed digest is refused"
    );
}

// ---------------------------------------------------------------- gates

#[cfg(unix)]
#[test]
fn no_bundled_corpus_is_an_honest_skip() {
    let dir = tmp_dir("nobundle");
    let mut p = plan(&dir, fake_importer(&dir, "", 1));
    p.corpus_dir = None;
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "skipped");
    assert_eq!(r.skipped.as_deref(), Some("no-bundle"));
    assert!(!dir.join("argv.txt").exists(), "importer must not run");
}

#[cfg(unix)]
#[test]
fn lexical_only_store_is_not_imported() {
    // Importing with no BGE would lock a fresh store to lexical vectors forever.
    let dir = tmp_dir("lexical");
    let mut p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    p.semantic = false;
    let r = run_plan(&p, |_| {});
    assert_eq!(r.skipped.as_deref(), Some("not-semantic"));
    assert!(!dir.join("argv.txt").exists());
}

// ---------------------------------------------------------------- happy path + idempotency

#[cfg(unix)]
#[test]
fn imports_reports_progress_and_writes_the_marker() {
    let dir = tmp_dir("ok");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    let seen = Arc::new(StdMutex::new(Vec::new()));
    let s2 = seen.clone();
    let r = run_plan(&p, move |l| s2.lock().unwrap().push(l.clone()));
    assert_eq!(r.state, "imported", "{r:?}");
    assert_eq!(r.bundle_digest.as_deref(), Some(FIXTURE_DIGEST));
    assert_eq!(r.nodes_added, 19);
    assert_eq!(r.edges_added, 13);
    assert_eq!(r.embed_model.as_deref(), Some("bge-base-en-v1.5"));
    assert_eq!(r.tenants_imported, ["citrate-docs", "refs", "methodology"]);
    assert_eq!(
        seen.lock().unwrap().len(),
        6,
        "every contract line is forwarded"
    );
    let argv = std::fs::read_to_string(dir.join("argv.txt")).unwrap();
    assert!(argv.starts_with("import-corpus "), "{argv}");
    assert!(argv.contains("store.bge.memdag") && argv.contains("knowledge-corpus"));
    assert_eq!(
        std::fs::read_to_string(marker_path(&p.store_path))
            .unwrap()
            .trim(),
        FIXTURE_DIGEST
    );
}

#[cfg(unix)]
#[test]
fn a_recorded_digest_skips_without_running_the_importer() {
    let dir = tmp_dir("again");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    std::fs::write(marker_path(&p.store_path), FIXTURE_DIGEST).unwrap();
    let r = run_plan(&p, |_| {});
    assert_eq!(r.skipped.as_deref(), Some("already-imported"));
    assert!(!dir.join("argv.txt").exists());
}

#[cfg(unix)]
#[test]
fn a_new_corpus_digest_imports_again() {
    let dir = tmp_dir("newer");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    std::fs::write(marker_path(&p.store_path), "0".repeat(64)).unwrap();
    assert_eq!(run_plan(&p, |_| {}).state, "imported");
}

#[cfg(unix)]
#[test]
fn a_marker_without_a_store_is_not_trusted() {
    // A wiped store with a lingering marker must import again.
    let dir = tmp_dir("wiped");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    std::fs::write(marker_path(&p.store_path), FIXTURE_DIGEST).unwrap();
    std::fs::remove_dir_all(&p.store_path).unwrap();
    assert_eq!(run_plan(&p, |_| {}).state, "imported");
}

// ---------------------------------------------------------------- honest failures

#[cfg(unix)]
#[test]
fn an_importer_error_line_fails_with_its_message_and_no_marker() {
    let dir = tmp_dir("err");
    let lines = r#"{"event":"error","stage":"verify","message":"corpus verification failed: tenants/refs.syncbundle.json does not match its manifest hash"}"#;
    let p = plan(&dir, fake_importer(&dir, lines, 1));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "failed");
    assert!(
        r.error
            .as_deref()
            .unwrap()
            .contains("does not match its manifest hash"),
        "{r:?}"
    );
    assert!(!marker_path(&p.store_path).exists());
}

#[cfg(unix)]
#[test]
fn exit_zero_without_a_done_line_is_a_failure() {
    let dir = tmp_dir("nodone");
    let lines = r#"{"event":"verified","bundle_digest":"9dfc0e4e64edf264b92c0a86678d21e32efc86d787526c049997ab01003505fe","tenants":4,"nodes":19,"edges":13}"#;
    let p = plan(&dir, fake_importer(&dir, lines, 0));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "failed");
    assert!(!marker_path(&p.store_path).exists());
}

#[cfg(unix)]
#[test]
fn a_done_line_for_another_corpus_is_a_failure() {
    let dir = tmp_dir("mismatch");
    let p = plan(&dir, fake_importer(&dir, &done_lines(&"a".repeat(64)), 0));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "failed");
    assert!(r.error.as_deref().unwrap().contains("digest"));
    assert!(!marker_path(&p.store_path).exists());
}

#[cfg(unix)]
#[test]
fn a_missing_importer_binary_is_a_failure_not_a_skip() {
    let dir = tmp_dir("nobin");
    let p = plan(&dir, dir.join("does-not-exist"));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "failed");
}

#[cfg(unix)]
#[test]
fn a_silent_importer_is_killed_and_reported() {
    // An older bundled mem-mcp without `import-corpus` would treat the arguments
    // as a store path and serve forever. No first line within the bound → fail.
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("silent");
    let bin = dir.join("old-mem-mcp.sh");
    std::fs::write(&bin, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut p = plan(&dir, bin);
    p.first_line_timeout = std::time::Duration::from_millis(500);
    let t0 = std::time::Instant::now();
    let r = run_plan(&p, |_| {});
    assert!(
        t0.elapsed() < std::time::Duration::from_secs(10),
        "must not wait for the hung child"
    );
    assert_eq!(r.state, "failed");
    assert!(
        r.error.as_deref().unwrap().contains("did not respond"),
        "{r:?}"
    );
    assert!(!marker_path(&p.store_path).exists());
}

#[cfg(unix)]
#[test]
fn concurrent_imports_do_not_overlap() {
    let dir = tmp_dir("overlap");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    let _guard = ImportGuard::acquire(&p.store_path).unwrap();
    let r = run_plan(&p, |_| {});
    assert_eq!(r.skipped.as_deref(), Some("in-progress"));
}

// ---------------------------------------------------------------- daemon stop / restart

#[derive(Default)]
struct MemKeyring(StdMutex<std::collections::HashMap<String, Vec<u8>>>);
impl crate::custody::Keyring for MemKeyring {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, crate::custody::CustodyError> {
        Ok(self.0.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), crate::custody::CustodyError> {
        self.0.lock().unwrap().insert(a.into(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> std::result::Result<(), crate::custody::CustodyError> {
        self.0.lock().unwrap().remove(a);
        Ok(())
    }
}

struct NoTransport;
impl crate::memory::MemoryTransport for NoTransport {
    fn call_tool(
        &self,
        _t: &str,
        _a: serde_json::Value,
    ) -> std::result::Result<String, crate::memory::MemoryError> {
        Err(crate::memory::MemoryError::NotRunning)
    }
}

/// One script for both roles: as the daemon it records its pid and sleeps; as
/// the importer it fails if that daemon is still alive (the store lock), else
/// prints a successful run.
#[cfg(unix)]
fn daemon_and_importer(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("mem-mcp-both.sh");
    let pid = dir.join("daemon.pid");
    let body = format!(
        r#"#!/bin/sh
if [ "$1" = "import-corpus" ]; then
  if [ -f '{pid}' ] && kill -0 "$(cat '{pid}')" 2>/dev/null; then
    echo '{{"event":"error","stage":"open","message":"store locked by a live daemon"}}'
    exit 1
  fi
  mkdir -p "$2"
  cat <<'LINES'
{lines}
LINES
  exit 0
fi
echo $$ > '{pid}'
exec sleep 30
"#,
        pid = pid.display(),
        lines = done_lines(FIXTURE_DIGEST)
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[cfg(unix)]
#[test]
fn a_running_daemon_is_stopped_for_the_import_and_restarted() {
    let dir = tmp_dir("restart");
    let bin = daemon_and_importer(&dir);
    let model = dir.join("bge");
    std::fs::create_dir_all(&model).unwrap();
    let mgr = crate::memory::MemoryManager::new(
        Box::new(MemKeyring::default()),
        bin,
        dir.join("memory/store.bge.memdag"),
        dir.join("memory/memdag.sock"),
        dir.join("memory/crash.jsonl"),
        Box::new(NoTransport),
    )
    .with_model_dir(Some(model));
    mgr.start().unwrap();
    for _ in 0..50 {
        if dir.join("daemon.pid").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(mgr.is_running_now());
    let r = import_with_manager(&mgr, Some(fixture_corpus()), |_| {});
    assert_eq!(
        r.state, "imported",
        "the daemon must be stopped before the import: {r:?}"
    );
    assert!(
        mgr.is_running_now(),
        "the daemon is restarted after the import"
    );
    // Second call: marker matches, so the daemon is not stopped at all.
    let r2 = import_with_manager(&mgr, Some(fixture_corpus()), |_| {});
    assert_eq!(r2.skipped.as_deref(), Some("already-imported"));
    assert!(mgr.is_running_now());
    mgr.stop();
}

#[cfg(unix)]
#[test]
fn a_stopped_daemon_stays_stopped() {
    let dir = tmp_dir("stopped");
    let bin = daemon_and_importer(&dir);
    let model = dir.join("bge");
    std::fs::create_dir_all(&model).unwrap();
    let mgr = crate::memory::MemoryManager::new(
        Box::new(MemKeyring::default()),
        bin,
        dir.join("memory/store.bge.memdag"),
        dir.join("memory/memdag.sock"),
        dir.join("memory/crash.jsonl"),
        Box::new(NoTransport),
    )
    .with_model_dir(Some(model));
    let r = import_with_manager(&mgr, Some(fixture_corpus()), |_| {});
    assert_eq!(r.state, "imported", "{r:?}");
    assert!(!mgr.is_running_now());
}

// ---------------------------------------------------------------- tripwire

#[test]
fn import_command_is_async_off_main_and_in_the_main_window_acl() {
    let src = include_str!("knowledge_import.rs");
    assert!(src.contains("pub async fn memory_import_knowledge"));
    assert!(src.contains("crate::blocking::off_main"));
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"memory_import_knowledge\""));
    let lib = include_str!("lib.rs");
    assert!(lib.contains("knowledge_import::memory_import_knowledge"));
    let popout = include_str!("../capabilities/popout.json");
    assert!(
        !popout.contains("memory_import_knowledge"),
        "pop-outs stay least-privilege"
    );
}

// ---------------------------------------------------------------- live (real mem-mcp)

/// Live proof against the real daemon binary and the committed fixture corpus:
/// `CITRATE_MEM_MCP_BIN=<path to mem-mcp built with --features rocksdb> \
///  cargo test --lib knowledge_import::tests::live_ -- --ignored`
#[test]
#[ignore]
fn live_real_mem_mcp_imports_the_fixture_corpus_once() {
    let Ok(bin) = std::env::var("CITRATE_MEM_MCP_BIN") else {
        panic!("set CITRATE_MEM_MCP_BIN to a real mem-mcp binary");
    };
    let dir = tmp_dir("live");
    let mut p = plan(&dir, PathBuf::from(bin));
    std::fs::remove_dir_all(&p.store_path).unwrap();
    p.env.clear(); // no BGE in CI: the real binary falls back to the hashing embedder
    let mut lines = Vec::new();
    let r = run_plan(&p, |l| lines.push(l.clone()));
    assert_eq!(r.state, "imported", "{r:?}");
    assert_eq!(r.nodes_added, 19);
    assert!(lines
        .iter()
        .any(|l| matches!(l, ImportLine::Progress { .. })));
    // Second run: the marker short-circuits; and with the marker removed the
    // store itself still knows (tenant hashes in store meta) and adds nothing.
    assert_eq!(
        run_plan(&p, |_| {}).skipped.as_deref(),
        Some("already-imported")
    );
    std::fs::remove_file(marker_path(&p.store_path)).unwrap();
    let again = run_plan(&p, |_| {});
    assert_eq!(again.state, "imported");
    assert_eq!(again.nodes_added, 0);
    assert_eq!(again.tenants_imported.len(), 0);
}
