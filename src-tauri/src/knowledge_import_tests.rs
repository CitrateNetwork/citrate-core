// HUP-S3.1 — first-run knowledge-corpus import. Red-first.
//
// CI-safe: the importer process is a small shell script per test that prints
// the mem-corpus JSON-lines contract (the real `mem-mcp import-corpus` is a
// rocksdb build; it is exercised by the `#[ignore]` live test at the bottom,
// against the committed fixture corpus in tests/fixtures/knowledge-corpus).

use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

const FIXTURE_DIGEST: &str = "ab4e8407b2bcf76090b8d2952ae0f81d48b6e6ab8f35d6d874bce036607283b8";

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
        registry: Arc::new(ImportRegistry::default()),
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

// ---------------------------------------------------------------- precomputed vectors

#[cfg(unix)]
#[test]
fn the_report_carries_how_many_nodes_were_embedded_or_took_bundled_vectors() {
    let dir = tmp_dir("vectors");
    let lines = done_lines(FIXTURE_DIGEST).replace(
        r#""tenants_skipped":["skills"]}"#,
        r#""tenants_skipped":["skills"],"nodes_embedded":2,"vectors_reused":17}"#,
    );
    let p = plan(&dir, fake_importer(&dir, &lines, 0));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "imported", "{r:?}");
    assert_eq!((r.nodes_embedded, r.vectors_reused), (2, 17));
}

#[test]
fn an_older_importer_without_the_counters_still_parses() {
    let line = done_lines(FIXTURE_DIGEST).lines().last().unwrap().to_string();
    match parse_line(&line) {
        Some(ImportLine::Done {
            nodes_embedded,
            vectors_reused,
            ..
        }) => assert_eq!((nodes_embedded, vectors_reused), (0, 0)),
        other => panic!("{other:?}"),
    }
}

// ---------------------------------------------------------------- staging

