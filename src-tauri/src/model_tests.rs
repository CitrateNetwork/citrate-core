// BC-3.1 — model.rs (download + verify) tests. RED-FIRST protocol.
//
// CI-safe: NO 5 GB Gemma pull, NO network. An injectable HTTP transport
// (mirroring rpc.rs's RpcTransport / staking.rs's style) drives a SMALL fixture
// stream, so the checksum-verify logic, the HTTP-Range resume, the GGUF magic
// check, and the tamper-reject are all proven WITHOUT the real bytes. The real
// 5 GB download + real llama-server inference are a documented manual/integration
// proof (see the SCOPE.md honest-gaps section), not faked here.
//
// The fixture uses a TEST-LOCAL expected hash (the sha256 of the fixture bytes),
// NOT the real Gemma hash — we prove the verify LOGIC bites, not that we shipped
// the real model. The real MODEL_SHA256 const is asserted for its shape/pinning
// separately.

use super::*;
use std::sync::Mutex as StdMutex;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// A minimal well-formed fixture body: the 4-byte GGUF magic followed by some
/// payload. `len` is the total body length (>= 4).
fn gguf_fixture(len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len.max(4));
    v.extend_from_slice(&GGUF_MAGIC);
    // Deterministic filler so the sha256 is stable across runs.
    let mut b: u8 = 0;
    while v.len() < len {
        v.push(b);
        b = b.wrapping_add(7);
    }
    v.truncate(len.max(4));
    v
}

/// Compute the sha256 hex of a byte slice (the TEST-LOCAL expected hash).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// A unique temp dir for a test's model dir.
fn tmp_model_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!(
        "citrate-core-model-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("mk model tmpdir");
    p
}

// ---------------------------------------------------------------------------
// A fixture HTTP transport (injectable, mirrors rpc.rs::RpcTransport style).
// It serves `body` and honours an HTTP Range `offset` so the resume path is
// exercised. It records the requested ranges so a test can prove a resume
// continued (offset > 0) rather than restarted.
// ---------------------------------------------------------------------------

struct FixtureTransport {
    body: Vec<u8>,
    /// If Some, the transport truncates its response to this many bytes AFTER the
    /// range offset (to simulate a short/failed stream mid-download).
    truncate_after: Option<usize>,
    /// Records every (offset) a GET requested, so a test can assert resume.
    requested_offsets: StdMutex<Vec<u64>>,
}

impl FixtureTransport {
    fn new(body: Vec<u8>) -> Self {
        FixtureTransport {
            body,
            truncate_after: None,
            requested_offsets: StdMutex::new(Vec::new()),
        }
    }
    fn short(body: Vec<u8>, truncate_after: usize) -> Self {
        FixtureTransport {
            body,
            truncate_after: Some(truncate_after),
            requested_offsets: StdMutex::new(Vec::new()),
        }
    }
    fn offsets(&self) -> Vec<u64> {
        self.requested_offsets.lock().unwrap().clone()
    }
}

impl ModelTransport for FixtureTransport {
    fn total_size(&self) -> std::result::Result<u64, ModelError> {
        Ok(self.body.len() as u64)
    }

    fn get_from(&self, offset: u64) -> std::result::Result<Box<dyn std::io::Read + Send>, ModelError> {
        self.requested_offsets.lock().unwrap().push(offset);
        let start = offset as usize;
        if start > self.body.len() {
            return Err(ModelError::Transport("range past end".into()));
        }
        let mut chunk = self.body[start..].to_vec();
        if let Some(t) = self.truncate_after {
            chunk.truncate(t);
        }
        Ok(Box::new(std::io::Cursor::new(chunk)))
    }
}

/// Build a manager over a fixture transport + temp dir + a TEST-LOCAL expected
/// hash/size (so the verify logic is proven without the real Gemma bytes).
fn fixture_manager(
    tag: &str,
    body: Vec<u8>,
    transport: FixtureTransport,
) -> (ModelManager, PathBuf) {
    let dir = tmp_model_dir(tag);
    let expected_hash = sha256_hex(&body);
    let expected_size = body.len() as u64;
    let mgr = ModelManager::new(
        dir.clone(),
        Box::new(transport),
        expected_hash,
        expected_size,
    );
    (mgr, dir)
}

// ---------------------------------------------------------------------------
// (0) Pinned consts — the grounded Gemma facts must not drift (Rule 1 pinning).
// ---------------------------------------------------------------------------

