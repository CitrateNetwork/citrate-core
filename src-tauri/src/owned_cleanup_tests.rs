// Tests for the startup owned-process cleanup (SCL-S0.1, US-0.1).
//
// Two layers: pure selection tests over a synthetic process list, and native fixture tests that
// start real processes (left without their parent, exactly like a crash leftover) and run the
// real cleanup against them. Every fixture process is recorded and stopped by a guard on drop,
// and every fixture sleeps for a bounded time, so nothing outlives the test run.

use super::*;
use std::path::{Path, PathBuf};

fn entry(pid: u32, ppid: u32, exe: &str) -> ProcEntry {
    ProcEntry {
        pid,
        ppid,
        exe: PathBuf::from(exe),
    }
}

const OWNED: &str = "/opt/citrate-test-install/bin/citrate";
const MAIN: &str = "/opt/citrate-test-install/bin/citrate-core";

// ---------------------------------------------------------------------------
// Pure selection
// ---------------------------------------------------------------------------

#[test]
fn only_the_exact_owned_path_is_selected() {
    let procs = vec![
        entry(100, 1, OWNED),
        // Same directory, not one of our binaries.
        entry(101, 1, "/opt/citrate-test-install/bin/other-tool"),
        // Same name, another directory.
        entry(102, 1, "/somewhere/else/citrate"),
        // A longer path that starts with ours.
        entry(103, 1, "/opt/citrate-test-install/bin/citrate-old"),
        // A path that contains ours.
        entry(104, 1, "/x/opt/citrate-test-install/bin/citrate"),
    ];
    let got = select_orphans(&procs, &[PathBuf::from(OWNED)], Some(Path::new(MAIN)), 5);
    assert_eq!(got, vec![100]);
}

#[test]
fn self_init_and_own_children_are_never_selected() {
    let procs = vec![
        entry(5, 1, OWNED),
        entry(1, 0, OWNED),
        entry(0, 0, OWNED),
        entry(200, 5, OWNED),
        entry(201, 1, OWNED),
    ];
    let got = select_orphans(&procs, &[PathBuf::from(OWNED)], Some(Path::new(MAIN)), 5);
    assert_eq!(got, vec![201]);
}

#[test]
fn a_sidecar_whose_parent_is_a_live_instance_is_left_alone() {
    let procs = vec![
        entry(300, 1, MAIN),
        entry(301, 300, OWNED),
        // Parent alive but not our main executable: the sidecar is a leftover.
        entry(310, 1, "/bin/sh"),
        entry(311, 310, OWNED),
        // Parent gone from the list: a leftover.
        entry(321, 999_999, OWNED),
    ];
    let got = select_orphans(&procs, &[PathBuf::from(OWNED)], Some(Path::new(MAIN)), 5);
    assert_eq!(got, vec![311, 321]);
}

#[test]
fn no_owned_paths_selects_nothing() {
    let procs = vec![entry(400, 1, OWNED), entry(401, 1, MAIN)];
    assert!(select_orphans(&procs, &[], Some(Path::new(MAIN)), 5).is_empty());
}

#[test]
fn a_replaced_binary_still_matches_its_install_path() {
    assert_eq!(
        linux_exe_link_path(Path::new("/opt/citrate-test-install/bin/citrate (deleted)")),
        PathBuf::from(OWNED)
    );
    assert_eq!(linux_exe_link_path(Path::new(OWNED)), PathBuf::from(OWNED));
}

#[test]
fn the_parent_pid_is_read_after_the_last_paren() {
    assert_eq!(linux_stat_ppid("42 (citrate) S 7 42 42 0 -1"), Some(7));
    assert_eq!(linux_stat_ppid("42 (a) b) (c d) R 9 1 1"), Some(9));
    assert_eq!(linux_stat_ppid("42 citrate S 7"), None);
    assert_eq!(linux_stat_ppid("42 (x) S"), None);
}

