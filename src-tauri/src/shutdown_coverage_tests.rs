//! SCL-S0.5: shutdown coverage. On app quit the BGE embedding server stops together with Hermes,
//! and a Hermes-sidecar worker exits when its stdin closes (so it never outlives the sidecar).
//!
//! The tests run real child processes and clean every one of them up, pass or fail.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::embed_serve::EmbedServer;
#[cfg(unix)]
use crate::hermes::HermesManager;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-s05-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

#[cfg(unix)]
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().expect("addr").port()
}

/// Whether `pid` names a live process (signal 0 is an existence probe and sends nothing).
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) only checks existence and permission.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Kills any listed pid that is still alive (test cleanup on a failed assertion).
#[cfg(unix)]
struct Reaper(Vec<u32>);

#[cfg(unix)]
impl Drop for Reaper {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if alive(pid) {
                // SAFETY: a pid this test's own children held; SIGKILL ends a leftover.
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
    }
}

/// A stand-in binary that ignores its argv and stays up until it is signalled: a real, long-lived
/// child process, so the stop path is exercised against a live pid and not a mock.
#[cfg(unix)]
fn long_lived_bin(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, "#!/bin/sh\nexec sleep 600\n").expect("write stand-in");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    p
}

#[cfg(unix)]
fn wait_for(what: &str, timeout: Duration, mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(unix)]
fn manager_with_embed(d: &Path, token_path: PathBuf) -> HermesManager {
    let embed = EmbedServer::new(
        long_lived_bin(d, "llama-server"),
        {
            let m = d.join("bge.gguf");
            std::fs::write(&m, b"x").expect("model stand-in");
            m
        },
        free_port(),
        d.join("embed.key"),
        d.join("embed-crashes.log"),
    )
    .assume_verified_for_test();
    HermesManager::new(
        long_lived_bin(d, "hermes"),
        token_path,
        d.join("hermes-crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{}", free_port()))
    .with_health_interval(Duration::from_secs(600))
    .with_embed(embed)
}

/// Quit path: `shutdown_all_sidecars` -> `hermes::shutdown()` -> `HermesManager::stop()`. Both the
/// Hermes child and the embedding server child are gone when it returns, and the embed key file
/// is removed.
#[cfg(unix)]
#[test]
fn quit_stops_the_embedding_server_together_with_hermes() {
    let d = tmp("quit");
    let mgr = manager_with_embed(&d, d.join("bearer.token"));
    mgr.start().expect("hermes starts");
    let embed = mgr.embed_for_test().expect("embed wired");
    let mut pids = (None, None);
    wait_for("both children", Duration::from_secs(10), || {
        pids = (mgr.sidecar_pid_for_test(), embed.pid_for_test());
        matches!(pids, (Some(h), Some(e)) if alive(h) && alive(e))
    });
    let (Some(hermes_pid), Some(embed_pid)) = pids else {
        unreachable!("waited for both pids")
    };
    let _reaper = Reaper(vec![hermes_pid, embed_pid]);
    assert!(
        d.join("embed.key").exists(),
        "the key file exists while it runs"
    );

    mgr.stop();

    assert!(!alive(hermes_pid), "the Hermes child is gone after stop");
    assert!(
        !alive(embed_pid),
        "the embedding server child is gone with Hermes"
    );
    assert!(
        !embed.is_started(),
        "the embedding server is not left started"
    );
    assert!(!d.join("embed.key").exists(), "the key file goes with it");
    let _ = std::fs::remove_dir_all(&d);
}

/// The gap S0.5 found: when Hermes itself failed to start after the embedding server came up, the
/// embedding server kept running (until quit) with no Hermes to use it.
#[cfg(unix)]
#[test]
fn a_hermes_that_fails_to_start_leaves_no_embedding_server_running() {
    let d = tmp("failstart");
    // The bearer cannot be written: its parent "directory" is a regular file.
    let blocker = d.join("not-a-dir");
    std::fs::write(&blocker, b"").expect("blocker");
    let mgr = manager_with_embed(&d, blocker.join("bearer.token"));
    let embed = mgr.embed_for_test().expect("embed wired");

    let err = mgr.start();
    let embed_pid = embed.pid_for_test();
    let _reaper = Reaper(embed_pid.into_iter().collect());

    assert!(
        err.is_err(),
        "the bearer write fails, so Hermes does not start"
    );
    assert!(!mgr.is_started());
    assert!(
        !embed.is_started(),
        "the embedding server must not run without Hermes"
    );
    if let Some(pid) = embed_pid {
        assert!(!alive(pid), "no embedding server child is left");
    }
    assert!(!d.join("embed.key").exists(), "no key file is left");
    let _ = std::fs::remove_dir_all(&d);
}

