// SCL-S0.6 / S8.5a — chain database lock and node port holders.
//
// The holder fixtures are separate OS processes: a POSIX record lock never conflicts with a
// lock held by the same process, so an in-process holder would prove nothing. Each fixture is
// this test binary re-run on the ignored `fixture_child_entry` test, which takes the resource
// and then waits for its stdin to close. Its lock is taken the way RocksDB takes it
// (`PosixFileSystem::LockFile`: open O_RDWR|O_CREAT, `fcntl(F_SETLK)` write lock over the whole
// file; on Windows an open with no sharing), not through `node_holder`, so the tests check the
// probe against the RocksDB behaviour and not against itself.
//
// Owned-process guard rules: every fixture is killed and reaped in `Drop`, including when the
// test fails, and exits on its own when the test process dies (its stdin closes).

use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

const FIXTURE_MODE_ENV: &str = "CITRATE_TEST_HOLDER_FIXTURE";
const FIXTURE_ARG_ENV: &str = "CITRATE_TEST_HOLDER_FIXTURE_ARG";
const FIXTURE_READY: &str = "CITRATE-HOLDER-FIXTURE-READY";
const FIXTURE_ENTRY: &str = "node_holder::tests::fixture_child_entry";

/// An owned fixture process. Killed and reaped on drop.
pub(crate) struct HolderFixture {
    child: Child,
    /// The fixture's pid.
    pub pid: u32,
    /// The port it listens on (listen mode).
    pub port: u16,
}

impl Drop for HolderFixture {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HolderFixture {
    /// Stop the fixture now (kill and reap), releasing what it held.
    pub fn stop(mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// The fixture's executable (this test binary), canonical.
    pub fn exe() -> PathBuf {
        let exe = std::env::current_exe().expect("test binary path");
        std::fs::canonicalize(&exe).unwrap_or(exe)
    }
}

fn spawn_fixture(mode: &str, arg: &str) -> HolderFixture {
    let mut child = Command::new(std::env::current_exe().expect("test binary path"))
        .args([
            "--exact",
            FIXTURE_ENTRY,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(FIXTURE_MODE_ENV, mode)
        .env(FIXTURE_ARG_ENV, arg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn holder fixture");
    let stdout = child.stdout.take().expect("fixture stdout");
    let pid = child.id();
    // Guard first, so a failed read below still kills and reaps the child.
    let mut fx = HolderFixture { child, pid, port: 0 };
    // Read on a thread so a fixture that never gets ready fails the test instead of hanging
    // it. libtest prints `test <name> ... ` before the marker, so match anywhere in the line.
    let (tx, rx) = std::sync::mpsc::channel::<Option<u16>>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(at) = line.find(FIXTURE_READY) {
                let rest = &line[at + FIXTURE_READY.len()..];
                let port = rest.split_whitespace().next().and_then(|p| p.parse().ok());
                let _ = tx.send(port);
                return;
            }
        }
        let _ = tx.send(None);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(Some(port)) => {
            fx.port = port;
            fx
        }
        Ok(None) => panic!("holder fixture ({mode}) exited before it was ready"),
        Err(_) => panic!("holder fixture ({mode}) not ready within 30s"),
    }
}

/// Hold `data_dir/LOCK` the way RocksDB does, from another process.
pub(crate) fn hold_chain_db_lock(data_dir: &Path) -> HolderFixture {
    std::fs::create_dir_all(data_dir).expect("create data dir");
    spawn_fixture(
        "lock",
        &data_dir.join(CHAIN_DB_LOCK_FILE).to_string_lossy(),
    )
}

/// Listen on a free loopback TCP port from another process.
pub(crate) fn listen_on_loopback() -> HolderFixture {
    spawn_fixture("listen", "")
}

/// The fixture process body. Not a test on its own: without the fixture env it does nothing.
#[test]
#[ignore = "fixture child process for the holder tests; does nothing when run directly"]
fn fixture_child_entry() {
    let Ok(mode) = std::env::var(FIXTURE_MODE_ENV) else {
        return;
    };
    let arg = std::env::var(FIXTURE_ARG_ENV).unwrap_or_default();
    // Keep whatever is held alive until stdin closes.
    let _held: Box<dyn std::any::Any> = match mode.as_str() {
        "lock" => Box::new(rocksdb_style_lock(Path::new(&arg))),
        "listen" => {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind fixture listener");
            let port = l.local_addr().expect("listener addr").port();
            println!("{FIXTURE_READY} {port}");
            Box::new(l)
        }
        other => panic!("unknown fixture mode {other}"),
    };
    if mode == "lock" {
        println!("{FIXTURE_READY} 0");
    }
    let mut sink = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut sink);
}

#[cfg(unix)]
fn rocksdb_style_lock(path: &Path) -> std::fs::File {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(path)
        .expect("open LOCK");
    // SAFETY: zeroed `flock` is valid; the fd is owned by `f` for the call.
    let mut fl: libc::flock = unsafe { std::mem::zeroed() };
    fl.l_type = libc::F_WRLCK as _;
    fl.l_whence = libc::SEEK_SET as _;
    let rc = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETLK, &fl) };
    assert_eq!(rc, 0, "fixture takes the LOCK: {}", std::io::Error::last_os_error());
    f
}

