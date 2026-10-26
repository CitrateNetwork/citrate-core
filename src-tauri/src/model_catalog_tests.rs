// CX-S1.4 — model catalog resolver tests. Included into `model_catalog::tests` (private access).
//
// The resolver is pure over an injected HttpClient, so these drive canned HF/GitHub API bodies
// through the real parsers — no network. They pin the two invariants that matter: (1) the shape
// mapping onto ModelDescriptor is correct, and (2) a file with no trustworthy sha256 is SKIPPED,
// never surfaced as an unverifiable download (Rule 1).

use super::*;

/// A fixture HttpClient that answers `get` from the first route whose fragment the URL contains.
/// Routes are checked in order, so put more-specific fragments first.
struct FixtureHttp {
    routes: Vec<(String, String)>,
}
impl FixtureHttp {
    fn new(routes: &[(&str, &str)]) -> Self {
        FixtureHttp {
            routes: routes
                .iter()
                .map(|(f, b)| (f.to_string(), b.to_string()))
                .collect(),
        }
    }
}
impl crate::oidc::HttpClient for FixtureHttp {
    fn get(
        &self,
        url: &str,
        _bearer: Option<&str>,
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        for (frag, body) in &self.routes {
            if url.contains(frag.as_str()) {
                return Ok(body.clone());
            }
        }
        Err(crate::oidc::AuthError::Network)
    }
    fn post_form(
        &self,
        _url: &str,
        _form: &[(&str, &str)],
    ) -> std::result::Result<String, crate::oidc::AuthError> {
        Err(crate::oidc::AuthError::Network)
    }
}

/// 64 hex chars (upper) → normalized to lowercase; a stand-in real sha256.
fn hex64(c: char) -> String {
    std::iter::repeat_n(c, 64).collect()
}

#[test]
fn pct_encodes_query_reserved_chars_only() {
    assert_eq!(pct("gemma"), "gemma");
    assert_eq!(pct("q4 gguf"), "q4%20gguf");
    assert_eq!(pct("a/b?c=1&d"), "a%2Fb%3Fc%3D1%26d");
    assert_eq!(pct("keep-._~"), "keep-._~"); // unreserved set stays literal
}

#[test]
fn normalize_sha256_accepts_bare_and_prefixed_rejects_junk() {
    let a = hex64('A');
    assert_eq!(normalize_sha256(&a).as_deref(), Some(a.to_lowercase().as_str()));
    assert_eq!(
        normalize_sha256(&format!("sha256:{a}")).as_deref(),
        Some(a.to_lowercase().as_str())
    );
    assert_eq!(normalize_sha256("  short  "), None);
    assert_eq!(normalize_sha256(&"z".repeat(64)), None); // 'z' not hex
    assert_eq!(normalize_sha256(&hex64('a').repeat(2)), None); // too long
}

#[test]
fn hf_search_resolves_gguf_files_and_skips_unverifiable() {
    let sha = hex64('A');
    let tree = format!(
        r#"[
          {{"path":"README.md","size":10,"oid":"deadbeef"}},
          {{"path":"model-q4.gguf","size":135,"lfs":{{"oid":"sha256:{sha}","size":4590807392}}}},
          {{"path":"model-q8.gguf","size":50}},
          {{"path":"model-bad.gguf","size":9,"lfs":{{"oid":"nothex","size":5}}}}
        ]"#
    );
    let http = FixtureHttp::new(&[
        ("/tree/", tree.as_str()),
        ("search=", r#"[{"id":"acme/cool-gguf"}]"#),
    ]);

    let out = hf_search(&http, "cool", None).expect("search resolves");
    // Only the one GGUF with a real LFS sha256 survives; README, no-lfs, and bad-oid are dropped.
    assert_eq!(out.len(), 1, "got {out:?}");
    let d = &out[0];
    assert_eq!(d.id, "hf:acme/cool-gguf/model-q4.gguf");
    assert_eq!(d.source, ModelSource::Hf);
    assert_eq!(d.repo, "acme/cool-gguf");
    assert_eq!(d.file, "model-q4.gguf");
    assert_eq!(d.revision.as_deref(), Some("main"));
    assert_eq!(d.size_bytes, 4_590_807_392); // the LFS size, not the pointer size
    assert_eq!(d.sha256, sha.to_lowercase());
    assert_eq!(d.kind, ModelKind::Gguf);
    // The derived download URL is the HF resolve path for this repo/rev/file.
    assert_eq!(
        d.download_url().as_deref(),
        Some("https://huggingface.co/acme/cool-gguf/resolve/main/model-q4.gguf")
    );
}

