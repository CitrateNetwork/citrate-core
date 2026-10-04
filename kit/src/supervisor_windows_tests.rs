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

#[test]
fn windows_child_has_no_console_and_logs_remain_piped() {
    let tmp = TmpDir::new("windows-no-console");
    let crash_file = tmp.path("crashes.jsonl");
    let script = concat!(
        "Add-Type -TypeDefinition '",
        "using System; using System.Runtime.InteropServices; ",
        "public static class NativeConsole { ",
        "[DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; ",
        "Write-Output ('CONSOLE_HANDLE_' + [NativeConsole]::GetConsoleWindow().ToInt64()); ",
        "[Console]::Error.WriteLine('ERR_OOPS'); ",
        "Start-Sleep -Seconds 60"
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
    let supervisor = Supervisor::start(cfg).expect("supervisor starts");

    let logs = wait_for_logs(
        &supervisor,
        |lines| {
            lines
                .iter()
                .any(|line| line.line == "CONSOLE_HANDLE_0" && line.stream == "out")
                && lines
                    .iter()
                    .any(|line| line.line == "ERR_OOPS" && line.stream == "err")
        },
        Duration::from_secs(10),
    );

    assert!(
        logs.iter()
            .any(|line| line.line == "CONSOLE_HANDLE_0" && line.stream == "out"),
        "Windows child inherited or created a console: {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|line| line.line == "ERR_OOPS" && line.stream == "err"),
        "stderr was not captured while CREATE_NO_WINDOW was active: {logs:?}"
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
