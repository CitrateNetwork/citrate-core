// SCL-S0.3: core records only managed browsers that are children of its own sidecar, and on
// cleanup signals only a recorded process whose live identity still matches the record.
//
// Stand-ins: a shell-script "sidecar" that starts a shell-script "browser" with the managed-browser
// command-line shape, and decoys with the same shape that are not the sidecar's children. Every
// test kills and reaps the processes it started and removes its folders, pass or fail.

use super::*;
use std::process::{Child, Command};
use std::time::Instant;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "citrate-owned-browser-test-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("scratch folder");
    p
}

fn profile_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "citrate-browser-unittest-{tag}-{}",
        std::process::id()
    ))
}

fn script(path: &Path, body: &str) -> PathBuf {
    std::fs::write(path, body).expect("write a stand-in script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path.to_path_buf()
}

fn fake_browser(dir: &Path) -> PathBuf {
    script(
        &dir.join("fake-browser.sh"),
        "#!/bin/sh\nwhile :; do sleep 1; done\n",
    )
}

/// Starts `$1` as its browser with profile `$2`, writes the browser pid to `$3`, runs until killed.
fn fake_sidecar(dir: &Path) -> PathBuf {
    script(
        &dir.join("fake-sidecar.sh"),
        r#"#!/bin/sh
mkdir -p "$2"
"$1" --remote-debugging-port=0 "--user-data-dir=$2" &
echo $! > "$3.tmp" && mv "$3.tmp" "$3"
while :; do sleep 1; done
"#,
    )
}

fn browser_args(profile: &Path) -> Vec<String> {
    vec![
        "--remote-debugging-port=0".into(),
        format!("--user-data-dir={}", profile.display()),
    ]
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

fn read_pid(file: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(s) = std::fs::read_to_string(file) {
            if let Ok(p) = s.trim().parse::<i32>() {
                return p;
            }
        }
        assert!(Instant::now() < deadline, "the stand-in never started");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Wait until the OS reports the process's command line (a just-forked shell may not have
/// exec'd its script yet).
fn wait_shaped(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if os::info(pid).is_some_and(|i| managed_profile(&i.argv).is_some()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Kills and reaps the children this test started, kills the processes it learned about, and
/// removes its folders, whatever the outcome.
struct Cleanup {
    children: Vec<Child>,
    pids: Vec<i32>,
    dirs: Vec<PathBuf>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for pid in &self.pids {
            if alive(*pid) {
                // SAFETY: kill(2) on a stand-in process this test started.
                unsafe {
                    libc::kill(*pid, libc::SIGKILL);
                }
            }
        }
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
        for d in &self.dirs {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

#[test]
fn only_a_managed_profile_directly_in_the_temp_folder_is_accepted() {
    let tmp = std::env::temp_dir();
    assert!(profile_ok(&tmp.join("citrate-browser-1a2b-3c")));
    assert!(!profile_ok(Path::new("citrate-browser-relative")));
    assert!(!profile_ok(&tmp.join("citrate-browser-")), "prefix alone");
    assert!(!profile_ok(&tmp.join("Default")), "another name");
    assert!(
        !profile_ok(&tmp.join("x").join("citrate-browser-1")),
        "nested deeper"
    );
    assert!(
        !profile_ok(Path::new("/citrate-browser-1")),
        "outside the temp folder"
    );
}

#[test]
fn only_the_sidecars_own_browser_children_are_recorded() {
    let dir = scratch("observe");
    let profile = profile_path("observe");
    let decoy_profile = profile_path("observe-decoy");
    let record = dir.join(RECORD_FILE);
    let pidfile = dir.join("browser.pid");
    let mut c = Cleanup {
        children: Vec::new(),
        pids: Vec::new(),
        dirs: vec![dir.clone(), profile.clone(), decoy_profile.clone()],
    };
    let sidecar = Command::new(fake_sidecar(&dir))
        .args([
            fake_browser(&dir).display().to_string(),
            profile.display().to_string(),
            pidfile.display().to_string(),
        ])
        .spawn()
        .expect("start the stand-in sidecar");
    let sidecar_pid = sidecar.id();
    c.children.push(sidecar);
    let browser = read_pid(&pidfile);
    c.pids.push(browser);
    // Same shape, but this test's child, not the sidecar's.
    std::fs::create_dir_all(&decoy_profile).expect("decoy profile");
    let decoy = Command::new(fake_browser(&dir))
        .args(browser_args(&decoy_profile))
        .spawn()
        .expect("start the decoy");
    let decoy_pid = decoy.id();
    c.children.push(decoy);
    wait_shaped(browser as u32);
    wait_shaped(decoy_pid);

    observe(&record, sidecar_pid);

    let got = read(&record).browsers;
    assert_eq!(got.len(), 1, "exactly the sidecar's browser: {got:?}");
    assert_eq!(got[0].pid, browser as u32);
    assert_eq!(got[0].profile, profile);
    assert_ne!(got[0].pid, decoy_pid, "the decoy is not the sidecar's child");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&record)
            .expect("record")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the record is owner-only");
    }
}

#[test]
fn a_recorded_pid_whose_identity_changed_is_left_alone() {
    let dir = scratch("identity");
    let profile = profile_path("identity");
    let record = dir.join(RECORD_FILE);
    let mut c = Cleanup {
        children: Vec::new(),
        pids: Vec::new(),
        dirs: vec![dir.clone(), profile.clone()],
    };
    std::fs::create_dir_all(&profile).expect("profile");
    let decoy = Command::new(fake_browser(&dir))
        .args(browser_args(&profile))
        .spawn()
        .expect("start the decoy");
    let pid = decoy.id();
    c.children.push(decoy);
    wait_shaped(pid);
    let real = browser_record(pid).expect("the decoy has the managed shape");

    // The same pid, but a different start time (as after the id was reused) or executable.
    for forged in [
        BrowserRecord {
            start: real.start + 1,
            ..real.clone()
        },
        BrowserRecord {
            exe: PathBuf::from("/nonexistent/other-binary"),
            ..real.clone()
        },
        BrowserRecord {
            boot: format!("{}-other", real.boot),
            ..real.clone()
        },
    ] {
        write(
            &record,
            &RecordFile {
                browsers: vec![forged],
            },
        )
        .expect("write the record");
        apply(&record);
        assert!(alive(pid as i32), "a process that is not the recorded one was signalled");
        assert!(profile.is_dir(), "the profile of a running process was removed");
    }

    // The genuine record does stop it.
    write(
        &record,
        &RecordFile {
            browsers: vec![real],
        },
    )
    .expect("write the record");
    let report = apply(&record);
    assert_eq!(report.stopped, 1);
    assert_eq!(report.profiles_removed, 1);
    assert!(!profile.exists(), "the recorded profile was removed");
    assert!(!record.exists(), "a fully handled record is removed");
}

#[test]
fn a_profile_outside_the_managed_shape_is_never_removed() {
    let dir = scratch("profile");
    let record = dir.join(RECORD_FILE);
    let keep = dir.join("citrate-browser-not-in-temp");
    std::fs::create_dir_all(&keep).expect("folder");
    let _c = Cleanup {
        children: Vec::new(),
        pids: Vec::new(),
        dirs: vec![dir.clone()],
    };
    write(
        &record,
        &RecordFile {
            browsers: vec![BrowserRecord {
                pid: u32::MAX - 7,
                start: 1,
                boot: "none".into(),
                exe: PathBuf::from("/nonexistent"),
                pgid: u32::MAX - 7,
                profile: keep.clone(),
            }],
        },
    )
    .expect("write the record");
    let report = apply(&record);
    assert_eq!(report, ApplyReport::default());
    assert!(keep.is_dir(), "a folder that is not a managed profile was removed");
}
