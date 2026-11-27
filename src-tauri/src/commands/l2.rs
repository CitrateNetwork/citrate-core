//! The app commands lane L2 registers (HIC, sandbox and deploy).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L2 edits this file. A command added here is also allowed in
//! `permissions/l2-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l2 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // signing — the B1.2 SignatureCeremony (the ONE HITL signing path).
            // sign_request returns a CeremonyId + decoded intent (NO signature);
            // sign_approve returns the signature hex ONLY (never key/seed/entropy
            // — I-2 compile barrier holds); sign_reject consumes with no sig. The
            // gated wallet::sign_message is reachable ONLY via approve (@rule8).
            ceremony::sign_request,
            ceremony::sign_approve,
            ceremony::sign_and_broadcast,
            // Join-8 — sign without sending (returns the tx hash), then send.
            ceremony::sign_prepare,
            ceremony::sign_send_prepared,
            ceremony::sign_reject,
            contract_deploy::contract_deploy,
            // HUP-S6.4 — the D-4 deploy gate (verifier outputs in, READY / NOT READY out).
            deploy_gate::deploy_gate_submit,
            deploy_gate::deploy_gate_lookup,
            // HUP-S6.3 → S6.4 — a Hermes session's toolchain reports into the gate (A27).
            deploy_gate_toolchain::deploy_gate_submit_toolchain,
            fork_dry_run::deploy_gate_fork_dry_run,
            // HUP-S6.6 — post-deploy steps for a hello-mint project.
            postdeploy::postdeploy_status,
            postdeploy::postdeploy_receipt,
            postdeploy::postdeploy_verify,
            postdeploy::postdeploy_switch_site,
            postdeploy::postdeploy_pin_site,
            postdeploy::postdeploy_vercel_export,
            // CORE-A3: durable, encrypted deploy records per project.
            deploy_records::deploy_record_save,
            deploy_records::deploy_records_list,
            deploy_records::deploy_record_get,
            faucet::faucet_status,
            faucet::faucet_grant,
            faucet::faucet_revoke,
            faucet::faucet_request,
            faucet::faucet_open_challenge,
            hermes::undo::hermes_checkpoints,
            hermes::undo::hermes_undo_step,
            hermes::undo::hermes_undo_session,
            // HUP-S5.4 — one step's diff for the Code and diff pop-out (read-only).
            hermes::undo::hermes_checkpoint_diff,
            // HUP-S2.2: the member decides each shell_run command the sidecar holds.
            hermes::shell::hermes_shell_pending,
            hermes::shell::hermes_shell_decide,
            hermes_terminal::hermes_terminal_get,
            hermes_terminal::hermes_terminal_set,
            // V1 (0.5.4): per-project opt-ins for node, npm run and git commit.
            hermes_auto_approve::hermes_auto_approve_get,
            hermes_auto_approve::hermes_auto_approve_set,
            hermes_auto_approve::hermes_auto_approve_rules,
            // HUP-S6.3 — the toolchain switch (off by default; applies at the next Hermes start).
            forge_toolchain::toolchain_settings_get,
            forge_toolchain::toolchain_settings_set,
        ])
    };
}