/// The quit hook reaches Hermes (and so the embedding server, which only Hermes owns).
#[test]
fn the_quit_hook_stops_hermes_and_hermes_stop_stops_the_embedding_server() {
    let lib = include_str!("lib.rs");
    let hook = lib
        .split("pub(crate) fn shutdown_all_sidecars")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("shutdown_all_sidecars exists");
    assert!(hook.contains("hermes::shutdown();"), "{hook}");
    assert!(
        lib.contains("tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit"),
        "the quit hook runs on ExitRequested and Exit"
    );
    let hermes = include_str!("hermes.rs");
    let shutdown = hermes
        .split("pub fn shutdown()")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("hermes::shutdown exists");
    assert!(shutdown.contains("m.stop();"), "{shutdown}");
    let stop = hermes
        .split("    pub fn stop(&self) {")
        .nth(1)
        .and_then(|s| s.split("\n    }\n").next())
        .expect("HermesManager::stop exists");
    assert!(stop.contains("embed.stop();"), "{stop}");
    // No other path in core starts the embedding server.
    let starts = hermes.matches("ensure_started()").count();
    assert_eq!(starts, 1, "only Hermes start starts the embedding server");
}

/// Live: a real Hermes-sidecar worker (`<sidecar> --worker toolchain`) answers a ping, then exits
/// on its own, with status 0, once its stdin closes. Run with
/// `CITRATE_E2E_HERMES_BIN=<citrate-agent-sidecar>`.
#[test]
fn live_a_hermes_worker_exits_when_its_stdin_closes() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let Ok(bin) = std::env::var("CITRATE_E2E_HERMES_BIN") else {
        eprintln!("skipped: set CITRATE_E2E_HERMES_BIN");
        return;
    };
    let d = tmp("worker");
    let mut child = Command::new(bin)
        .args(["--worker", "toolchain"])
        .env("CITRATE_HERMES_TOOLCHAIN", "1")
        .env("HOME", &d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the worker");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut out = BufReader::new(child.stdout.take().expect("stdout"));
    let ping = writeln!(stdin, r#"{{"id":7,"method":"ping"}}"#).and_then(|()| stdin.flush());
    let mut line = String::new();
    let read = out.read_line(&mut line);
    if ping.is_err() || read.is_err() || !line.contains("\"id\":7") {
        let _ = child.kill();
        let _ = child.wait();
        panic!("the worker did not answer a ping: {line:?}");
    }

    drop(stdin);
    let start = Instant::now();
    let status = loop {
        if let Ok(Some(s)) = child.try_wait() {
            break Some(s);
        }
        if start.elapsed() > Duration::from_secs(10) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        panic!("the worker outlived its closed stdin by 10 s");
    };
    assert!(status.success(), "EOF is a clean exit: {status:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Live: the sidecar dies without a clean shutdown (SIGKILL). The only holder of the worker's
/// stdin write end is gone, so the worker reads EOF and exits. Run with
/// `CITRATE_E2E_HERMES_BIN=<citrate-agent-sidecar>`.
#[cfg(unix)]
#[test]
fn live_a_hermes_worker_exits_when_the_process_holding_its_stdin_is_killed() {
    use std::process::{Command, Stdio};
    let Ok(bin) = std::env::var("CITRATE_E2E_HERMES_BIN") else {
        eprintln!("skipped: set CITRATE_E2E_HERMES_BIN");
        return;
    };
    let d = tmp("worker-kill");
    // Stand-in for the sidecar: a real process that holds the write end of the worker's stdin.
    let mut holder = Command::new("sleep")
        .arg("600")
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn the holder");
    let pipe = holder.stdout.take().expect("pipe");
    let mut worker = Command::new(bin)
        .args(["--worker", "toolchain"])
        .env("CITRATE_HERMES_TOOLCHAIN", "1")
        .env("HOME", &d)
        .stdin(Stdio::from(pipe))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the worker");
    let _reaper = Reaper(vec![holder.id(), worker.id()]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        matches!(worker.try_wait(), Ok(None)),
        "the worker serves while its stdin is open"
    );

    let _ = holder.kill();
    let _ = holder.wait();

    let start = Instant::now();
    let status = loop {
        if let Ok(Some(s)) = worker.try_wait() {
            break s;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the worker outlived the holder of its stdin by 10 s"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(status.success(), "EOF is a clean exit: {status:?}");
    let _ = std::fs::remove_dir_all(&d);
}
