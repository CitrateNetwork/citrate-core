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
fn a_line_on_disk_keeps_addresses_tags_64_hex_and_never_keeps_a_key_token_or_email() {
    let line = format!("imported {BLOCK_HASH} from {ADDR} key={} auth: eyJhbGciOiJIUzI1NiJ9.payload.sig mail=member@example.com", vendor_shape());
    let out = disk_filter(&line, LogPolicy::Full).expect("a full-policy line is kept");
    // #613 review: a 64-hex value is private-key shaped, so even a block hash becomes a short tag.
    assert!(!out.contains(&BLOCK_HASH[2..]), "the filtered line is not as expected");
    assert!(out.contains("<hex64 "), "the filtered line is not as expected");
    assert!(out.contains(ADDR), "the filtered line is not as expected");
    assert!(!out.contains("PLANTED"), "the filtered line is not as expected");
    assert!(!out.contains("eyJhbGci"), "the filtered line is not as expected");
    assert!(!out.contains("member@example.com"), "the filtered line is not as expected");
    assert_eq!(
        disk_filter("Validated and imported 32/32 blocks", LogPolicy::Full).as_deref(),
        Some("Validated and imported 32/32 blocks")
    );
}

#[test]
fn each_sidecar_gets_its_own_bounded_file_under_the_log_folder() {
    let dir = Path::new("/var/log/ai.citrate.core/sidecars");
    let c = config_in(dir, "citrate-node", LogPolicy::Full);
    assert_eq!(c.path, dir.join("citrate-node.log"));
    assert_eq!(c.max_bytes, MAX_LOG_BYTES);
    assert_eq!(c.keep, KEEP_ROTATED);
    assert!(c.filter.as_ref().is_some_and(|f| f(&format!("k={}", vendor_shape())).is_some_and(|l| !l.contains("PLANTED"))));
    assert_eq!(c.queue, LOG_QUEUE_LINES);
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
    attach(&mut cfg, LogPolicy::Full);
    assert_eq!(
        cfg.log_file.as_ref().map(|c| c.path.clone()),
        log_dir().map(|d| d.join("d5-attach-probe.log")),
        "the log file follows the folder set at start-up (none before it)"
    );
    assert!(crash_files_seen()
        .iter()
        .any(|(n, p, pol)| n == "d5-attach-probe" && p == Path::new("/tmp/d5-attach-probe/crash.jsonl") && *pol == LogPolicy::Full));
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
        // #613 review (P2): sidecars whose output can carry prompts, memory results or message
        // previews keep only lifecycle and error lines.
        let content = ["src-tauri/src/hermes.rs", "src-tauri/src/memory.rs", "src-tauri/src/comms.rs", "src-tauri/src/serve.rs", "src-tauri/src/embed_serve.rs"];
        let want = if content.contains(&d.owner) { "LogPolicy::LifecycleOnly" } else { "LogPolicy::Full" };
        let call = format!("crate::sidecar_logs::attach(&mut config, crate::sidecar_logs::{want});");
        let attach_at = rest.find(&call).unwrap_or_else(|| panic!("{} must attach its log file with {want}", d.owner));
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
    assert!(lines[0].starts_with("13 out"), "the oldest kept line is wrong");
    assert!(lines[11].starts_with("24 out"), "the newest kept line is wrong");
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

fn inputs<'a>(dir: Option<&'a Path>, crash: Vec<(String, PathBuf, LogPolicy)>, panic: Option<PathBuf>) -> BundleInputs<'a> {
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

    let (text, sidecars, n) = build_bundle(&inputs(Some(&logs), vec![("node".into(), crash, LogPolicy::Full)], Some(panic))).unwrap();
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
        assert!(!text.contains(planted), "a planted value leaked into the bundle");
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
    assert!(!bundle_scrub(&format!("a {} b", hex64()), HOME).contains(&hex64()));
    assert!(!bundle_scrub(&format!("{}ff", hex64()), HOME).contains(&hex64()), "longer runs too");
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
        // #613 review (P2): the refusal is the platform-neutral pre-check (Windows has no O_NOFOLLOW).
        assert!(refuse_link(&link).unwrap_err().contains("link"));
        assert!(refuse_link(&dest).is_ok());
        assert!(refuse_link(&t.0.join("not-there.txt")).is_ok());
    }
}

// ---- #613 review fixes ----

#[test]
fn the_bundle_scrub_and_tripwire_cover_every_corpus_shape() {
    for (shape, line, secret) in crate::secret_shapes::test_corpus() {
        assert!(bundle_tripwire(&line).is_err(), "{shape}: the tripwire refuses it unredacted");
        let out = bundle_scrub(&line, HOME);
        assert!(!out.contains(&secret), "{shape}: the planted value survived");
        bundle_tripwire(&out).unwrap_or_else(|e| panic!("{shape}: the final check refused it ({})", e.len()));
    }
}

