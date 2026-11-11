// CORE-D5 — the rotating on-disk log of one sidecar. Pure: temp folders only.
use super::*;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-sidecar-log-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        Tmp(p)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(dir: &Path, max_bytes: u64, keep: usize) -> LogFileConfig {
    LogFileConfig {
        path: dir.join("sidecars").join("node.log"),
        max_bytes,
        keep,
        filter: None,
        queue: 16,
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn each_line_is_appended_with_its_time_and_stream() {
    let t = Tmp::new("append");
    let mut log = RotatingLog::new(config(&t.0, 1 << 20, 2));
    log.append(1_700, "out", "imported 32 blocks");
    log.append(1_701, "err", "peer dropped");
    let path = t.0.join("sidecars/node.log");
    assert_eq!(read(&path), "1700 out imported 32 blocks\n1701 err peer dropped\n");
    assert_eq!(log.status().write_failures, 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600, "the log is owner-only");
        assert_eq!(mode(&t.0.join("sidecars")), 0o700, "the folder is owner-only");
    }
}

#[test]
fn a_restarted_log_appends_to_the_existing_file() {
    let t = Tmp::new("reopen");
    RotatingLog::new(config(&t.0, 1 << 20, 2)).append(1, "out", "first run");
    RotatingLog::new(config(&t.0, 1 << 20, 2)).append(2, "out", "second run");
    assert_eq!(read(&t.0.join("sidecars/node.log")), "1 out first run\n2 out second run\n");
}

#[test]
fn the_file_rotates_at_the_bound_and_keeps_only_keep_files() {
    let t = Tmp::new("rotate");
    let max = MIN_MAX_BYTES;
    let mut log = RotatingLog::new(config(&t.0, max, 2));
    // Each line is 100 bytes on disk ("<ts> out " + payload + "\n").
    let payload = "x".repeat(100 - "1000000 out \n".len());
    let lines_per_file = (max / 100) as usize;
    let total = lines_per_file * 5;
    for i in 0..total {
        log.append(1_000_000 + i as u64, "out", &payload);
    }
    let path = t.0.join("sidecars/node.log");
    let files = existing_files(&path, 2);
    assert_eq!(files, vec![rotated_path(&path, 2), rotated_path(&path, 1), path.clone()]);
    assert!(!rotated_path(&path, 3).exists(), "never more than `keep` rotated files");
    for f in &files {
        let len = std::fs::metadata(f).unwrap().len();
        assert!(len <= max, "{} is {len} bytes, over the bound {max}", f.display());
    }
    // The newest line is in the current file, and lines run in order across the files.
    let all: Vec<u64> = files
        .iter()
        .flat_map(|f| read(f).lines().map(|l| l.split(' ').next().unwrap().parse::<u64>().unwrap()).collect::<Vec<_>>())
        .collect();
    assert_eq!(*all.last().unwrap(), 1_000_000 + total as u64 - 1);
    assert!(all.windows(2).all(|w| w[0] + 1 == w[1]), "the kept lines are contiguous and in order");
    assert_eq!(all.len(), lines_per_file * 3, "the oldest file was dropped");
}

#[test]
fn keep_zero_starts_over_in_one_file() {
    let t = Tmp::new("keep0");
    let mut log = RotatingLog::new(config(&t.0, MIN_MAX_BYTES, 0));
    let payload = "y".repeat(1000);
    for i in 0..20 {
        log.append(i, "err", &payload);
    }
    let path = t.0.join("sidecars/node.log");
    assert_eq!(existing_files(&path, 3), vec![path.clone()]);
    assert!(std::fs::metadata(&path).unwrap().len() <= MIN_MAX_BYTES);
}

#[test]
fn a_tiny_bound_is_raised_to_the_minimum() {
    let t = Tmp::new("tiny");
    let mut log = RotatingLog::new(config(&t.0, 1, 1));
    log.append(1, "out", "a");
    log.append(2, "out", "b");
    let path = t.0.join("sidecars/node.log");
    assert_eq!(read(&path), "1 out a\n2 out b\n", "no rotation per line under a 1-byte bound");
}

#[test]
fn the_filter_runs_before_the_disk() {
    let t = Tmp::new("filter");
    let mut c = config(&t.0, 1 << 20, 1);
    c.filter = Some(Arc::new(|l: &str| Some(l.replace("sk-SECRETSECRETSECRET", "<redacted>"))));
    let mut log = RotatingLog::new(c);
    log.append(5, "out", "key=sk-SECRETSECRETSECRET ok");
    let text = read(&t.0.join("sidecars/node.log"));
    assert_eq!(text, "5 out key=<redacted> ok\n");
    assert!(!text.contains("SECRET"));
}

#[test]
fn a_long_line_is_cut_at_a_character_boundary() {
    let t = Tmp::new("long");
    let mut log = RotatingLog::new(config(&t.0, 1 << 22, 1));
    let line = "é".repeat(MAX_LINE_BYTES); // 2 bytes each: twice the bound
    log.append(9, "out", &line);
    let text = read(&t.0.join("sidecars/node.log"));
    assert!(text.ends_with(" [line cut]\n"), "the cut is marked");
    assert!(text.len() < MAX_LINE_BYTES + 64);
    assert!(std::str::from_utf8(text.as_bytes()).is_ok());
}

#[test]
fn a_write_failure_is_counted_and_capture_goes_on() {
    let t = Tmp::new("fail");
    std::fs::create_dir_all(&t.0).unwrap();
    // The log's folder name is taken by a file, so the folder cannot be created.
    std::fs::write(t.0.join("sidecars"), "not a folder").unwrap();
    let mut log = RotatingLog::new(config(&t.0, 1 << 20, 1));
    log.append(1, "out", "lost");
    log.append(2, "out", "lost too");
    let st = log.status();
    assert_eq!(st.write_failures, 2);
    assert!(st.last_error.is_some());
    // Once the folder can be made, the next line lands.
    std::fs::remove_file(t.0.join("sidecars")).unwrap();
    log.append(3, "out", "kept");
    assert_eq!(read(&t.0.join("sidecars/node.log")), "3 out kept\n");
    assert_eq!(log.status().write_failures, 2);
}

#[cfg(unix)]
#[test]
fn a_link_at_the_log_name_is_never_followed() {
    let t = Tmp::new("link");
    std::fs::create_dir_all(t.0.join("sidecars")).unwrap();
    let target = t.0.join("elsewhere.txt");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, t.0.join("sidecars/node.log")).unwrap();
    let mut log = RotatingLog::new(config(&t.0, 1 << 20, 1));
    log.append(1, "out", "must not land in the link target");
    assert_eq!(read(&target), "untouched");
    assert_eq!(log.status().write_failures, 1);
}