#[test]
fn github_releases_requires_a_sha256_sidecar() {
    let sha = hex64('B');
    let releases = r#"[{"tag_name":"v1.0","assets":[
        {"name":"m.gguf","size":100},
        {"name":"m.gguf.sha256","size":80},
        {"name":"nosidecar.gguf","size":200}
    ]}]"#;
    let sidecar_body = format!("{sha}  m.gguf\n");
    let http = FixtureHttp::new(&[
        ("releases/download/", sidecar_body.as_str()),
        ("api.github.com", releases),
    ]);

    let out = github_releases(&http, "owner/repo", None).expect("resolves");
    // Only the asset WITH a sidecar is catalog-addable; the bare .gguf is skipped.
    assert_eq!(out.len(), 1, "got {out:?}");
    let d = &out[0];
    assert_eq!(d.id, "github:owner/repo/m.gguf@v1.0");
    assert_eq!(d.source, ModelSource::Github);
    assert_eq!(d.file, "m.gguf");
    assert_eq!(d.revision.as_deref(), Some("v1.0"));
    assert_eq!(d.size_bytes, 100);
    assert_eq!(d.sha256, sha.to_lowercase());
    assert_eq!(
        d.download_url().as_deref(),
        Some("https://github.com/owner/repo/releases/download/v1.0/m.gguf")
    );
}

#[test]
fn github_source_rejects_a_non_slug_query() {
    let http = FixtureHttp::new(&[]);
    assert!(github_releases(&http, "notaslug", None).is_err());
    assert!(github_releases(&http, "", None).is_err());
}

#[test]
fn search_dispatches_by_source_and_bundled_is_empty() {
    let http = FixtureHttp::new(&[("search=", "[]")]);
    // Bundled is not a searchable source.
    assert!(search(&http, ModelSource::Bundled, "anything", None)
        .unwrap()
        .is_empty());
    // Hf routes to hf_search (empty repo list → empty result, no error).
    assert!(search(&http, ModelSource::Hf, "gemma", None).unwrap().is_empty());
}

#[test]
fn resolver_surfaces_transport_errors_as_err() {
    // No routes match → the fixture returns AuthError::Network, which the resolver maps to Err.
    let http = FixtureHttp::new(&[]);
    assert!(hf_search(&http, "x", None).is_err());
}

#[test]
fn model_file_from_id_extracts_the_filename() {
    assert_eq!(
        model_file_from_id("hf:acme/cool-gguf/model-q4.gguf").unwrap(),
        "model-q4.gguf"
    );
    assert_eq!(
        model_file_from_id("github:owner/repo/m.gguf@v1.0").unwrap(),
        "m.gguf"
    );
    // Bundled ids map to the app's default model file (no network, no parse).
    assert_eq!(
        model_file_from_id("bundled:gemma-4-e4b-it-q4_0").unwrap(),
        crate::model::MODEL_FILE
    );
    assert!(model_file_from_id("nonsense").is_err());
    assert!(model_file_from_id("github:no-at-sign").is_err());
}

#[test]
fn resolve_by_id_reresolves_a_fresh_descriptor() {
    let sha = hex64('C');
    let tree = format!(
        r#"[{{"path":"model-q4.gguf","size":1,"lfs":{{"oid":"{sha}","size":1234}}}}]"#
    );
    let http = FixtureHttp::new(&[("/tree/", tree.as_str())]);

    // hf id → re-resolves via hf_files, matching by filename.
    let d = resolve_by_id(&http, "hf:acme/cool-gguf/model-q4.gguf", None).expect("resolves");
    assert_eq!(d.repo, "acme/cool-gguf");
    assert_eq!(d.file, "model-q4.gguf");
    assert_eq!(d.sha256, sha.to_lowercase());
    assert_eq!(d.size_bytes, 1234);

    // A file the repo doesn't expose → an honest not-found error, not a fabricated descriptor.
    assert!(resolve_by_id(&http, "hf:acme/cool-gguf/missing.gguf", None).is_err());
    assert!(resolve_by_id(&http, "bundled:x", None).is_err()); // bundled has no catalog source
}