#[cfg(windows)]
fn rocksdb_style_lock(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
        .expect("open LOCK exclusively")
}

fn tmp(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-holder-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("tmp dir");
    p
}

// ---------------------------------------------------------------------------
// Chain database lock
// ---------------------------------------------------------------------------

#[test]
fn a_free_lock_is_taken_and_released() {
    let d = tmp("free");
    std::fs::write(d.join(CHAIN_DB_LOCK_FILE), b"").expect("LOCK");
    let lock = acquire_chain_db_lock(&d).expect("free lock is taken");
    assert!(lock.holds_file());
    drop(lock);
    let again = acquire_chain_db_lock(&d).expect("released on drop");
    drop(again);
    assert_eq!(find_blocker(&d, &[]), Ok(None));
}

#[test]
fn no_lock_file_is_free_and_none_is_created() {
    let d = tmp("nolock");
    let lock = acquire_chain_db_lock(&d).expect("no LOCK file: nothing to hold");
    assert!(!lock.holds_file());
    drop(lock);
    assert!(
        !d.join(CHAIN_DB_LOCK_FILE).exists(),
        "the probe never creates LOCK (the reset's file set is unchanged)"
    );
}

#[test]
fn an_orphan_holding_the_lock_is_reported_with_its_pid_and_path() {
    let d = tmp("held");
    let fx = hold_chain_db_lock(&d);
    let err = acquire_chain_db_lock(&d).expect_err("a held lock is not taken");
    let DbLockError::Held(holder) = err else {
        panic!("expected Held, got {err:?}");
    };
    #[cfg(windows)]
    assert_eq!(holder.pid, None, "Windows does not name a file's holder");
    #[cfg(unix)]
    {
        assert_eq!(holder.pid, Some(fx.pid), "F_GETLK names the holder");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert_eq!(
            holder.path.as_deref().map(PathBuf::from),
            Some(HolderFixture::exe()),
            "the holder's executable is named"
        );
    }
    let blocker = find_blocker(&d, &[])
        .expect("probe runs")
        .expect("a held lock blocks");
    assert_eq!(blocker.resource, BlockedResource::ChainDatabase);
    #[cfg(unix)]
    assert!(
        blocker.message.contains(&fx.pid.to_string()),
        "{}",
        blocker.message
    );
    assert!(blocker.message.contains("Quit that process"), "{}", blocker.message);
    assert!(blocker.message.contains("no chain data was deleted"));
    fx.stop();
    assert_eq!(find_blocker(&d, &[]), Ok(None), "free once the holder exits");
}

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

