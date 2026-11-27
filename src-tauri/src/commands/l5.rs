//! The app commands lane L5 registers (models, multimodal and LoRA).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L5 edits this file. A command added here is also allowed in
//! `permissions/l5-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l5 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // ── CX (planset citrate-core-social) — command names FROZEN in S0.3. Each is a
            // NotWired stub until its lane wires the body (in its own module, never here). ──
            model_catalog::model_catalog_local,
            model_catalog::model_catalog_partials,
            model_catalog::model_catalog_search,
            model_catalog::model_catalog_download,
            model_catalog::model_catalog_select,
            model_registry::models_registry_list,
            model_register::models_registry_register,
            model_delete::model_delete_states,
            model_delete::model_delete,
            // HUP-S10.1 — media generation tiers, gallery, and the Media pop-out's data.
            media::media_options,
            media::media_set_settings,
            media::media_generate_image,
            media::media_gallery,
            media::media_read,
            media::media_save_copy,
            // model — BC-3.1 local Gemma download + verify. model_status is the
            // honest file-derived state (Ready ONLY after a real SHA-256 verify —
            // never mere presence, Rule 1); model_download is STREAMED + resumable
            // (HTTP Range) with a GGUF-magic gate + pinned-length finalize;
            // model_verify quarantines any checksum/size mismatch (never Ready).
            // No secrets, no key material cross this surface.
            model::model_status,
            model::model_download,
            model::model_download_pause,
            model::model_local_options,
            model::model_verify,
            // model-serve — BC-3.2 the bundled llama-server sidecar under the
            // SidecarSupervisor. model_serve_start fails CLOSED unless the model is
            // verified-Ready AND the binary is bundled; model_inference_state
            // returns the HONEST route (ready/local-fallback/downloading/
            // gateway-only/no-model/demo). No secret crosses this surface.
            serve::model_serve_start,
            serve::model_serve_stop,
            serve::model_inference_state,
        ])
    };
}
