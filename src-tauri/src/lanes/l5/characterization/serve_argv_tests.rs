// L5-WP01 L5-ARGV (characterization, citrate-core a826e441). Included into `serve::tests` from
// serve_tests.rs; registry ci/characterization.d/l5.json.
//
// Pins the llama-server argv exactly (E6): the model, a loopback bind, the port, the context
// (8192 before the plan is sized), the model's own chat template (--jinja), thinking split out
// (--reasoning-format deepseek), the web UI and slots monitor off, then -ngl only when the plan
// offloads, then --lora last only when an eval-gated adapter is set. There is no --mmproj today,
// so no local model can see an image. L5-WP07 (local vision) supersedes this test with a stricter
// one that names the pinned projector, in a separate commit that says why.

mod lq_l5_serve_argv {
    use crate::serve::LlamaServerManager;
    use crate::serve_plan::ServePlan;
    use std::path::PathBuf;

    const MODEL: &str = "/models/gemma-4-E4B-it-Q4_0.gguf";
    const ADAPTER: &str = "/adapters/a77d.gguf";

    fn manager() -> LlamaServerManager {
        LlamaServerManager::new(
            PathBuf::from("/opt/citrate/llama-server"),
            PathBuf::from(MODEL),
            PathBuf::from("/opt/citrate/llama-crash.jsonl"),
            18081,
        )
    }

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn default_argv_has_no_mmproj_and_keeps_jinja() {
        let mgr = manager();
        let base = argv(&[
            "-m",
            MODEL,
            "--host",
            "127.0.0.1",
            "--port",
            "18081",
            "--ctx-size",
            "8192",
            "--jinja",
            "--reasoning-format",
            "deepseek",
            "--no-webui",
            "--no-slots",
        ]);
        // Before any plan or adapter: exactly the base argv.
        assert_eq!(mgr.spawn_args_for_test(), base);

        // An eval-gated adapter is appended last as `--lora <path>`.
        mgr.set_lora(Some(PathBuf::from(ADAPTER)));
        let mut with_lora = base.clone();
        with_lora.extend(argv(&["--lora", ADAPTER]));
        assert_eq!(mgr.spawn_args_for_test(), with_lora);

        // A sized plan changes --ctx-size and adds -ngl before the adapter; nothing else moves.
        mgr.set_plan(ServePlan {
            ctx_size: 32768,
            gpu_layers: Some(99),
            ..ServePlan::not_sized()
        });
        let mut planned = base.clone();
        planned[7] = "32768".to_string();
        planned.extend(argv(&["-ngl", "99", "--lora", ADAPTER]));
        assert_eq!(mgr.spawn_args_for_test(), planned);

        // Clearing the adapter drops it; no projector flag appears in any of these.
        mgr.set_lora(None);
        let args = mgr.spawn_args_for_test();
        assert!(!args.iter().any(|a| a == "--lora"));
        for a in [base, with_lora, planned, args] {
            assert!(!a.iter().any(|w| w == "--mmproj" || w == "-mm"), "{a:?}");
            assert!(a.iter().any(|w| w == "--jinja"), "{a:?}");
        }
    }
}