#[test]
fn a_port_held_by_another_process_is_reported() {
    let d = tmp("port");
    let fx = listen_on_loopback();
    assert!(port_in_use(fx.port));
    let blocker = find_blocker(&d, &[fx.port])
        .expect("probe runs")
        .expect("a held port blocks");
    assert_eq!(blocker.resource, BlockedResource::Port);
    assert_eq!(blocker.port, Some(fx.port));
    if let Some(pid) = blocker.pid {
        // The pid comes from lsof / netstat when present; when it is, it must be right.
        assert_eq!(pid, fx.pid);
        assert!(blocker.message.contains(&pid.to_string()));
    }
    assert!(blocker.message.contains(&format!("port {}", fx.port)));
    let port = fx.port;
    fx.stop();
    assert!(!port_in_use(port), "free once the holder exits");
}

#[test]
fn the_member_config_names_the_rpc_ws_and_p2p_ports() {
    let cfg = include_str!("../config/member-node.toml");
    assert_eq!(node_ports_from_config(cfg), vec![8545, 8546, 30303]);
    assert_eq!(node_ports_from_config("not toml ["), Vec::<u16>::new());
    assert_eq!(
        node_ports_from_config("[rpc]\nlisten_addr = \"127.0.0.1:9000\"\nws_addr = \"127.0.0.1:9000\"\n"),
        vec![9000]
    );
}

#[test]
fn netstat_listener_rows_are_parsed() {
    let out = "\nActive Connections\n\n  Proto  Local Address          Foreign Address        State           PID\n  \
               TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1012\n  \
               TCP    127.0.0.1:8545         0.0.0.0:0              LISTENING       4242\n  \
               TCP    127.0.0.1:8545         127.0.0.1:50000        ESTABLISHED     4242\n  \
               TCP    [::]:30303             [::]:0                 LISTENING       77\n";
    assert_eq!(parse_netstat_listener(out, 8545), Some(4242));
    assert_eq!(parse_netstat_listener(out, 30303), Some(77));
    assert_eq!(parse_netstat_listener(out, 8546), None);
}

#[test]
fn a_citrate_node_holder_is_named_as_an_older_node() {
    for path in [
        "/Applications/Citrate Core.app/Contents/MacOS/citrate",
        "/usr/lib/Citrate Core/citrate",
        r"C:\Program Files\Citrate Core\citrate.exe",
        "/x/target/release/citrate-aarch64-apple-darwin",
    ] {
        let h = HolderProcess {
            pid: Some(7),
            path: Some(path.to_string()),
        };
        assert!(h.is_citrate_node(), "{path}");
        let b = NodeBlocker::chain_database(h, Path::new("/d"));
        assert!(
            b.message.starts_with("An older Citrate node is still running (process 7, "),
            "{}",
            b.message
        );
    }
    for path in ["/usr/bin/anvil", "/Applications/Citrate Core.app/Contents/MacOS/citrate-core"] {
        let h = HolderProcess {
            pid: Some(7),
            path: Some(path.to_string()),
        };
        assert!(!h.is_citrate_node(), "{path}");
        assert!(NodeBlocker::port(8545, h)
            .message
            .starts_with("Another process is running"));
    }
    let unknown = NodeBlocker::chain_database(HolderProcess { pid: None, path: None }, Path::new("/d"));
    assert!(unknown.message.contains("a process the system did not identify"));
}

#[test]
fn the_blocker_serializes_for_the_ui() {
    let b = NodeBlocker::port(
        8545,
        HolderProcess {
            pid: Some(42),
            path: Some("/bin/x".into()),
        },
    );
    let v = serde_json::to_value(&b).expect("serialize");
    assert_eq!(v["resource"], "port");
    assert_eq!(v["port"], 8545);
    assert_eq!(v["pid"], 42);
    assert_eq!(v["path"], "/bin/x");
    assert!(v["message"].as_str().is_some_and(|m| m.contains("port 8545")));
}