/// Every sidecar binary the bundles ship next to the main executable is covered by the cleanup
/// list, and every name on the list resolves through a launcher resolver. (llama-server is
/// shipped in the `llama/` resource folder with its libraries and is not on the list.)
#[test]
fn the_owned_list_matches_the_bundled_sidecars() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut checked = 0;
    for conf in [
        "tauri.bundle-lite.conf.json",
        "tauri.bundle-node.conf.json",
        "tauri.bundle-linux.conf.json",
        "tauri.local-run.conf.json",
    ] {
        let text = std::fs::read_to_string(manifest.join(conf)).expect("read bundle config");
        let v: serde_json::Value = serde_json::from_str(&text).expect("parse bundle config");
        let bins = v["bundle"]["externalBin"]
            .as_array()
            .expect("externalBin list");
        for b in bins {
            let name = b
                .as_str()
                .and_then(|s| s.rsplit('/').next())
                .expect("externalBin entry");
            if name == "llama-server" {
                continue;
            }
            assert!(
                crate::OWNED_SIDECARS.contains(&name),
                "{conf}: bundled sidecar {name} is missing from OWNED_SIDECARS"
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
}

// ---------------------------------------------------------------------------
// Native fixtures (macOS and Linux)
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::super::*;
    use std::io::BufRead;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    /// Fixture processes sleep this long at most, so even a crashed test leaves nothing behind
    /// for long.
    const SLEEP_MS: &str = "60000";

    /// Compile the fixture once per test process (under OUT_DIR, never in the checkout).
    fn fixture_bin() -> PathBuf {
        static BIN: OnceLock<PathBuf> = OnceLock::new();
        BIN.get_or_init(|| {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("cleanup_fixture.rs");
            let out_dir = PathBuf::from(env!("OUT_DIR")).join("test-fixtures");
            std::fs::create_dir_all(&out_dir).expect("create fixture output directory");
            let output = out_dir.join(format!("cleanup_fixture-{}", std::process::id()));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let status = Command::new(rustc)
                .arg("--edition=2021")
                .arg(&source)
                .arg("-o")
                .arg(&output)
                .status()
                .expect("run rustc for the cleanup fixture");
            assert!(status.success(), "cleanup fixture must compile");
            output
        })
        .clone()
    }

    /// Owns everything a test starts: stops each recorded process (only while it still runs the
    /// fixture path it was started from, so a reused pid is never signalled), reaps held
    /// parents, and removes the test's directories.
    struct Fixtures {
        dirs: Vec<PathBuf>,
        detached: Vec<(u32, PathBuf)>,
        held: Vec<Child>,
    }

    impl Fixtures {
        fn new() -> Self {
            Fixtures {
                dirs: Vec::new(),
                detached: Vec::new(),
                held: Vec::new(),
            }
        }

        /// A fresh directory for this test. Tags are distinct and none is a prefix of another.
        fn dir(&mut self, tag: &str) -> PathBuf {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let d = std::env::temp_dir()
                .join(format!("scl-s01-{tag}-{}-{nanos}", std::process::id()));
            std::fs::create_dir_all(&d).expect("create fixture dir");
            // Canonical, so the paths compare with what the kernel reports.
            let d = std::fs::canonicalize(&d).expect("canonicalize fixture dir");
            self.dirs.push(d.clone());
            d
        }

        /// Copy the fixture binary to `path`.
        fn install(&self, path: &Path) {
            std::fs::copy(fixture_bin(), path).expect("copy fixture binary");
        }

        /// Start `exe` with `arg0` and `args`, left without its parent (the launcher exits at
        /// once), and return its pid.
        fn start_orphan(&mut self, exe: &Path, arg0: &str, args: &[&str]) -> u32 {
            let out = Command::new(fixture_bin())
                .arg("detach")
                .arg(exe)
                .arg(arg0)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .expect("run the fixture launcher");
            let pid: u32 = String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .expect("launcher prints the pid");
            self.detached.push((pid, exe.to_path_buf()));
            assert!(
                wait_until(Duration::from_secs(5), || runs(pid, exe)),
                "fixture {pid} did not start"
            );
            pid
        }

        /// Start `parent_exe hold <child_exe> args...`: a live parent with its own child.
        /// Returns the child's pid.
        fn start_held(&mut self, parent_exe: &Path, child_exe: &Path, args: &[&str]) -> u32 {
            let mut parent = Command::new(parent_exe)
                .arg("hold")
                .arg(child_exe)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("start the holding parent");
            let mut line = String::new();
            if let Some(out) = parent.stdout.take() {
                let _ = std::io::BufReader::new(out).read_line(&mut line);
            }
            self.held.push(parent);
            let pid: u32 = line.trim().parse().expect("holding parent prints the pid");
            self.detached.push((pid, child_exe.to_path_buf()));
            assert!(
                wait_until(Duration::from_secs(5), || runs(pid, child_exe)),
                "held fixture {pid} did not start"
            );
            pid
        }
    }

    impl Drop for Fixtures {
        fn drop(&mut self) {
            for (pid, exe) in &self.detached {
                if runs(*pid, exe) {
                    // SAFETY: kill(2) on one positive pid, checked above to run our fixture.
                    unsafe {
                        libc::kill(*pid as libc::pid_t, libc::SIGKILL);
                    }
                }
            }
            for c in &mut self.held {
                let _ = c.kill();
                let _ = c.wait();
            }
            for d in &self.dirs {
                let _ = std::fs::remove_dir_all(d);
            }
        }
    }

    /// True while `pid` runs the binary at `exe` (a reaped or reused pid reads false).
    fn runs(pid: u32, exe: &Path) -> bool {
        os::exe_of(pid).map(|e| e == exe).unwrap_or(false)
    }

    fn wait_until(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        f()
    }

    /// Survival check: still running our fixture a moment after the cleanup returned.
    fn survives(pid: u32, exe: &Path) -> bool {
        std::thread::sleep(Duration::from_millis(300));
        runs(pid, exe)
    }

    /// One installation: `<dir>/citrate` (owned sidecar) and `<dir>/citrate-core` (main exe).
    fn install(fx: &mut Fixtures, tag: &str) -> (PathBuf, PathBuf) {
        let dir = fx.dir(tag);
        let owned = dir.join("citrate");
        let main = dir.join("citrate-core");
        fx.install(&owned);
        fx.install(&main);
        (owned, main)
    }

    #[test]
    fn a_leftover_owned_sidecar_at_the_exact_path_is_stopped() {
        let mut fx = Fixtures::new();
        let (owned, main) = install(&mut fx, "positive");
        let pid = fx.start_orphan(&owned, &owned.to_string_lossy(), &["sleep", SLEEP_MS]);

        let signalled = cleanup(std::slice::from_ref(&owned), Some(&main));

        assert!(signalled.contains(&pid), "the leftover must be signalled");
        assert!(
            wait_until(Duration::from_secs(5), || !runs(pid, &owned)),
            "the leftover must be gone"
        );
    }

    #[test]
    fn a_process_that_mentions_our_binary_dir_on_its_command_line_survives() {
        let mut fx = Fixtures::new();
        let (owned, main) = install(&mut fx, "cmdline");
        let other_dir = fx.dir("elsewhere");
        let tool = other_dir.join("unrelated-tool");
        fx.install(&tool);
        let bin_dir = owned.parent().expect("bin dir").to_string_lossy().to_string();
        let owned_s = owned.to_string_lossy().to_string();
        let decoy = fx.start_orphan(&tool, "unrelated-tool", &["sleep", SLEEP_MS, &bin_dir, &owned_s]);
        // A real leftover alongside it, so the cleanup demonstrably ran.
        let leftover = fx.start_orphan(&owned, &owned_s, &["sleep", SLEEP_MS]);

        let signalled = cleanup(std::slice::from_ref(&owned), Some(&main));

        assert!(!signalled.contains(&decoy), "a non-owned process must not be signalled");
        assert!(survives(decoy, &tool), "the decoy must still be running");
        assert!(signalled.contains(&leftover));
    }

    #[test]
    fn a_process_that_names_our_binary_as_argv0_survives() {
        let mut fx = Fixtures::new();
        let (owned, main) = install(&mut fx, "argzero");
        let other_dir = fx.dir("spoofdir");
        let tool = other_dir.join("unrelated-tool");
        fx.install(&tool);
        let decoy = fx.start_orphan(&tool, &owned.to_string_lossy(), &["sleep", SLEEP_MS]);

        let signalled = cleanup(std::slice::from_ref(&owned), Some(&main));

        assert!(!signalled.contains(&decoy));
        assert!(survives(decoy, &tool), "argv[0] alone must never make a process ours");
    }

    #[test]
    fn a_same_named_binary_at_another_path_survives() {
        let mut fx = Fixtures::new();
        let (owned, main) = install(&mut fx, "samename");
        let other_dir = fx.dir("otherinstall");
        let same_name = other_dir.join("citrate");
        fx.install(&same_name);
        let decoy = fx.start_orphan(&same_name, &same_name.to_string_lossy(), &["sleep", SLEEP_MS]);

        let signalled = cleanup(std::slice::from_ref(&owned), Some(&main));

        assert!(!signalled.contains(&decoy));
        assert!(survives(decoy, &same_name), "a name match alone must never make a process ours");
    }

    #[test]
    fn an_owned_sidecar_of_a_live_instance_survives() {
        let mut fx = Fixtures::new();
        let (owned, main) = install(&mut fx, "liveparent");
        let child = fx.start_held(&main, &owned, &["sleep", SLEEP_MS]);

        let signalled = cleanup(std::slice::from_ref(&owned), Some(&main));

        assert!(!signalled.contains(&child));
        assert!(survives(child, &owned), "a running instance keeps its sidecars");
    }
}
