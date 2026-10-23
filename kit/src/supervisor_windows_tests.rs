use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TmpDir(PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "citrate-core-sup-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_for_logs(
    supervisor: &Supervisor,
    predicate: impl Fn(&[LogLine]) -> bool,
    timeout: Duration,
) -> Vec<LogLine> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let logs = supervisor.logs();
        if predicate(&logs) || std::time::Instant::now() >= deadline {
            return logs;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn sidecar_candidates_try_exe_suffix_first_on_windows() {
    assert_eq!(
        super::sidecar_candidate_names("citrate", ".exe"),
        vec!["citrate.exe".to_string(), "citrate".to_string()]
    );
    assert_eq!(
        super::sidecar_candidate_names("citrate", ""),
        vec!["citrate".to_string()]
    );
    assert_eq!(
        super::sidecar_candidate_names("llama-server", std::env::consts::EXE_SUFFIX)
            .first()
            .map(String::as_str),
        Some("llama-server.exe")
    );
}

/// How long the console probe may take to report. The probe runs in Windows
/// PowerShell 5.1, whose `Add-Type` compiles the P/Invoke shim by launching
/// `csc.exe` out of process. On hosted runners that start-up plus compile has
/// been measured at 2-9 s when it succeeds, and a 10 s budget timed out in
/// roughly one run in eight with no output at all. The budget only bounds a
/// wait for lines that are already expected; it does not relax what is checked.
const CONSOLE_PROBE_TIMEOUT: Duration = Duration::from_secs(90);

#[test]
fn windows_child_has_no_console_and_logs_remain_piped() {
    let tmp = TmpDir::new("windows-no-console");
    let crash_file = tmp.path("crashes.jsonl");
    // CHILD_UP is written before the slow Add-Type step, so a failure report
    // separates "the child never produced output" from "the probe was slow".
    // The sleep outlasts CONSOLE_PROBE_TIMEOUT so the first child is the one
    // observed; `stop()` ends it.
    let script = concat!(
        "Write-Output 'CHILD_UP'; ",
        "Add-Type -TypeDefinition '",
        "using System; using System.Runtime.InteropServices; ",
        "public static class NativeConsole { ",
        "[DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; ",
        "Write-Output ('CONSOLE_HANDLE_' + [NativeConsole]::GetConsoleWindow().ToInt64()); ",
        "[Console]::Error.WriteLine('ERR_OOPS'); ",
        "Start-Sleep -Seconds 300"
    );
    let spec = SidecarSpec::new(
        "windows-child",
        PathBuf::from("powershell.exe"),
        vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ],
    );
    let cfg = SupervisorConfig::new(spec, &crash_file);
    let started = std::time::Instant::now();
    let supervisor = Supervisor::start(cfg).expect("supervisor starts");

    let has = |lines: &[LogLine], text: &str, stream: &str| {
        lines
            .iter()
            .any(|line| line.line == text && line.stream == stream)
    };
    let logs = wait_for_logs(
        &supervisor,
        |lines| {
            // Any CONSOLE_HANDLE_ line settles the console question, so stop
            // waiting on it rather than only on the expected value.
            lines
                .iter()
                .any(|line| line.line.starts_with("CONSOLE_HANDLE_") && line.stream == "out")
                && has(lines, "ERR_OOPS", "err")
        },
        CONSOLE_PROBE_TIMEOUT,
    );
    let elapsed = started.elapsed();
    let status = supervisor.status();
    eprintln!("console probe settled after {elapsed:?}");

    assert!(
        has(&logs, "CHILD_UP", "out"),
        "the child wrote no stdout within {elapsed:?}; status {status:?}; logs {logs:?}"
    );
    assert!(
        has(&logs, "CONSOLE_HANDLE_0", "out"),
        "Windows child inherited or created a console (after {elapsed:?}; status {status:?}): {logs:?}"
    );
    assert!(
        has(&logs, "ERR_OOPS", "err"),
        "stderr was not captured while CREATE_NO_WINDOW was active (after {elapsed:?}; status {status:?}): {logs:?}"
    );

    supervisor.stop();
    assert_eq!(supervisor.status().state, SupervisorState::Off);
}

#[test]
fn windows_immediate_exit_reaches_retry_bound() {
    let tmp = TmpDir::new("windows-retry-bound");
    let crash_file = tmp.path("crashes.jsonl");
    let spec = SidecarSpec::new(
        "windows-flapper",
        PathBuf::from("cmd.exe"),
        vec!["/D".into(), "/S".into(), "/C".into(), "exit /b 7".into()],
    );
    let mut cfg = SupervisorConfig::new(spec, &crash_file);
    cfg.backoff = BackoffPolicy {
        base_delay: Duration::from_millis(5),
        multiplier: 2,
        max_delay: Duration::from_millis(20),
        max_retries: 2,
    };
    let supervisor = Supervisor::start(cfg).expect("supervisor starts");

    let status = supervisor.wait_until(
        |snapshot| matches!(snapshot, SupervisorState::Failed),
        Duration::from_secs(10),
    );
    assert_eq!(status.state, SupervisorState::Failed);
    assert_eq!(status.restarts, 3);

    let records = std::fs::read_to_string(&crash_file).expect("read crash records");
    assert_eq!(records.lines().count(), 3);
}