#[test]
fn only_a_directory_with_a_manifest_counts_as_a_staged_corpus() {
    // The app ships `knowledge-corpus/README.md` in every build so the bundle
    // resource glob always matches; a release stages the corpus beside it.
    let dir = tmp_dir("staged");
    assert_eq!(staged_corpus_dir(&dir.join("absent")), None);
    std::fs::write(dir.join("README.md"), "staging notes").unwrap();
    assert_eq!(staged_corpus_dir(&dir), None, "README only = no bundle");
    std::fs::write(dir.join("manifest.json"), "{}").unwrap();
    assert_eq!(staged_corpus_dir(&dir), Some(dir.clone()));
    assert_eq!(staged_corpus_dir(&fixture_corpus()), Some(fixture_corpus()));
    // A manifest that is a directory (or anything but a file) is not a corpus.
    let odd = tmp_dir("staged-odd");
    std::fs::create_dir_all(odd.join("manifest.json")).unwrap();
    assert_eq!(staged_corpus_dir(&odd), None);
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
fn a_done_line_with_a_failing_exit_is_still_a_failure() {
    // Review hardening: the exit status is checked even when a done line arrived.
    let dir = tmp_dir("doneexit");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 3));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.state, "failed", "{r:?}");
    assert!(
        r.error
            .as_deref()
            .unwrap()
            .contains("exited unsuccessfully"),
        "{r:?}"
    );
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
fn an_import_already_in_flight_never_stops_the_running_daemon() {
    // Review hardening: a second caller that loses the race must not stop (or later
    // restart) the daemon around an import it is not running.
    let dir = tmp_dir("inflight");
    let bin = daemon_and_importer(&dir);
    let model = dir.join("bge");
    std::fs::create_dir_all(&model).unwrap();
    let store = dir.join("memory/store.bge.memdag");
    let mgr = crate::memory::MemoryManager::new(
        Box::new(MemKeyring::default()),
        bin,
        store.clone(),
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
    let pid_before = std::fs::read_to_string(dir.join("daemon.pid")).unwrap();
    let held = ImportGuard::acquire(&store).unwrap();
    let r = import_with_manager(&mgr, Some(fixture_corpus()), |_| {});
    drop(held);
    assert_eq!(r.skipped.as_deref(), Some("in-progress"), "{r:?}");
    // A respawned daemon writes its own pid shortly after start: watch for it.
    for _ in 0..20 {
        let pid_after = std::fs::read_to_string(dir.join("daemon.pid")).unwrap();
        assert_eq!(
            pid_before, pid_after,
            "the daemon was stopped and respawned"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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

// ---------------------------------------------------------------- lifecycle (v0.5.0 C3 finding 1)

/// Whether `pid` names a live process (signal 0 sends nothing).
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) only checks existence and permission.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Kills any pid it was given that is still alive (cleanup when an assertion fails).
#[cfg(unix)]
struct Reaper(Vec<u32>);

#[cfg(unix)]
impl Drop for Reaper {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if alive(pid) {
                // SAFETY: a pid this test's own importer held; SIGKILL ends a leftover.
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }
}

/// Wait until `cond` holds (bounded); panics with `what` otherwise.
fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !cond() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// A fake importer with the real importer's resume rule: tenants run in order, a
/// tenant is recorded in the store (a file) only after it finished, and a recorded
/// tenant is skipped. While `<dir>/block` exists it stops inside `refs` (after
/// one progress line) and waits to be killed; `ignore_term` makes it ignore SIGTERM.
#[cfg(unix)]
fn resumable_importer(dir: &Path, ignore_term: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("resumable-mem-mcp.sh");
    let trap = if ignore_term { "trap '' TERM\n" } else { "" };
    let block = if ignore_term {
        "while :; do :; done"
    } else {
        "exec sleep 30"
    };
    let body = format!(
        r#"#!/bin/sh
{trap}store="$2"
echo $$ > '{pid}'
echo "run" >> '{runs}'
echo '{{"event":"verified","bundle_digest":"{d}","tenants":4,"nodes":19,"edges":13}}'
imported=""
skipped=""
for t in citrate-docs methodology refs skills; do
  if [ -f "$store/tenant-$t" ]; then
    echo "{{\"event\":\"tenant_skipped\",\"tenant\":\"$t\",\"reason\":\"already-imported\"}}"
    skipped="$skipped,\"$t\""
    continue
  fi
  echo "{{\"event\":\"tenant_start\",\"tenant\":\"$t\",\"nodes\":4,\"edges\":3}}"
  echo "start $t" >> '{runs}'
  echo "{{\"event\":\"progress\",\"tenant\":\"$t\",\"done\":2,\"total\":4}}"
  if [ "$t" = "refs" ] && [ -f '{blockf}' ]; then
    {block}
  fi
  touch "$store/tenant-$t"
  echo "{{\"event\":\"tenant_done\",\"tenant\":\"$t\"}}"
  imported="$imported,\"$t\""
done
echo "{{\"event\":\"done\",\"bundle_digest\":\"{d}\",\"embed_model\":\"bge-base-en-v1.5\",\"nodes_added\":8,\"nodes_merged\":0,\"edges_added\":6,\"tenants_imported\":[${{imported#,}}],\"tenants_skipped\":[${{skipped#,}}]}}"
exit 0
"#,
        pid = dir.join("importer.pid").display(),
        runs = dir.join("runs.log").display(),
        blockf = dir.join("block").display(),
        d = FIXTURE_DIGEST,
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Start `run_plan` on a thread and wait until its importer is blocked inside `refs`.
#[cfg(unix)]
fn start_blocked_import(
    dir: &Path,
    p: &ImportPlan,
) -> (std::thread::JoinHandle<KnowledgeImportReport>, u32) {
    std::fs::write(dir.join("block"), b"").unwrap();
    let plan2 = p.clone();
    let handle = std::thread::spawn(move || run_plan(&plan2, |_| {}));
    wait_for("the importer to reach refs", || {
        read_progress(&p.store_path, FIXTURE_DIGEST)
            .is_some_and(|pr| pr.tenant.as_deref() == Some("refs") && pr.done == 2)
    });
    let pid = read_pid(&dir.join("importer.pid")).expect("importer pid");
    assert!(p.registry.holds_store(&p.store_path));
    (handle, pid)
}

#[cfg(unix)]
#[test]
fn app_quit_stops_a_running_importer_and_reaps_it() {
    let dir = tmp_dir("quit");
    let p = plan(&dir, resumable_importer(&dir, false));
    let (handle, pid) = start_blocked_import(&dir, &p);
    let _reaper = Reaper(vec![pid]);
    assert!(alive(pid));

    let t0 = std::time::Instant::now();
    assert_eq!(p.registry.shutdown(std::time::Duration::from_secs(5)), 1);
    assert!(!alive(pid), "the importer is gone (and reaped) when shutdown returns");
    assert!(t0.elapsed() < std::time::Duration::from_secs(5), "SIGTERM was enough");

    let r = handle.join().unwrap();
    assert_eq!(r.state, "failed");
    assert_eq!(r.error.as_deref(), Some(INTERRUPTED_ERROR));
    assert!(!marker_path(&p.store_path).exists(), "no completion marker");
    assert!(!p.registry.holds_store(&p.store_path), "released after the reap");
    let pr = read_progress(&p.store_path, FIXTURE_DIGEST).expect("progress kept");
    assert_eq!(pr.state, "interrupted");
    assert_eq!(
        pr.tenants_done.iter().map(String::as_str).collect::<Vec<_>>(),
        ["citrate-docs", "methodology"]
    );
    assert_eq!(pr.tenant.as_deref(), Some("refs"));
    // A second shutdown (ExitRequested then Exit) is a no-op.
    assert_eq!(p.registry.shutdown(std::time::Duration::from_secs(5)), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn an_importer_that_ignores_sigterm_is_killed_after_the_grace() {
    let dir = tmp_dir("quitkill");
    let p = plan(&dir, resumable_importer(&dir, true));
    let (handle, pid) = start_blocked_import(&dir, &p);
    let _reaper = Reaper(vec![pid]);
    let t0 = std::time::Instant::now();
    assert_eq!(p.registry.shutdown(std::time::Duration::from_millis(300)), 1);
    let took = t0.elapsed();
    assert!(!alive(pid), "SIGKILL after the grace");
    assert!(took >= std::time::Duration::from_millis(300), "{took:?}");
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
    assert_eq!(handle.join().unwrap().error.as_deref(), Some(INTERRUPTED_ERROR));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn an_interrupted_import_continues_on_the_next_launch_without_redoing_finished_tenants() {
    let dir = tmp_dir("resume");
    let importer = resumable_importer(&dir, false);
    let p = plan(&dir, importer.clone());
    let (handle, pid) = start_blocked_import(&dir, &p);
    let _reaper = Reaper(vec![pid]);
    p.registry.shutdown(std::time::Duration::from_secs(5));
    assert_eq!(handle.join().unwrap().error.as_deref(), Some(INTERRUPTED_ERROR));

    // Next launch: a fresh registry, nothing blocks.
    std::fs::remove_file(dir.join("block")).unwrap();
    std::fs::remove_file(dir.join("runs.log")).unwrap();
    let next = plan(&dir, importer);
    let mut lines = Vec::new();
    let r = run_plan(&next, |l| lines.push(l.clone()));
    assert_eq!(r.state, "imported", "{r:?}");
    assert_eq!(r.tenants_skipped, ["citrate-docs", "methodology"]);
    assert_eq!(r.tenants_imported, ["refs", "skills"]);
    let runs = std::fs::read_to_string(dir.join("runs.log")).unwrap();
    assert_eq!(runs, "run\nstart refs\nstart skills\n", "finished tenants were not redone");
    assert!(marker_path(&next.store_path).exists());
    assert!(
        !progress_path(&next.store_path).exists(),
        "the progress record is removed once the marker is written"
    );
    assert!(!next.registry.holds_store(&next.store_path));
    // And the launch after that skips without running the importer.
    assert_eq!(
        run_plan(&next, |_| {}).skipped.as_deref(),
        Some("already-imported")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn no_importer_starts_once_app_quit_began() {
    let dir = tmp_dir("closed");
    let p = plan(&dir, fake_importer(&dir, &done_lines(FIXTURE_DIGEST), 0));
    p.registry.shutdown(std::time::Duration::from_secs(1));
    let r = run_plan(&p, |_| {});
    assert_eq!(r.error.as_deref(), Some(INTERRUPTED_ERROR));
    assert!(!dir.join("argv.txt").exists(), "the importer was never spawned");
    let _ = std::fs::remove_dir_all(&dir);
}

/// As [`daemon_and_importer`], but the importer prints `verified` and then waits to be killed.
#[cfg(unix)]
fn daemon_and_slow_importer(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("mem-mcp-slow.sh");
    let body = format!(
        r#"#!/bin/sh
if [ "$1" = "import-corpus" ]; then
  echo $$ > '{ipid}'
  echo '{{"event":"verified","bundle_digest":"{d}","tenants":4,"nodes":19,"edges":13}}'
  exec sleep 30
fi
echo $$ > '{dpid}'
exec sleep 30
"#,
        ipid = dir.join("importer.pid").display(),
        dpid = dir.join("daemon.pid").display(),
        d = FIXTURE_DIGEST,
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[cfg(unix)]
#[test]
fn app_quit_during_an_import_does_not_restart_the_memory_daemon() {
    let dir = tmp_dir("norestart");
    let bin = daemon_and_slow_importer(&dir);
    let model = dir.join("bge");
    std::fs::create_dir_all(&model).unwrap();
    let store = dir.join("memory/store.bge.memdag");
    let mgr = Arc::new(
        crate::memory::MemoryManager::new(
            Box::new(MemKeyring::default()),
            bin.clone(),
            store.clone(),
            dir.join("memory/memdag.sock"),
            dir.join("memory/crash.jsonl"),
            Box::new(NoTransport),
        )
        .with_model_dir(Some(model)),
    );
    mgr.start().unwrap();
    wait_for("the daemon", || dir.join("daemon.pid").exists());
    let daemon_pid = read_pid(&dir.join("daemon.pid")).unwrap();
    let mut p = plan(&dir, bin);
    p.env = mgr.embedder_env();
    let (m2, p2) = (mgr.clone(), p.clone());
    let handle = std::thread::spawn(move || import_plan_with_manager(&m2, &p2, |_| {}));
    wait_for("the importer", || p.registry.holds_store(&store));
    wait_for("the importer pid", || read_pid(&dir.join("importer.pid")).is_some());
    let importer_pid = read_pid(&dir.join("importer.pid")).unwrap();
    let _reaper = Reaper(vec![daemon_pid, importer_pid]);
    assert!(!mgr.is_running_now(), "the daemon was stopped for the import");

    p.registry.shutdown(std::time::Duration::from_secs(5));
    let r = handle.join().unwrap();
    assert_eq!(r.error.as_deref(), Some(INTERRUPTED_ERROR), "{r:?}");
    assert!(!alive(importer_pid));
    assert!(
        !mgr.is_running_now(),
        "the daemon is not restarted once app quit began"
    );
    mgr.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn the_memory_daemon_refuses_to_start_while_an_importer_holds_the_store() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp_dir("refuse");
    // Finishes on its own after a short wait (the global registry is never shut down in tests).
    let bin = dir.join("slow-done.sh");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"import-corpus\" ]; then\n  echo $$ > '{}'\n  sleep 2\n  cat <<'LINES'\n{}\nLINES\n  exit 0\nfi\nexec sleep 30\n",
            dir.join("importer.pid").display(),
            done_lines(FIXTURE_DIGEST)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let model = dir.join("bge");
    std::fs::create_dir_all(&model).unwrap();
    let store = dir.join("memory/store.bge.memdag");
    let mgr = crate::memory::MemoryManager::new(
        Box::new(MemKeyring::default()),
        bin.clone(),
        store.clone(),
        dir.join("memory/memdag.sock"),
        dir.join("memory/crash.jsonl"),
        Box::new(NoTransport),
    )
    .with_model_dir(Some(model));
    let mut p = plan(&dir, bin);
    p.registry = ImportRegistry::global();
    let p2 = p.clone();
    let handle = std::thread::spawn(move || run_plan(&p2, |_| {}));
    wait_for("the importer", || ImportRegistry::global().holds_store(&store));
    wait_for("the importer pid", || read_pid(&dir.join("importer.pid")).is_some());
    let _reaper = Reaper(read_pid(&dir.join("importer.pid")).into_iter().collect());
    let err = mgr.start().expect_err("the store is held by the importer");
    assert!(err.to_string().contains("knowledge import"), "{err}");
    assert!(!mgr.is_running_now());
    assert_eq!(handle.join().unwrap().state, "imported");
    assert!(!ImportRegistry::global().holds_store(&store));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_quit_hook_stops_the_importer_before_any_sidecar() {
    let lib = include_str!("lib.rs");
    let hook = lib
        .split("pub(crate) fn shutdown_all_sidecars")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("shutdown_all_sidecars exists");
    let at = hook
        .find("knowledge_import::shutdown();")
        .expect("the quit hook stops the importer");
    let first_stop = hook.find(".stop();").expect("sidecar stops");
    assert!(at < first_stop, "the importer stops first: {hook}");
    let src = include_str!("knowledge_import.rs");
    assert!(
        src.contains("registry.adopt(&plan.store_path, child)"),
        "the importer child is owned by the registry"
    );
    assert!(src.contains("crate::supervisor::DEFAULT_STOP_GRACE"));
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

/// Live proof of v0.5.0 C3 finding 1 against the real importer and a real corpus: app quit stops
/// `mem-mcp import-corpus` mid-run (after at least one tenant finished), the store is left
/// consistent, and the next run continues with the tenants that did not finish.
/// `CITRATE_MEM_MCP_BIN=<mem-mcp, rocksdb> CITRATE_KNOWLEDGE_CORPUS_DIR=<staged corpus> \
///  [CITRATE_BGE_MODEL_DIR=<bge dir>] cargo test --lib knowledge_import::tests::live_interrupted -- --ignored`
#[test]
#[ignore]
fn live_interrupted_real_import_resumes_and_leaves_the_store_consistent() {
    let (Ok(bin), Ok(corpus)) = (
        std::env::var("CITRATE_MEM_MCP_BIN"),
        std::env::var(CORPUS_DIR_ENV),
    ) else {
        panic!("set CITRATE_MEM_MCP_BIN and {CORPUS_DIR_ENV}");
    };
    let dir = tmp_dir("live-resume");
    let mut p = plan(&dir, PathBuf::from(bin));
    std::fs::remove_dir_all(&p.store_path).unwrap();
    p.corpus_dir = Some(PathBuf::from(corpus));
    p.first_line_timeout = std::time::Duration::from_secs(600);
    p.env = match std::env::var("CITRATE_BGE_MODEL_DIR") {
        Ok(m) => vec![
            ("CITRATE_BGE_MODEL_DIR".into(), m),
            ("CITRATE_MEM_EMBED".into(), "bge".into()),
        ],
        Err(_) => vec![],
    };
    let digest = read_bundle_digest(p.corpus_dir.as_ref().unwrap()).unwrap();

    // Run 1: stop it once a tenant finished and the next one is in flight.
    let t0 = std::time::Instant::now();
    let p1 = p.clone();
    let handle = std::thread::spawn(move || run_plan(&p1, |_| {}));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    loop {
        let pr = read_progress(&p.store_path, &digest);
        if pr.as_ref().is_some_and(|pr| !pr.tenants_done.is_empty() && pr.done > 0) {
            break;
        }
        assert!(!handle.is_finished(), "the import finished before it could be interrupted");
        assert!(std::time::Instant::now() < deadline, "no tenant finished within an hour");
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let stopped = p.registry.shutdown(crate::supervisor::DEFAULT_STOP_GRACE);
    let r1 = handle.join().unwrap();
    let pr = read_progress(&p.store_path, &digest).unwrap();
    eprintln!(
        "run 1: stopped {stopped} after {:?}; {:?}; progress {:?}",
        t0.elapsed(),
        r1.error,
        pr
    );
    assert_eq!(stopped, 1);
    assert_eq!(r1.error.as_deref(), Some(INTERRUPTED_ERROR));
    assert_eq!(pr.state, "interrupted");
    assert!(!p.registry.holds_store(&p.store_path));

    // Run 2 (next launch): the store opens (the lock was released), finished tenants are skipped.
    let t1 = std::time::Instant::now();
    let mut p2 = p.clone();
    p2.registry = Arc::new(ImportRegistry::default());
    let r2 = run_plan(&p2, |_| {});
    eprintln!("run 2: {:?} after {:?}", r2, t1.elapsed());
    assert_eq!(r2.state, "imported", "{r2:?}");
    for t in &pr.tenants_done {
        assert!(r2.tenants_skipped.contains(t), "{t} was redone: {r2:?}");
        assert!(!r2.tenants_imported.contains(t), "{t} was redone: {r2:?}");
    }
    assert!(!progress_path(&p.store_path).exists());

    // Run 3, marker removed: the store already holds everything (consistent, nothing added).
    std::fs::remove_file(marker_path(&p.store_path)).unwrap();
    let r3 = run_plan(&p2, |_| {});
    eprintln!("run 3: {r3:?}");
    assert_eq!(r3.state, "imported", "{r3:?}");
    assert_eq!(r3.nodes_added, 0);
    assert!(r3.tenants_imported.is_empty(), "{r3:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
