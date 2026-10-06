// SCL-S0.3: the managed browser does not outlive Hermes. The Hermes sidecar starts its managed
// browser itself; when core has to hard-kill the sidecar after its stop grace, or the sidecar
// ended while core was not running, core stops that browser and removes its profile.
//
// The sidecar and the browser are stand-ins: a shell script that ignores SIGTERM (so core's stop
// has to escalate to SIGKILL) and starts a "browser" child with the managed-browser command-line
// shape. Every test stops the processes it started and removes its folders, pass or fail.

use super::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "citrate-hermes-browser-test-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("scratch folder");
    p
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("a free loopback port")
}

fn script(path: &Path, body: &str) -> PathBuf {
    std::fs::write(path, body).expect("write a stand-in script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path.to_path_buf()
}

/// A stand-in Hermes: ignores SIGTERM, starts `$1` as its managed browser with profile `$2`,
/// writes the browser's pid to `$3`, and runs until killed.
fn fake_hermes(dir: &Path) -> PathBuf {
    script(
        &dir.join("fake-hermes.sh"),
        r#"#!/bin/sh
trap '' TERM
mkdir -p "$2"
"$1" --headless=new --remote-debugging-port=0 "--user-data-dir=$2" about:blank &
echo $! > "$3.tmp" && mv "$3.tmp" "$3"
while :; do sleep 1; done
"#,
    )
}

/// A stand-in browser: runs until killed.
fn fake_browser(dir: &Path) -> PathBuf {
    script(
        &dir.join("fake-browser.sh"),
        "#!/bin/sh\nwhile :; do sleep 1; done\n",
    )
}

/// A profile path shaped like the runtime's, directly in the temporary folder.
fn profile_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "citrate-browser-coretest-{tag}-{}",
        std::process::id()
    ))
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        if let Some(i) = s.rfind(')') {
            if s[i + 1..].trim_start().starts_with('Z') {
                return false;
            }
        }
    }
    true
}

fn gone_within(pid: i32, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !alive(pid)
}

fn read_pid(file: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(s) = std::fs::read_to_string(file) {
            if let Ok(p) = s.trim().parse::<i32>() {
                return p;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the stand-in Hermes never started its browser"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Stops the stand-in browser and removes the test folders, whatever the outcome.
struct Cleanup {
    pids: Vec<i32>,
    dirs: Vec<PathBuf>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for pid in &self.pids {
            if alive(*pid) {
                // SAFETY: kill(2) on the stand-in process this test started.
                unsafe {
                    libc::kill(*pid, libc::SIGKILL);
                }
            }
        }
        for d in &self.dirs {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[test]
fn a_hard_killed_hermes_leaves_no_managed_browser_behind() {
    let dir = scratch("hardkill");
    let profile = profile_path("hardkill");
    let pidfile = dir.join("browser.pid");
    let mut cleanup = Cleanup {
        pids: Vec::new(),
        dirs: vec![dir.clone(), profile.clone()],
    };
    let mgr = HermesManager::new(
        fake_hermes(&dir),
        dir.join("bearer.token"),
        dir.join("crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{}", free_port()))
    .with_spawn_args(vec![
        fake_browser(&dir).display().to_string(),
        profile.display().to_string(),
        pidfile.display().to_string(),
    ]);
    mgr.start().expect("the stand-in Hermes starts");
    let browser = read_pid(&pidfile);
    cleanup.pids.push(browser);
    assert!(alive(browser), "the browser runs while Hermes runs");
    assert!(profile.is_dir(), "the profile exists while the browser runs");

    // The stand-in ignores SIGTERM, so this is core's hard kill after the stop grace.
    mgr.stop();

    assert!(
        gone_within(browser, Duration::from_secs(3)),
        "the managed browser outlived the hard-killed Hermes"
    );
    assert!(!profile.exists(), "the browser profile was left behind");
}

/// A stand-in Hermes for the restart: runs until stopped.
fn idle_hermes(dir: &Path) -> PathBuf {
    script(
        &dir.join("idle-hermes.sh"),
        "#!/bin/sh\nwhile :; do sleep 1; done\n",
    )
}

#[test]
fn the_next_start_stops_a_browser_left_by_a_hermes_that_ended_without_core() {
    let dir = scratch("nextstart");
    let profile = profile_path("nextstart");
    let pidfile = dir.join("browser.pid");
    let record = dir.join("core-lifecycle").join(crate::owned_browser::RECORD_FILE);
    let mut cleanup = Cleanup {
        pids: Vec::new(),
        dirs: vec![dir.clone(), profile.clone()],
    };
    // The earlier run: a Hermes this app started and observed (as its supervisor does), which
    // then ended while the app was not there to clean up (the app itself was killed).
    let mut earlier = std::process::Command::new(fake_hermes(&dir))
        .args([
            fake_browser(&dir).display().to_string(),
            profile.display().to_string(),
            pidfile.display().to_string(),
        ])
        .spawn()
        .expect("start the earlier stand-in Hermes");
    let browser = read_pid(&pidfile);
    cleanup.pids.push(browser);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        crate::owned_browser::observe(&record, earlier.id());
        if record.exists() || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = earlier.kill();
    let _ = earlier.wait();
    assert!(record.exists(), "the earlier run's browser was recorded");
    assert!(alive(browser), "the browser outlived the earlier Hermes");

    // The next start.
    let mgr = HermesManager::new(
        idle_hermes(&dir),
        dir.join("bearer.token"),
        dir.join("crashes.log"),
    )
    .with_browser_record(record.clone())
    .with_control_addr(&format!("127.0.0.1:{}", free_port()));
    mgr.start().expect("the next Hermes starts");
    let left = gone_within(browser, Duration::from_secs(3));
    let profile_left = profile.exists();
    mgr.stop();

    assert!(left, "the earlier run's browser is still running after the next start");
    assert!(!profile_left, "the earlier run's profile is still there");
    assert!(!record.exists(), "the handled record is removed");
}
