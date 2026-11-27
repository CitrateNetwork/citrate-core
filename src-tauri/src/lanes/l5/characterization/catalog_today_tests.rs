// L5-WP01 L5-CATALOG (characterization, citrate-core a826e441). Included into `model_catalog::tests`
// from model_catalog_tests.rs; registry ci/characterization.d/l5.json.
//
// Records two behaviours of the Models screen catalog as they are today, both of which L5-WP03
// fixes. They are pinned ON PURPOSE so WP03 has to replace them with stricter tests deliberately
// (named "superseded by" in its PR), never delete them:
//
// * E3: search and download-by-id resolve a Hugging Face repo at `main`, not at a commit, so an
//   upstream re-upload turns a download into a checksum mismatch (the failure #767 fixed for the
//   onboarding pins only).
// * E4: every `.gguf` with an LFS oid is listed as a model, including the vision/audio projector
//   (`mmproj-*`) and the draft model (`mtp-*`), which are not chat models.
//
// The tree below is the shape of `ggml-org/gemma-4-E4B-it-GGUF` at commit 8903cb40. The weights and
// mmproj rows carry their real sizes and sha256 (lane L5 evidence E1, E5); the draft-model row is a
// stand-in name, size and hash for a file of that role. No network: an injected client answers.

mod lq_l5_catalog {
    use crate::model::{ModelKind, ModelSource};
    use crate::model_catalog::{hf_files, hf_search, resolve_by_id};
    use crate::oidc::{AuthError, HttpClient};
    use std::sync::Mutex;

    const REPO: &str = "ggml-org/gemma-4-E4B-it-GGUF";
    const COMMIT: &str = "8903cb408c7df39cde2ed15fac397e6d7be92951";
    const WEIGHTS: &str = "gemma-4-E4B-it-Q4_0.gguf";
    const WEIGHTS_SHA: &str = "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2";
    const MMPROJ: &str = "mmproj-gemma-4-E4B-it-Q8_0.gguf";
    const MMPROJ_SHA: &str = "9257f21a0f5358204b4a612cff83fad2e56412f4f607f0049331ce60c0cf9d34";
    const DRAFT: &str = "mtp-gemma-4-E4B-it-Q8_0.gguf";

    fn draft_sha() -> String {
        "d".repeat(64)
    }

    fn tree() -> String {
        format!(
            r#"[
              {{"path":".gitattributes","size":1570,"oid":"aa11"}},
              {{"path":"README.md","size":2104,"oid":"bb22"}},
              {{"path":"{WEIGHTS}","size":135,"lfs":{{"oid":"{WEIGHTS_SHA}","size":4590807392}}}},
              {{"path":"{MMPROJ}","size":135,"lfs":{{"oid":"sha256:{MMPROJ_SHA}","size":559874848}}}},
              {{"path":"{DRAFT}","size":135,"lfs":{{"oid":"{}","size":312000000}}}}
            ]"#,
            draft_sha()
        )
    }

    /// Answers the HF search and tree endpoints and records every URL it was asked for.
    struct Recording {
        seen: Mutex<Vec<String>>,
    }

    impl Recording {
        fn new() -> Self {
            Recording {
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    impl HttpClient for Recording {
        fn get(&self, url: &str, _bearer: Option<&str>) -> Result<String, AuthError> {
            self.seen
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(url.to_string());
            if url.contains("/api/models?search=") {
                return Ok(format!(r#"[{{"id":"{REPO}"}}]"#));
            }
            if url.contains(&format!("/api/models/{REPO}/tree/")) {
                return Ok(tree());
            }
            Err(AuthError::Network)
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<String, AuthError> {
            Err(AuthError::Network)
        }
    }

    #[test]
    fn catalog_resolves_at_main_today() {
        let tree_at_main = format!("https://huggingface.co/api/models/{REPO}/tree/main");

        // Download and select re-resolve by id at `main`.
        let http = Recording::new();
        let id = format!("hf:{REPO}/{WEIGHTS}");
        let d = resolve_by_id(&http, &id, None).expect("resolves");
        assert_eq!(http.seen(), vec![tree_at_main.clone()]);
        assert_eq!(d.revision.as_deref(), Some("main"));
        assert_eq!(d.sha256, WEIGHTS_SHA);
        assert_eq!(
            d.download_url().as_deref(),
            Some(format!("https://huggingface.co/{REPO}/resolve/main/{WEIGHTS}").as_str())
        );

        // Search resolves every hit's files at `main` too.
        let http = Recording::new();
        let hits = hf_search(&http, "gemma", None).expect("search");
        let seen = http.seen();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(
            seen[0].contains("/api/models?search=gemma&filter=gguf"),
            "{seen:?}"
        );
        assert_eq!(seen[1], tree_at_main);
        assert!(!hits.is_empty());
        for h in &hits {
            assert_eq!(h.revision.as_deref(), Some("main"), "{}", h.id);
        }
    }

    #[test]
    fn catalog_lists_mmproj_as_a_model_today() {
        let http = Recording::new();
        let files = hf_files(&http, REPO, COMMIT, None).expect("tree");
        let names: Vec<&str> = files.iter().map(|d| d.file.as_str()).collect();
        // Non-GGUF files are skipped; every GGUF is a model, whatever its role.
        assert_eq!(names, vec![WEIGHTS, MMPROJ, DRAFT]);
        for d in &files {
            assert_eq!(d.id, format!("hf:{REPO}/{}", d.file));
            assert!(matches!(d.source, ModelSource::Hf), "{}", d.id);
            assert!(matches!(d.kind, ModelKind::Gguf), "{}", d.id);
            assert_eq!(d.repo, REPO);
            assert_eq!(d.revision.as_deref(), Some(COMMIT));
        }
        // The projector is offered like a chat model, with its real size and hash.
        let mmproj = &files[1];
        assert_eq!(mmproj.size_bytes, 559_874_848);
        assert_eq!(mmproj.sha256, MMPROJ_SHA);
        assert_eq!(files[0].size_bytes, 4_590_807_392);
        assert_eq!(files[2].sha256, draft_sha());
    }
}
