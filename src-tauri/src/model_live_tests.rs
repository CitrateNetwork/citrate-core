// Live model-download harness. IGNORED by default: it pulls real bytes over the network.
//
// It drives the SAME code path the `model_download` command runs (`download_with` over real
// `UreqModelTransport`s with the production settings, inside `download_and_verify_with_clean_retry`),
// against real URLs or a local test server, and prints timestamped progress read from the `.part`. It is the scripted
// manual verification for the onboarding model download (see scripts/model-download/).
//
// Environment:
// - `MDL_DIR`   (required) the model directory to download into.
// - `MDL_URLS`  comma-separated source URLs, in order (default: the app's own source list).
// - `MDL_SIZE`  pinned byte length (default: MODEL_SIZE_BYTES).
// - `MDL_SHA`   pinned SHA-256 hex (default: MODEL_SHA256).
//
// Run: `cargo test --locked -p citrate-core --lib model::live_tests -- --ignored --nocapture`

use super::*;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

#[test]
#[ignore = "live network: pulls the real model (scripts/model-download/)"]
fn live_download_and_verify() {
    let dir = PathBuf::from(std::env::var("MDL_DIR").expect("MDL_DIR is required"));
    let size: u64 = env_or("MDL_SIZE", &MODEL_SIZE_BYTES.to_string())
        .parse()
        .expect("MDL_SIZE is a number");
    let sha = env_or("MDL_SHA", MODEL_SHA256);
    let file = MODEL_FILE.to_string();
    let urls: Vec<String> = match std::env::var("MDL_URLS") {
        Ok(v) if !v.trim().is_empty() => v.split(',').map(|s| s.trim().to_string()).collect(),
        _ => model_source_urls(None),
    };
    eprintln!("LIVE dir={} file={file} size={size} sources={urls:?}", dir.display());

    let part = dir.join(format!("{file}.part"));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done2 = done.clone();
    let start = std::time::Instant::now();
    let part2 = part.clone();
    let ticker = std::thread::spawn(move || {
        let mut last = 0u64;
        let mut last_t = std::time::Instant::now();
        while !done2.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let have = std::fs::metadata(&part2).map(|m| m.len()).unwrap_or(0);
            let dt = last_t.elapsed().as_secs_f64().max(0.001);
            let rate = (have.saturating_sub(last)) as f64 / dt / 1e6;
            eprintln!(
                "LIVE t={:>6.0}s part={have} ({:.1}%) rate={rate:.2}MB/s",
                start.elapsed().as_secs_f64(),
                have as f64 / size.max(1) as f64 * 100.0
            );
            last = have;
            last_t = std::time::Instant::now();
        }
    });

    // The production settings `model_download` runs with (ModelDownloadOptions::for_pin), over the
    // pinned length/hash given here, through the same clean-retry-on-mismatch wrapper.
    let mut round = 0usize;
    let res = download_and_verify_with_clean_retry(&dir, &file, &sha, size, || {
        let mut order = urls.clone();
        let k = round % order.len().max(1);
        order.rotate_left(k);
        round += 1;
        let opts = ModelDownloadOptions {
            file: file.clone(),
            expected_hash: sha.clone(),
            expected_size: size,
            ..ModelDownloadOptions::for_pin(&DEFAULT_PIN, order.clone())
        };
        let sources: Vec<Box<dyn ModelTransport>> = order
            .iter()
            .map(|u| Box::new(UreqModelTransport::new(u.clone()).with_expected_total(size)) as Box<dyn ModelTransport>)
            .collect();
        download_with(&dir, sources, &opts)
    });
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = ticker.join();
    match &res {
        Ok(()) => eprintln!(
            "LIVE RESULT ok (downloaded and verified) after {:.0}s status={:?}",
            start.elapsed().as_secs_f64(),
            ModelManager::new(dir.clone(), Box::new(UreqModelTransport::new(String::new())), sha.clone(), size).status()
        ),
        Err(e) => eprintln!(
            "LIVE RESULT err after {:.0}s: {e} | shown to the member: {}",
            start.elapsed().as_secs_f64(),
            e.user_message()
        ),
    }
}
