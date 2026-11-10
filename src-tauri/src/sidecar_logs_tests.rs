// CORE-D5 — sidecar log files and the diagnostics bundle. Pure: temp folders and fixture lines.
use super::*;
use citrate_core_kit::sidecar_log::rotated_path;

const HOME: &str = "/Users/member";
/// A planted vendor-key shape (not a credential; built so no scanner reads it as one).
fn vendor_shape() -> String {
    ["sk-ant-", "PLANTED", "PLANTED", "PLANTED0123"].concat()
}
/// A planted 64-hex run printed without 0x (the shape of a raw private key; synthetic).
fn hex64() -> String {
    "5e".repeat(32)
}
const BLOCK_HASH: &str = "0x9f2c6a1be0e7f1a0b4d7c2e3f4a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d";
const ADDR: &str = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let n: u64 = rand::random();
        let p = std::env::temp_dir().join(format!("citrate-sidecar-logs-{tag}-{n:016x}"));
        std::fs::create_dir_all(&p).expect("tmp");
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_line_on_disk_keeps_hashes_and_addresses_but_never_a_key_or_email() {
    let line = format!("imported {BLOCK_HASH} from {ADDR} key={} auth: eyJhbGciOiJIUzI1NiJ9.payload.sig mail=member@example.com", vendor_shape());
    let out = disk_filter(&line);
    assert!(out.contains(BLOCK_HASH), "{out}");
    assert!(out.contains(ADDR), "{out}");
    assert!(!out.contains("PLANTED"), "{out}");
    assert!(!out.contains("eyJhbGci"), "{out}");
    assert!(!out.contains("member@example.com"), "{out}");
    assert_eq!(disk_filter("Validated and imported 32/32 blocks"), "Validated and imported 32/32 blocks");
}

#[test]
fn each_sidecar_gets_its_own_bounded_file_under_the_log_folder() {
    let dir = Path::new("/var/log/ai.citrate.core/sidecars");
    let c = config_in(dir, "citrate-node");
    assert_eq!(c.path, dir.join("citrate-node.log"));
    assert_eq!(c.max_bytes, MAX_LOG_BYTES);
    assert_eq!(c.keep, KEEP_ROTATED);
    assert!(c.filter.as_ref().is_some_and(|f| !f(&format!("k={}", vendor_shape())).contains("PLANTED")));
    assert_eq!(file_stem("llama/serve chat"), "llama-serve-chat");
    assert_eq!(file_stem("../x"), "---x");
    assert_eq!(file_stem(""), "sidecar");
    // Nine sidecars at the bound: 4 files each.
    assert!(MAX_LOG_BYTES * (KEEP_ROTATED as u64 + 1) * 9 <= 160 * 1024 * 1024);
}

#[test]
fn attach_sets_the_log_file_and_records_the_owners_crash_file() {
    let spec = citrate_core_kit::supervisor::SidecarSpec::new("d5-attach-probe", "/bin/true", vec![]);
    let mut cfg = citrate_core_kit::supervisor::SupervisorConfig::new(spec, "/tmp/d5-attach-probe/crash.jsonl");
    attach(&mut cfg);
    assert_eq!(
        cfg.log_file.as_ref().map(|c| c.path.clone()),
        log_dir().map(|d| d.join("d5-attach-probe.log")),
        "the log file follows the folder set at start-up (none before it)"
    );
    assert!(crash_files_seen()
        .iter()
        .any(|(n, p)| n == "d5-attach-probe" && p == Path::new("/tmp/d5-attach-probe/crash.jsonl")));
}

/// Tripwire (CORE-D5): every registry owner attaches the log file before it starts its supervisor.
#[test]
fn every_registry_owner_keeps_its_sidecar_output_on_disk() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for d in &crate::services::REGISTRY {
        let file = dir.join(d.owner.trim_start_matches("src-tauri/src/"));
        let src = std::fs::read_to_string(&file).unwrap_or_default();
        let new_at = src.find("SupervisorConfig::new(").unwrap_or_else(|| panic!("{} builds a supervisor config", d.owner));
        let rest = &src[new_at..];
        let attach_at = rest.find("crate::sidecar_logs::attach(&mut config);").unwrap_or_else(|| panic!("{} must attach its log file", d.owner));
        let start_at = rest.find("Supervisor::start(").unwrap_or_else(|| panic!("{} starts its supervisor", d.owner));
        assert!(attach_at < start_at, "{} attaches the log file before Supervisor::start", d.owner);
    }
}