#[test]
fn existing_files_lists_oldest_first_and_ignores_missing() {
    let t = Tmp::new("list");
    std::fs::create_dir_all(t.0.join("sidecars")).unwrap();
    let path = t.0.join("sidecars/node.log");
    std::fs::write(rotated_path(&path, 2), "b").unwrap();
    std::fs::write(&path, "c").unwrap();
    assert_eq!(existing_files(&path, 3), vec![rotated_path(&path, 2), path.clone()]);
    assert_eq!(rotated_path(&path, 1).file_name().unwrap(), "node.log.1");
}

#[test]
fn a_line_the_filter_withholds_is_counted_and_never_written() {
    let t = Tmp::new("withhold");
    let mut c = config(&t.0, 1 << 20, 1);
    c.filter = Some(Arc::new(|l: &str| (!l.contains("prompt")).then(|| l.to_string())));
    let mut log = RotatingLog::new(c);
    log.append(1, "out", "listening on 127.0.0.1:18090");
    log.append(2, "out", "prompt: the member's private question");
    log.append(3, "err", "error: slot 0 failed");
    let text = read(&t.0.join("sidecars/node.log"));
    assert_eq!(text, "1 out listening on 127.0.0.1:18090\n3 err error: slot 0 failed\n");
    let st = log.status();
    assert_eq!(st.lines_withheld, 1);
    assert_eq!(st.write_failures, 0);
}

#[cfg(unix)]
#[test]
fn a_fifo_at_the_log_name_fails_at_once_instead_of_blocking() {
    let t = Tmp::new("fifo");
    std::fs::create_dir_all(t.0.join("sidecars")).unwrap();
    let fifo = t.0.join("sidecars/node.log");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("INCONCLUSIVE: mkfifo is not available here");
        return;
    }
    let cfg = config(&t.0, 1 << 20, 1);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut log = RotatingLog::new(cfg);
        log.append(1, "out", "x");
        let _ = tx.send(log.status());
    });
    let st = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the append returned instead of blocking on a FIFO with no reader");
    assert_eq!(st.write_failures, 1);
}
