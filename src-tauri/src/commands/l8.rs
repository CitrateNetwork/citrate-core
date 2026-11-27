//! The app commands lane L8 registers (train together).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L8 edits this file. A command added here is also allowed in
//! `permissions/l8-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l8 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // HUP-S9.4 — plan/explain/start federated rounds (HIC-1) and the LoRA eval gate.
            fl_market::fl_market_offers,
            fl_rounds::fl_overview,
            fl_rounds::fl_coordinator_set,
            fl_rounds::fl_round_plan,
            fl_rounds::fl_round_plan_lookup,
            fl_rounds::fl_round_start,
            fl_rounds::fl_round_consent_revoke,
            fl_rounds::fl_adapter_gate,
            fl_rounds::fl_adapter_load,
            fl_rounds::fl_adapter_unload,
            // HUP-S9.3 — the member's opt-in for training on verified conversations (default off).
            fl_trajectories::trajectories_settings_get,
            fl_trajectories::trajectories_settings_set,
            fl_trajectories::trajectories_consent_revoke,
            fl_trajectories::trajectories_dataset_build,
            // 0.5.4 WP-H6 — the local dataset store (create, versions, sample, clean, consent).
            fl_datasets::fl_dataset_list,
            fl_datasets::fl_dataset_manifest,
            fl_datasets::fl_dataset_create,
            fl_datasets::fl_dataset_sample,
            fl_datasets::fl_dataset_clean_preview,
            fl_datasets::fl_dataset_clean_apply,
            fl_datasets::fl_dataset_consent_list,
            fl_datasets::fl_dataset_consent_withdraw,
            fl_datasets::fl_dataset_delete,
            fl_datasets::fl_dataset_publish_preview,
            fl_datasets::fl_dataset_publish,
            fl_datasets::fl_dataset_published,
            fl_trajectories::trajectories_datasets_list,
            // 0.5.4 FL design board 11: Settings › Work, sealed on this device.
            fl_work::fl_work_terms_get,
            fl_work::fl_work_terms_save,
            fl_work::fl_work_envelope_get,
            fl_work::fl_work_envelope_approve,
            fl_work::fl_work_envelope_off,
            fl_publish::fl_requirements_publish,
            fl_envelope::fl_envelope_status,
            fl_envelope::fl_envelope_check,
            // G-SEC PAY-14 — whether FL payouts are open, as the signer decides it.
            fl_payouts::fl_payouts_status,
            training::training_status,
            training::training_reward,
            // WP-H7 (0.5.4): the dataset PII scan, license check and publish gate.
            fl_dataset_gate::fl_dataset_pii_scan,
            fl_dataset_gate::fl_dataset_license_check,
            fl_dataset_gate::fl_dataset_publish_check,
            // WP-H8 (0.5.4): the compliant collector.
            fl_collect::fl_collect_policy,
            fl_collect::fl_collect_source_check,
            fl_collect::fl_collect_plan,
            fl_collect::fl_collect_plan_decide,
            fl_collect::fl_collect_run,
        ])
    };
}