// --- #63: read_local_models — scan the models dir for verified on-disk GGUFs ---
#[test]
fn read_local_models_lists_only_verified_gguf_files() {
    let dir = std::env::temp_dir().join(format!("cc-local-models-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // a.gguf — verified (has a status.json recording the earned verify) → listed
    std::fs::write(dir.join("a.gguf"), vec![0u8; 10]).unwrap();
    std::fs::write(dir.join("a.gguf.status.json"), r#"{"verified":true}"#).unwrap();
    // b.gguf — present but NOT verified (no status.json) → skipped (router treats local as ready)
    std::fs::write(dir.join("b.gguf"), vec![0u8; 20]).unwrap();
    // c.gguf — status.json says NOT verified (interrupted verify) → skipped
    std::fs::write(dir.join("c.gguf"), vec![0u8; 30]).unwrap();
    std::fs::write(dir.join("c.gguf.status.json"), r#"{"verified":false}"#).unwrap();
    // noise: a partial download + a non-model file → ignored
    std::fs::write(dir.join("d.gguf.part"), vec![0u8; 5]).unwrap();
    std::fs::write(dir.join("notes.txt"), b"nope").unwrap();

    let models = read_local_models(&dir).expect("scan ok");
    assert_eq!(models.len(), 1, "only the verified a.gguf is listed");
    let m = &models[0];
    assert_eq!(m.file, "a.gguf");
    assert_eq!(m.id, "local:a.gguf");
    assert_eq!(m.size_bytes, 10);
    assert!(matches!(m.kind, crate::model::ModelKind::Gguf));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn read_local_models_missing_dir_is_honest_empty_not_error() {
    let missing = std::path::Path::new("/no/such/models/dir/xyz");
    assert_eq!(read_local_models(missing).unwrap().len(), 0);
}

/// PBA-L7b-013: a GGUF in an HF sub-directory resolves against the RIGHT repo (the first two
/// segments) instead of `owner/name/subdir`, and lands locally as a single flat file.
#[test]
fn pba_l7b_013_hf_subdirectory_ggufs_resolve_and_select() {
    let sha = hex64('D');
    let tree = format!(
        r#"[{{"path":"Q4/model-q4.gguf","size":1,"lfs":{{"oid":"{sha}","size":99}}}}]"#
    );
    // Only answers the CORRECT repo tree URL.
    let http = FixtureHttp::new(&[("/models/acme/cool-gguf/tree/", tree.as_str())]);
    let d = resolve_by_id(&http, "hf:acme/cool-gguf/Q4/model-q4.gguf", None)
        .expect("a sub-directory GGUF resolves against owner/name");
    assert_eq!(d.repo, "acme/cool-gguf");
    assert_eq!(d.file, "Q4/model-q4.gguf");
    assert_eq!(
        model_file_from_id("hf:acme/cool-gguf/Q4/model-q4.gguf").unwrap(),
        "model-q4.gguf"
    );
    // Traversal / empty segments are malformed, never a path.
    for bad in ["hf:acme/cool-gguf/../x.gguf", "hf:acme//x.gguf", "hf:acme/x", "hf:../x/y.gguf"] {
        assert!(model_file_from_id(bad).is_err(), "{bad}");
    }
}

// HUP-S0.3 — a catalog download interrupted by an app restart is listed as resumable (it used to
// vanish: the in-memory slice was lost and read_local_models skips `.part` files).
#[test]
fn interrupted_catalog_downloads_are_listed_as_resumable() {
    let dir = std::env::temp_dir().join(format!("cc-partials-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // A tracked partial: 10 of 40 bytes.
    write_download_marker(&dir, "qwen.gguf", "hf:org/repo/qwen.gguf", 40).unwrap();
    std::fs::write(dir.join("qwen.gguf.part"), [0u8; 10]).unwrap();
    // An untracked stray .part (no marker → unknown origin → not offered).
    std::fs::write(dir.join("stray.gguf.part"), [0u8; 5]).unwrap();
    // A marker whose download has not written a byte yet.
    write_download_marker(&dir, "new.gguf", "hf:org/repo/new.gguf", 100).unwrap();

    let mut got = read_partial_downloads(&dir);
    got.sort_by(|a, b| a.file.cmp(&b.file));
    assert_eq!(got.len(), 2, "tracked partials only: {got:?}");
    assert_eq!(got[1].id, "hf:org/repo/qwen.gguf");
    assert_eq!(got[1].downloaded_bytes, 10);
    assert_eq!(got[1].total_bytes, 40);
    assert_eq!(got[1].pct, 25);
    assert_eq!(got[0].downloaded_bytes, 0);

    // Finishing clears the marker, so a completed model is never offered as a resume.
    clear_download_marker(&dir, "qwen.gguf");
    assert!(read_partial_downloads(&dir).iter().all(|p| p.file != "qwen.gguf"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_models_dir_has_no_partials() {
    assert!(read_partial_downloads(std::path::Path::new("/nonexistent/citrate/models")).is_empty());
}

/// Bug (owner, 2026-10-01): "Use this model" on a downloaded model failed with
/// "unknown model id source: local:Qwen3.8-27B-GSQ-RCO-IQ2_S-mtp.gguf". `read_local_models` emits
/// `local:<file>` ids, so select must map them to their on-disk file (and nothing else).
#[test]
fn local_ids_from_the_models_list_map_to_their_file() {
    assert_eq!(
        model_file_from_id("local:Qwen3.8-27B-GSQ-RCO-IQ2_S-mtp.gguf").unwrap(),
        "Qwen3.8-27B-GSQ-RCO-IQ2_S-mtp.gguf"
    );
    assert_eq!(model_file_from_id("local:a.gguf").unwrap(), "a.gguf");
    // Every id the listing produces is accepted back by select.
    let dir = std::env::temp_dir().join(format!("local-ids-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for m in read_local_models(&dir).unwrap() {
        assert_eq!(model_file_from_id(&m.id).unwrap(), m.file);
    }
}

#[test]
fn local_ids_are_a_single_plain_gguf_filename() {
    for bad in [
        "local:",
        "local:../x.gguf",
        "local:sub/x.gguf",
        "local:sub\\x.gguf",
        "local:/etc/x.gguf",
        "local:.hidden.gguf",
        "local:x.gguf.part",
        "local:x.bin",
        "local:..",
        "local:x\0.gguf",
    ] {
        assert!(model_file_from_id(bad).is_err(), "{bad:?}");
    }
    let long = format!("local:{}.gguf", "a".repeat(300));
    assert!(model_file_from_id(&long).is_err());
}

// ---- #489: the resume marker is written only once the file's download lock is held -----------

fn gguf_body(fill: u8) -> Vec<u8> {
    let mut b = b"GGUF".to_vec();
    b.extend_from_slice(&[fill; 40]);
    b
}

fn tiny_descriptor(id: &str, body: &[u8]) -> crate::model::ModelDescriptor {
    use sha2::Digest as _;
    crate::model::ModelDescriptor {
        id: id.to_string(),
        source: crate::model::ModelSource::Hf,
        repo: "org/repo".to_string(),
        file: "tiny.gguf".to_string(),
        revision: None,
        size_bytes: body.len() as u64,
        sha256: hex::encode(sha2::Sha256::digest(body)),
        kind: crate::model::ModelKind::Gguf,
    }
}

/// Serves `body`, and records whether the marker was on disk, under a held lock, when the bytes
/// were requested.
struct MarkerProbe {
    dir: std::path::PathBuf,
    body: Vec<u8>,
    seen: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl crate::model::ModelTransport for MarkerProbe {
    fn total_size(&self) -> std::result::Result<u64, crate::model::ModelError> {
        Ok(self.body.len() as u64)
    }
    fn get_from(
        &self,
        offset: u64,
    ) -> std::result::Result<Box<dyn std::io::Read + Send>, crate::model::ModelError> {
        let tracked = marker_path(&self.dir, "tiny.gguf").exists()
            && crate::model::download_in_flight(&self.dir.join("tiny.gguf.part"));
        *self.seen.lock().unwrap() = Some(tracked);
        let start = (offset as usize).min(self.body.len());
        Ok(Box::new(std::io::Cursor::new(self.body[start..].to_vec())))
    }
}

fn catalog_tmp(tag: &str) -> std::path::PathBuf {
    let n: u64 = rand::random();
    std::env::temp_dir().join(format!("citrate-core-catalog-{tag}-{n:016x}"))
}

#[test]
fn a_catalog_download_is_tracked_while_it_streams_and_forgotten_once_verified() {
    let dir = catalog_tmp("tracked");
    std::fs::create_dir_all(&dir).unwrap();
    let body = gguf_body(4);
    let desc = tiny_descriptor("hf:org/repo/tiny.gguf", &body);
    let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
    let probe = MarkerProbe { dir: dir.clone(), body: body.clone(), seen: seen.clone() };
    download_tracked(&dir, &desc.id, &desc, Box::new(probe), |_, _| {}).expect("downloaded and verified");
    assert_eq!(*seen.lock().unwrap(), Some(true), "the marker is there, under the lock, while bytes stream");
    assert!(!marker_path(&dir, "tiny.gguf").exists(), "and gone once verified");
    assert!(crate::model::is_file_ready(&dir, "tiny.gguf"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_catalog_download_refused_by_the_lock_writes_no_marker() {
    let dir = catalog_tmp("refused");
    std::fs::create_dir_all(&dir).unwrap();
    // Another download (the Store's) holds the file's lock.
    let lock = crate::model::DownloadGuard::acquire(&dir.join("tiny.gguf.part")).expect("lock");
    let body = gguf_body(5);
    let desc = tiny_descriptor("hf:org/repo/tiny.gguf", &body);
    let probe = MarkerProbe { dir: dir.clone(), body, seen: Default::default() };
    let err = download_tracked(&dir, &desc.id, &desc, Box::new(probe), |_, _| {}).unwrap_err();
    assert_eq!(err, crate::model::ModelError::AlreadyDownloading.to_string());
    assert!(!marker_path(&dir, "tiny.gguf").exists(), "no marker for a download that never got the lock");
    drop(lock);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_catalog_download_leaves_the_running_downloads_marker_as_it_was() {
    let dir = catalog_tmp("refused-keep");
    std::fs::create_dir_all(&dir).unwrap();
    // A Models download of model A is running: its marker is written and it holds the lock.
    let lock = crate::model::DownloadGuard::acquire(&dir.join("tiny.gguf.part")).expect("lock");
    write_download_marker(&dir, "tiny.gguf", "hf:org/a/tiny.gguf", 44).unwrap();
    let before = std::fs::read(marker_path(&dir, "tiny.gguf")).unwrap();
    let body = gguf_body(6);
    let desc = tiny_descriptor("hf:org/b/tiny.gguf", &body);
    let probe = MarkerProbe { dir: dir.clone(), body, seen: Default::default() };
    assert!(download_tracked(&dir, &desc.id, &desc, Box::new(probe), |_, _| {}).is_err());
    assert_eq!(std::fs::read(marker_path(&dir, "tiny.gguf")).ok(), Some(before), "A's marker is untouched");
    drop(lock);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Whether every resume-marker write in `src` (model_catalog.rs's production part) is the claim
/// `download_and_verify` runs once it holds the file's download lock.
fn markers_written_under_the_lock(src: &str) -> Result<(), String> {
    let prod = src.split("#[cfg(test)]\nmod tests").next().unwrap_or(src);
    let calls: Vec<usize> = prod
        .match_indices("write_download_marker(")
        .map(|(i, _)| i)
        .filter(|&i| !prod[..i].ends_with("pub fn "))
        .collect();
    if calls.is_empty() {
        return Err("no marker write found".into());
    }
    for i in calls {
        let Some(dv) = prod[..i].rfind(".download_and_verify(") else {
            return Err(format!("marker write at byte {i} is not inside a download_and_verify call"));
        };
        if prod[dv..i].contains(';') {
            return Err(format!("marker write at byte {i} runs outside the locked claim"));
        }
    }
    Ok(())
}

/// Tripwire (#489): a Models download writes its marker only as the claim `download_and_verify`
/// runs under the file's download lock; and that claim runs right after the lock is taken.
#[test]
fn tripwire_the_models_marker_is_written_only_under_the_download_lock() {
    markers_written_under_the_lock(include_str!("model_catalog.rs")).expect("model_catalog.rs");
    // Negative control: the order #489 fixed (marker first, then the locked download) is caught.
    let before_the_lock = "fn f() {\n    write_download_marker(&dir, &file, &id, size)?;\n    mgr.download_and_verify(ExistingFile::ResumeUnverified, || Ok(()), |_, _| {})?;\n}\n";
    assert!(markers_written_under_the_lock(before_the_lock).is_err());
    let model = include_str!("model.rs");
    let f = &model[model.find("pub fn download_and_verify(").expect("fn")..];
    let f = &f[..f.find("\n    }\n").expect("ends")];
    let guard = f.find("DownloadGuard::acquire(").expect("guard");
    let claim = f.find("on_claimed()").expect("claim");
    assert!(guard < claim && claim < f.find("self.final_path().exists()").expect("check"), "lock, claim, check");
}