fn write_lines(path: &Path, from: usize, to: usize, extra: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut s = String::new();
    for i in from..to {
        s.push_str(&format!("{i} out line {i}{extra}\n"));
    }
    std::fs::write(path, s).unwrap();
}

#[test]
fn the_newest_lines_are_read_across_rotated_files_in_order() {
    let t = Tmp::new("tail");
    let path = t.0.join("node.log");
    write_lines(&rotated_path(&path, 2), 0, 10, "");
    write_lines(&rotated_path(&path, 1), 10, 20, "");
    write_lines(&path, 20, 25, "");
    let lines = tail_of_logs(&path, 3, 12, 1 << 20);
    assert_eq!(lines.len(), 12);
    assert!(lines[0].starts_with("13 out"), "{lines:?}");
    assert!(lines[11].starts_with("24 out"), "{lines:?}");
    // A byte budget smaller than one file keeps only whole newest lines.
    let few = tail_of_logs(&path, 3, 100, 40);
    assert!(!few.is_empty() && few.last().unwrap().starts_with("24 out"), "{few:?}");
    assert!(few.iter().all(|l| l.contains(" out line ")), "no partial first line: {few:?}");
}

#[test]
fn only_this_apps_log_names_are_listed() {
    let t = Tmp::new("names");
    for n in ["node.log", "node.log.1", "hermes.log", "Weird Name.log", "notes.txt"] {
        std::fs::write(t.0.join(n), "x").unwrap();
    }
    assert_eq!(log_names(&t.0), vec!["hermes".to_string(), "node".to_string()]);
}

fn inputs<'a>(dir: Option<&'a Path>, crash: Vec<(String, PathBuf)>, panic: Option<PathBuf>) -> BundleInputs<'a> {
    BundleInputs {
        app_version: "0.5.3",
        os: "macos",
        made_at_unix_ms: 1_700_000_000_000,
        health_json: format!("{{\"services\":[{{\"id\":\"node\",\"phase\":\"alive\",\"logFile\":{{\"path\":\"{HOME}/Library/Logs/ai.citrate.core/sidecars/node.log\"}}}}]}}"),
        log_dir: dir,
        crash_files: crash,
        panic_file: panic,
        home: HOME,
    }
}

#[test]
fn the_bundle_holds_health_logs_crashes_and_panics_and_no_secret() {
    let t = Tmp::new("bundle");
    let logs = t.0.join("sidecars");
    let planted = format!(" k={} pk={} at {HOME}/projects mail=a@b.co {ADDR}", vendor_shape(), hex64());
    write_lines(&rotated_path(&logs.join("node.log"), 1), 0, 3, &planted);
    write_lines(&logs.join("node.log"), 3, 6, "");
    write_lines(&logs.join("hermes.log"), 0, 2, &format!(" gw {}", ["cgk_", "PLANTEDGATEWAY", "SHAPE99"].concat()));
    let crash = t.0.join("crash-records.jsonl");
    std::fs::write(&crash, format!("{{\"name\":\"citrate-node\",\"exit\":\"exit code: 1\",\"stderr_tail\":\"{HOME}/x {}\"}}\n", hex64())).unwrap();
    let panic = t.0.join("last-panic.log");
    std::fs::write(&panic, "panic at src/node.rs:10: boom\n").unwrap();

    let (text, sidecars, n) = build_bundle(&inputs(Some(&logs), vec![("node".into(), crash)], Some(panic))).unwrap();
    assert_eq!(sidecars, vec!["hermes".to_string(), "node".to_string()]);
    assert_eq!(n, 8);
    assert!(text.contains("app 0.5.3 on macos"));
    assert!(text.contains("== Sidecar health (sidecars_health) =="));
    assert!(text.contains("== node log (newest 6 lines) =="));
    assert!(text.contains("0 out line 0"), "rotated lines are included");
    assert!(text.contains("5 out line 5"));
    assert!(text.contains("== node crash records =="));
    assert!(text.contains("exit code: 1"));
    assert!(text.contains("panic at src/node.rs:10: boom"));
    let hex = hex64();
    for planted in ["PLANTED", hex.as_str(), HOME, "a@b.co", &ADDR[2..]] {
        assert!(!text.contains(planted), "{planted} leaked into the bundle");
    }
    assert!(text.contains("~/projects"), "the home folder becomes ~");
    bundle_tripwire(&text).expect("a finished bundle passes the key tripwire");
}