#[test]
fn a_bundle_with_every_planted_shape_carries_none_of_them() {
    let t = Tmp::new("corpus-bundle");
    let logs = t.0.join("sidecars");
    std::fs::create_dir_all(&logs).unwrap();
    let corpus = crate::secret_shapes::test_corpus();
    let body: String = corpus.iter().enumerate().map(|(i, (_, l, _))| format!("{i} out {l}\n")).collect();
    std::fs::write(logs.join("node.log"), body).unwrap();
    let (text, _, n) = build_bundle(&inputs(Some(&logs), vec![], None)).unwrap();
    assert_eq!(n, corpus.len());
    for (shape, _, secret) in &corpus {
        assert!(!text.contains(secret.as_str()), "{shape} survived into the bundle");
    }
}

#[test]
fn the_bundle_says_what_the_scrub_does_and_no_more() {
    let (text, _, _) = build_bundle(&inputs(None, vec![], None)).unwrap();
    assert!(!text.contains("holds no wallet, key or seed"), "a claim the code does not establish");
    assert!(text.contains("best-effort"), "the bundle text is not as expected");
    assert!(text.contains("Read this file before you share it"), "the bundle text is not as expected");
}

#[test]
fn content_bearing_sidecars_write_only_allowlisted_lifecycle_lines() {
    let f = |l: &str| disk_filter(l, LogPolicy::LifecycleOnly);
    // Allowlisted shapes: every word is a lifecycle word or technical (a number, address, path,
    // version, key=value with no prose).
    for keep in [
        "server listening on 127.0.0.1:18090",
        "INFO hermes-agent version 0.3.1 starting",
        "main: model loaded in 1520 ms",
        "shutting down",
        "embed server ready on port 18091",
    ] {
        assert_eq!(f(keep).as_deref(), Some(keep), "an allowlisted lifecycle line was not kept as it is");
    }
    // Free text, tracebacks, Python dicts and JSON bodies never reach the disk.
    for body in [
        "the member asked about their savings plan",
        "starting conversation about my savings plan",
        "{\"role\":\"user\",\"content\":\"my private note\"}",
        "{'prompt': 'my private note', 'temperature': 0.2}",
        "memory.search result: dentist appointment friday",
        "preview: hi, are we still on for tonight?",
        "  File \"/app/agent.py\", line 12, in respond",
        "    reply = handle(note=\"my private note\")",
    ] {
        let out = f(body);
        assert!(out.as_deref().is_none_or(|o| !o.contains("private") && !o.contains("savings") && !o.contains("dentist") && !o.contains("tonight")), "content reached the disk");
    }
    // An error, warning or traceback is noted with no text from it.
    for (line, level) in [
        ("error while handling {\"prompt\": \"my private note\"}", "error"),
        ("WARNING: slow tool call for the note about my savings", "warning"),
        ("Traceback (most recent call last):", "traceback"),
        ("panic: index out of range for the user's note", "error"),
    ] {
        let out = f(line).expect("an error-level line leaves a marker");
        assert!(out.starts_with(&format!("[withheld: {level} line")), "the marker names the level");
        assert!(!out.contains("note") && !out.contains("prompt"), "the marker carries no text from the line");
    }
    let pw = ["hunter2", "hunter2"].concat();
    let out = f(&format!("error: login failed password={pw}")).expect("a marker");
    assert!(!out.contains(&pw));
    // The full policy keeps prose.
    assert!(disk_filter("the member asked about their savings plan", LogPolicy::Full).is_some());
}

#[test]
fn crash_records_of_content_bearing_sidecars_drop_their_stderr_tail() {
    let t = Tmp::new("crash-content");
    let crash = t.0.join("crashes.log");
    std::fs::write(
        &crash,
        "{\"name\":\"hermes\",\"at_unix_ms\":5,\"exit\":\"exit status: 1\",\"stderr_tail\":\"user said: my private note\",\"restart_attempt\":1}\nnot json at all: my private note\n",
    )
    .unwrap();
    let (text, _, _) = build_bundle(&inputs(None, vec![("hermes".into(), crash, LogPolicy::LifecycleOnly)], None)).unwrap();
    assert!(text.contains("== hermes crash records =="));
    assert!(text.contains("exit status: 1"));
    assert!(!text.contains("my private note"), "the bundle text is not as expected");
    assert!(text.contains("stderr tail withheld"));
}

#[test]
fn the_home_folder_falls_back_to_userprofile() {
    let env = |vars: &'static [(&'static str, &'static str)]| move |k: &str| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert_eq!(home_from(env(&[("HOME", "/Users/m"), ("USERPROFILE", "C:\\Users\\m")])), "/Users/m");
    assert_eq!(home_from(env(&[("USERPROFILE", "C:\\Users\\m")])), "C:\\Users\\m");
    assert_eq!(home_from(env(&[("HOME", "")])), "");
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
    let mut lf = config_in(&t.0.join("sidecars"), "d5-health", LogPolicy::Full);
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