#[test]
fn pinned_model_consts_match_grounded_facts() {
    assert_eq!(MODEL_FILE, "gemma-4-E4B-it-Q4_K_M.gguf");
    assert_eq!(MODEL_SIZE_BYTES, 5_335_289_824);
    assert_eq!(
        MODEL_SHA256,
        "90ce98129eb3e8cc57e62433d500c97c624b1e3af1fcc85dd3b55ad7e0313e9f"
    );
    // A 64-hex-char (32-byte) lowercase sha256.
    assert_eq!(MODEL_SHA256.len(), 64);
    assert!(MODEL_SHA256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    // GGUF magic "GGUF".
    assert_eq!(&GGUF_MAGIC, b"GGUF");
    // The default source URL points at the grounded HF resolve path.
    assert!(DEFAULT_MODEL_URL.starts_with("https://"));
    assert!(DEFAULT_MODEL_URL.ends_with(MODEL_FILE));
}

/// The URL is overridable via CITRATE_MODEL_URL (a config seam), else defaults.
#[test]
fn model_url_is_env_overridable() {
    // Default when unset (we don't mutate the process env here to avoid races;
    // resolve_model_url takes the value directly for determinism).
    assert_eq!(resolve_model_url(None), DEFAULT_MODEL_URL);
    assert_eq!(
        resolve_model_url(Some("https://cdn.citrate.ai/gemma.gguf".to_string())),
        "https://cdn.citrate.ai/gemma.gguf"
    );
}

// ---------------------------------------------------------------------------
// (a) fixture whose sha256 == the (test-pinned) hash → download + verify Ready.
// ---------------------------------------------------------------------------

#[test]
fn full_download_then_verify_reaches_ready() {
    let body = gguf_fixture(4096);
    let transport = FixtureTransport::new(body.clone());
    let (mgr, dir) = fixture_manager("ready", body.clone(), transport);

    // Not present before any download.
    assert!(matches!(mgr.status(), ModelStatus::NotPresent));

    mgr.download().expect("streamed download completes");
    // After download the final file exists at the model path and is the full body.
    let final_path = dir.join(MODEL_FILE);
    assert!(final_path.exists(), "final model file must exist after download");
    assert_eq!(std::fs::read(&final_path).unwrap(), body);
    // The .part file was renamed away (no leftover partial).
    assert!(!dir.join(format!("{MODEL_FILE}.part")).exists());

    // Verify streams the file through sha256 and matches → Ready.
    mgr.verify().expect("verify matches the pinned hash");
    assert!(matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (b) flip ONE byte → verify REJECTS and NEVER marks Ready (tamper-reject).
// ---------------------------------------------------------------------------

#[test]
fn tampered_file_is_rejected_and_never_ready() {
    let body = gguf_fixture(4096);
    let transport = FixtureTransport::new(body.clone());
    let (mgr, dir) = fixture_manager("tamper", body.clone(), transport);
    mgr.download().expect("download");

    // Corrupt one byte of the final file AFTER download (simulate bit-rot / MITM).
    let final_path = dir.join(MODEL_FILE);
    let mut bytes = std::fs::read(&final_path).unwrap();
    let i = bytes.len() - 1;
    bytes[i] ^= 0xFF;
    std::fs::write(&final_path, &bytes).unwrap();

    // Verify MUST reject and MUST NOT report Ready.
    let r = mgr.verify();
    assert!(matches!(r, Err(ModelError::ChecksumMismatch)), "got {r:?}");
    assert!(
        !matches!(mgr.status(), ModelStatus::Ready),
        "a tampered file must NEVER be Ready"
    );
    // The quarantine removed/renamed the bad file so a stale Ready cannot appear.
    assert!(!final_path.exists(), "mismatched file must be quarantined/removed");
}

// ---------------------------------------------------------------------------
// (c) partial file + Range resume CONTINUES rather than restarts.
// ---------------------------------------------------------------------------

#[test]
fn partial_download_resumes_via_range_not_restart() {
    let body = gguf_fixture(8192);
    // Pre-seed a partial .part file with the first 3000 bytes (a prior interrupted
    // download). A CORRECT resume requests from offset 3000, not 0.
    let dir = tmp_model_dir("resume");
    std::fs::write(dir.join(format!("{MODEL_FILE}.part")), &body[..3000]).unwrap();

    let transport = FixtureTransport::new(body.clone());
    let expected_hash = sha256_hex(&body);
    // Keep a raw pointer to the transport to read its recorded offsets after.
    let mgr = ModelManager::new(
        dir.clone(),
        Box::new(transport),
        expected_hash,
        body.len() as u64,
    );

    // Status should reflect an in-progress download from the partial size.
    match mgr.status() {
        ModelStatus::Downloading { downloaded_bytes, total_bytes, .. } => {
            assert_eq!(downloaded_bytes, 3000);
            assert_eq!(total_bytes, body.len() as u64);
        }
        other => panic!("expected Downloading with partial bytes, got {other:?}"),
    }

    let offsets = mgr.download().expect("resume completes");
    // The download requested a Range from the existing partial size (3000), NOT 0
    // — proving it resumed rather than restarted.
    assert!(
        offsets.contains(&3000),
        "resume must request from the partial offset (3000); requested {offsets:?}"
    );
    assert!(
        !offsets.contains(&0),
        "resume must NOT restart from 0; requested {offsets:?}"
    );
    // The completed file is the full, correct body.
    let final_path = dir.join(MODEL_FILE);
    assert_eq!(std::fs::read(&final_path).unwrap(), body);
    mgr.verify().expect("verify after resume");
    assert!(matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (c2) F-3: the server IGNORES Range and returns the FULL body from 0 on a
// resume. The oversized `.part` must be rejected/reset and re-completed cleanly,
// NEVER finalized as Ready with a doubled/oversized file.
// ---------------------------------------------------------------------------

/// A transport that IGNORES the requested Range offset and ALWAYS streams the full
/// body from byte 0 (a real-world non-compliant server). On a resume (`have > 0`)
/// this would append the full body on top of the partial → an oversized `.part`.
struct IgnoreRangeTransport {
    body: Vec<u8>,
    requested_offsets: StdMutex<Vec<u64>>,
}
impl IgnoreRangeTransport {
    fn new(body: Vec<u8>) -> Self {
        IgnoreRangeTransport {
            body,
            requested_offsets: StdMutex::new(Vec::new()),
        }
    }
}
impl ModelTransport for IgnoreRangeTransport {
    fn total_size(&self) -> std::result::Result<u64, ModelError> {
        Ok(self.body.len() as u64)
    }
    fn get_from(&self, offset: u64) -> std::result::Result<Box<dyn std::io::Read + Send>, ModelError> {
        // Record the requested offset but SERVE THE FULL BODY regardless (ignore Range).
        self.requested_offsets.lock().unwrap().push(offset);
        Ok(Box::new(std::io::Cursor::new(self.body.clone())))
    }
}

#[test]
fn server_ignoring_range_overshoots_and_is_reset_not_finalized() {
    let body = gguf_fixture(8192);
    // Pre-seed a partial .part (3000 bytes) so download() resumes from offset 3000.
    let dir = tmp_model_dir("overshoot");
    std::fs::write(dir.join(format!("{MODEL_FILE}.part")), &body[..3000]).unwrap();

    let expected_hash = sha256_hex(&body);
    let mgr = ModelManager::new(
        dir.clone(),
        Box::new(IgnoreRangeTransport::new(body.clone())),
        expected_hash.clone(),
        body.len() as u64,
    );

    // The server ignores Range: it appends a FULL body onto the 3000-byte partial,
    // overshooting the pinned length. The download MUST error (Overshoot), never
    // finalize an oversized file as Ready.
    let r = mgr.download();
    assert!(
        matches!(r, Err(ModelError::Overshoot { .. })),
        "an oversized (Range-ignored) stream must error as Overshoot, got {r:?}"
    );
    // The final model file must NOT exist — an overshoot never finalizes.
    let final_path = dir.join(MODEL_FILE);
    assert!(!final_path.exists(), "overshoot must not finalize a model file");
    assert!(!matches!(mgr.status(), ModelStatus::Ready));
    // The oversized `.part` was reset (removed or truncated back to empty), so the
    // next attempt starts clean rather than resuming a corrupt oversized partial.
    let part_path = dir.join(format!("{MODEL_FILE}.part"));
    let part_len = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    assert_eq!(part_len, 0, "the oversized .part must be reset to a clean state");

    // A subsequent download against a COMPLIANT transport completes cleanly and
    // finalizes correctly (the overshoot did not corrupt future progress).
    let mgr2 = ModelManager::new(
        dir.clone(),
        Box::new(FixtureTransport::new(body.clone())),
        expected_hash,
        body.len() as u64,
    );
    mgr2.download().expect("clean re-download completes");
    assert_eq!(std::fs::read(&final_path).unwrap(), body);
    mgr2.verify().expect("verify after clean re-download");
    assert!(matches!(mgr2.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (d) wrong GGUF magic → abort (never write a bogus model as complete).
// ---------------------------------------------------------------------------

#[test]
fn wrong_gguf_magic_aborts_download() {
    // A body that does NOT start with the GGUF magic.
    let mut body = vec![0x00, 0x11, 0x22, 0x33];
    body.extend_from_slice(&[0u8; 2000]);
    let transport = FixtureTransport::new(body.clone());
    let (mgr, dir) = fixture_manager("badmagic", body, transport);

    let r = mgr.download();
    assert!(matches!(r, Err(ModelError::BadMagic)), "got {r:?}");
    // No completed model file — the bad stream was aborted, not renamed to final.
    assert!(!dir.join(MODEL_FILE).exists(), "bad-magic download must not finalize");
    assert!(!matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (e) short/failed transport mid-stream → Error, NEVER Ready (fail closed).
// ---------------------------------------------------------------------------

#[test]
fn short_transport_errors_and_never_ready() {
    let body = gguf_fixture(8192);
    // The transport delivers only 100 bytes then EOFs (a truncated/failed stream),
    // far short of the 8192-byte total.
    let transport = FixtureTransport::short(body.clone(), 100);
    let (mgr, dir) = fixture_manager("short", body, transport);

    let r = mgr.download();
    assert!(matches!(r, Err(ModelError::ShortStream { .. })), "got {r:?}");
    // The final model file must NOT exist (never finalize a short download).
    assert!(!dir.join(MODEL_FILE).exists());
    // Status is NOT Ready. It may honestly report a partial Downloading state, but
    // never Ready without a full verify.
    assert!(!matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (e2) NEGATIVE CONTROL: status is NEVER Ready without a real verify. A fully
// downloaded, correct file that has NOT been verified reports Present-but-not-
// Ready — the Ready state is EARNED only by verify(). This is the load-bearing
// no-Ready-without-verify guard.
// ---------------------------------------------------------------------------

#[test]
fn ready_requires_verify_not_just_presence() {
    let body = gguf_fixture(4096);
    let transport = FixtureTransport::new(body.clone());
    let (mgr, _dir) = fixture_manager("noverify", body, transport);
    mgr.download().expect("download");

    // The file is present + correct, but verify() has NOT run. Status must NOT be
    // Ready — presence alone never earns Ready.
    assert!(
        !matches!(mgr.status(), ModelStatus::Ready),
        "Ready must require a real verify, not mere file presence"
    );

    // Only after a real verify does Ready appear.
    mgr.verify().expect("verify");
    assert!(matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (f) serde camelCase shape — the status crosses the invoke boundary as the
// frontend contract expects (downloadedBytes/totalBytes/pct, error{msg}).
// ---------------------------------------------------------------------------

#[test]
fn status_serializes_camelcase() {
    let dl = ModelStatus::Downloading {
        downloaded_bytes: 10,
        total_bytes: 100,
        pct: 10.0,
    };
    let j = serde_json::to_value(&dl).unwrap();
    assert_eq!(j["state"], "downloading");
    assert_eq!(j["downloadedBytes"], 10);
    assert_eq!(j["totalBytes"], 100);
    assert_eq!(j["pct"], 10.0);

    let err = ModelStatus::Error {
        msg: "boom".into(),
    };
    let je = serde_json::to_value(&err).unwrap();
    assert_eq!(je["state"], "error");
    assert_eq!(je["msg"], "boom");

    assert_eq!(serde_json::to_value(ModelStatus::NotPresent).unwrap()["state"], "notPresent");
    assert_eq!(serde_json::to_value(ModelStatus::Verifying).unwrap()["state"], "verifying");
    assert_eq!(serde_json::to_value(ModelStatus::Ready).unwrap()["state"], "ready");
}

// ---------------------------------------------------------------------------
// (g) verify on a MISSING file errors honestly (never Ready on nothing).
// ---------------------------------------------------------------------------

#[test]
fn verify_missing_file_errors() {
    let body = gguf_fixture(64);
    let transport = FixtureTransport::new(body.clone());
    let (mgr, _dir) = fixture_manager("missing", body, transport);
    // No download performed — the file does not exist.
    let r = mgr.verify();
    assert!(matches!(r, Err(ModelError::NotPresent)), "got {r:?}");
    assert!(!matches!(mgr.status(), ModelStatus::Ready));
}

// ---------------------------------------------------------------------------
// (h) size mismatch (short body vs pinned expected size) is caught by verify
// even if a caller bypassed the streaming length check — belt-and-suspenders.
// ---------------------------------------------------------------------------

#[test]
fn size_mismatch_is_rejected() {
    let body = gguf_fixture(4096);
    let transport = FixtureTransport::new(body.clone());
    let dir = tmp_model_dir("sizemismatch");
    // Manager EXPECTS a larger size than the fixture actually is.
    let mgr = ModelManager::new(
        dir.clone(),
        Box::new(transport),
        sha256_hex(&body),
        body.len() as u64 + 10_000, // wrong expected size
    );
    // Write the (short) body directly as the final file, bypassing download.
    std::fs::write(dir.join(MODEL_FILE), &body).unwrap();
    let r = mgr.verify();
    assert!(matches!(r, Err(ModelError::SizeMismatch { .. })), "got {r:?}");
    assert!(!matches!(mgr.status(), ModelStatus::Ready));
}