#[test]
fn the_key_tripwire_refuses_anything_key_shaped() {
    assert!(bundle_tripwire("fine: block 42 imported").is_ok());
    assert!(bundle_tripwire(&format!("x {}", vendor_shape())).is_err());
    assert!(bundle_tripwire("-----BEGIN PRIVATE KEY-----").is_err());
    assert!(bundle_tripwire(&format!("pk {}", hex64())).is_err());
    // Shorter hex (a peer id fragment, a 40-hex value behind 0x was already scrubbed) is fine.
    assert!(bundle_tripwire("peer 12D3KooW deadbeefcafe").is_ok());
    assert_eq!(bundle_scrub(&format!("a {} b", hex64()), HOME), "a <redacted> b");
    assert_eq!(bundle_scrub(&format!("{}ff", hex64()), HOME), "<redacted>", "longer runs too");
}

#[test]
fn a_bundle_without_a_log_folder_says_so() {
    let (text, sidecars, n) = build_bundle(&inputs(None, vec![], None)).unwrap();
    assert!(sidecars.is_empty());
    assert_eq!(n, 0);
    assert!(text.contains("The log folder is not set up in this session."));
}

#[test]
fn the_bundle_is_written_only_to_an_absolute_txt_path_owner_only() {
    assert!(check_export_path(Path::new("relative/bundle.txt")).is_err());
    assert!(check_export_path(Path::new("/tmp/bundle.json")).is_err());
    assert!(check_export_path(Path::new("/tmp/bundle.TXT")).is_ok());
    let t = Tmp::new("write");
    let dest = t.0.join("citrate-diagnostics.txt");
    write_bundle(&dest, "hello\n").unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello\n");
    write_bundle(&dest, "again\n").unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "again\n", "a replaced bundle holds only the new text");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o600);
        let target = t.0.join("other.txt");
        std::fs::write(&target, "keep").unwrap();
        let link = t.0.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(write_bundle(&link, "x").is_err(), "a link at the path is refused");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    }
}

#[test]
fn the_health_row_reports_the_log_file_with_the_home_folder_scrubbed() {
    let t = Tmp::new("health");
    let spec = citrate_core_kit::supervisor::SidecarSpec::new("d5-health", t.0.join("no-such-binary"), vec![]);
    let mut cfg = citrate_core_kit::supervisor::SupervisorConfig::new(spec, t.0.join("crash.jsonl"));
    cfg.backoff = citrate_core_kit::supervisor::BackoffPolicy {
        base_delay: std::time::Duration::from_millis(5),
        multiplier: 1,
        max_delay: std::time::Duration::from_millis(5),
        max_retries: 0,
    };
    let mut lf = config_in(&t.0.join("sidecars"), "d5-health");
    lf.path = PathBuf::from(format!("{HOME}/Library/Logs/ai.citrate.core/sidecars/d5-health.log"));
    cfg.log_file = Some(lf);
    let sup = citrate_core_kit::supervisor::Supervisor::start(cfg).expect("start");
    let slot = std::sync::Mutex::new(Some(sup));
    let desc = crate::services::REGISTRY.iter().find(|d| d.id == crate::services::ServiceId::Node).unwrap();
    // A read can find the cell busy for a moment; it then claims nothing, so read until it answers.
    let mut h = crate::services::health_entry(desc, crate::services::SlotRead::Slot(&slot), 0, HOME);
    for _ in 0..200 {
        if h.counters.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        h = crate::services::health_entry(desc, crate::services::SlotRead::Slot(&slot), 0, HOME);
    }
    let f = h.log_file.expect("the supervisor keeps a log file");
    assert_eq!(f.path, "~/Library/Logs/ai.citrate.core/sidecars/d5-health.log");
    assert_eq!(f.write_failures, 0);
    let v = serde_json::to_value(crate::services::health_entry(desc, crate::services::SlotRead::NoOwner, 0, HOME)).unwrap();
    assert!(v["logFile"].is_null(), "no supervisor, no log file claimed");
    let held = slot.lock().unwrap().take();
    if let Some(s) = held {
        s.stop();
    }
}

#[test]
fn the_export_command_names_its_data_sources_and_is_registered() {
    let src = include_str!("sidecar_logs.rs");
    let at = src.find("pub async fn diagnostics_logs_export(").expect("defined");
    let doc = &src[src[..at].rfind("/// **diagnostics_logs_export**").expect("documented")..at];
    assert!(doc.contains("Data sources"), "Rule 7");
    assert!(doc.contains("nothing is sent"));
    assert!(include_str!("lib.rs").contains("sidecar_logs::diagnostics_logs_export,"));
    assert!(include_str!("../permissions/main-window.toml").contains("\"diagnostics_logs_export\""));
}
