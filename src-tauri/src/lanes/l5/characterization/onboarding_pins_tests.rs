// L5-WP01 L5-PINS (characterization, citrate-core a826e441). Included into `model::tests` from
// model_tests.rs; registry ci/characterization.d/l5.json.
//
// Pins the two onboarding models (#767) exactly as they are before any L5 change: file, size,
// sha256, Hugging Face repo and commit, the Citrate redirect on the default only, no Citrate mirror
// yet, and the order the download tries its sources in. The TS copy (src/shell/localModels.ts) is
// checked against these pins by localModels.test.ts. A failure here means a pin changed: change
// this expectation only in the reviewed PR that re-pins the model, and say why in that commit.

mod lq_l5_onboarding_pins {
    use crate::model::{
        DEFAULT_MODEL_URL, DEFAULT_PIN, MODEL_FILE, MODEL_SHA256, MODEL_SIZE_BYTES, SMALL_PIN,
    };

    #[test]
    fn onboarding_pins_are_unchanged() {
        // The default: Gemma 4 E4B instruct, Q4_0.
        assert_eq!(DEFAULT_PIN.key, "default");
        assert_eq!(DEFAULT_PIN.label, "Gemma 4 E4B");
        assert_eq!(DEFAULT_PIN.file, "gemma-4-E4B-it-Q4_0.gguf");
        assert_eq!(DEFAULT_PIN.size_bytes, 4_590_807_392);
        assert_eq!(
            DEFAULT_PIN.sha256,
            "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2"
        );
        assert_eq!(DEFAULT_PIN.hf_repo, "ggml-org/gemma-4-E4B-it-GGUF");
        assert_eq!(
            DEFAULT_PIN.hf_revision,
            "8903cb408c7df39cde2ed15fac397e6d7be92951"
        );
        assert_eq!(
            DEFAULT_PIN.vanity_url,
            Some("https://citrate.ai/download/model")
        );
        assert!(DEFAULT_PIN.mirror_urls.is_empty(), "no Citrate mirror yet");
        // The single-model constants the verify path reads are the default pin's values.
        assert_eq!(MODEL_FILE, DEFAULT_PIN.file);
        assert_eq!(MODEL_SIZE_BYTES, DEFAULT_PIN.size_bytes);
        assert_eq!(MODEL_SHA256, DEFAULT_PIN.sha256);
        assert_eq!(Some(DEFAULT_MODEL_URL), DEFAULT_PIN.vanity_url);
        // Sources in order: the Citrate redirect, then Hugging Face at the pinned commit.
        assert_eq!(
            DEFAULT_PIN.source_urls(),
            vec![
                "https://citrate.ai/download/model".to_string(),
                "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/8903cb408c7df39cde2ed15fac397e6d7be92951/gemma-4-E4B-it-Q4_0.gguf".to_string(),
            ]
        );

        // The smaller option: Gemma 4 E2B instruct, Q4_0. Hugging Face only.
        assert_eq!(SMALL_PIN.key, "small");
        assert_eq!(SMALL_PIN.label, "Gemma 4 E2B");
        assert_eq!(SMALL_PIN.file, "gemma-4-E2B-it-Q4_0.gguf");
        assert_eq!(SMALL_PIN.size_bytes, 2_841_481_184);
        assert_eq!(
            SMALL_PIN.sha256,
            "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52"
        );
        assert_eq!(SMALL_PIN.hf_repo, "ggml-org/gemma-4-E2B-it-GGUF");
        assert_eq!(
            SMALL_PIN.hf_revision,
            "2a8538c2e2d4687953a86c422c7c623cfd022880"
        );
        assert_eq!(SMALL_PIN.vanity_url, None);
        assert!(SMALL_PIN.mirror_urls.is_empty(), "no Citrate mirror yet");
        assert_eq!(
            SMALL_PIN.source_urls(),
            vec!["https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/resolve/2a8538c2e2d4687953a86c422c7c623cfd022880/gemma-4-E2B-it-Q4_0.gguf".to_string()]
        );
    }
}
