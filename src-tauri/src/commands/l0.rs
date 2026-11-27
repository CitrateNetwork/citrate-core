//! The app commands lane L0 registers (integration seams).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L0 edits this file. A command added here is also allowed in
//! `permissions/l0-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l0 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // config — the one genuinely-live domain (A1.4)
            config::config_read,
            config::config_write,
            config::config_keyring_status,
            // custody — the OS-keyring vault (A2). NOTE: custody_get is NOT here;
            // it is an in-process pub fn — no invoke command returns secret bytes.
            custody::custody_status,
            custody::custody_init,
            custody::custody_unlock,
            // Seamless device-bound re-unlock (passphrase-less model): re-provisions
            // + unlocks from the keyring device passphrase, for the Settings "Unlock"
            // control and app launch/resume. Without it an auto-locked vault (and the
            // wallet reads gated on it) stay stuck — no user passphrase exists to enter.
            custody::custody_ensure_unlocked,
            custody::custody_lock,
            custody::custody_list,
            // auth — real OIDC loopback-PKCE (A3). Replaces the A1 auth seam
            // stubs. Every command returns claim-derived AuthStatus or (); NO
            // command returns a token (ADV-8 boundary; see oidc::tests).
            oidc::auth_status,
            oidc::auth_login,
            // FIX 2 (rc.5): abort the in-flight system-browser sign-in (Cancel button).
            oidc::auth_login_cancel,
            oidc::auth_userinfo,
            oidc::auth_refresh,
            oidc::auth_logout,
            oidc::kyc_start,
            // Group claimable invites (ADR D4) — mint / list / verify-consume / revoke.
            invites::group_invite_create,
            invites::group_invites,
            invites::group_invite_revoke,
            invites::group_invite_requests,
            invites::group_invite_approve,
            invites::group_invite_check,
            invites::group_invite_decline,
            invites::group_invite_preview,
            invites::group_invite_request,
            invites::group_invite_my_requests,
            invites::group_invite_dismiss,
            // Download gate: first-run access check for invited installs (off unless built with a gate URL).
            access_pass::access_pass_status,
            access_pass::access_pass_verify,
            // membership — the D3.C checkout popup (@rule8 money seam). Opens the
            // REAL core-membership checkout ({coreMembershipUrl}/checkout) in an
            // in-app popup with the SAME isolation as the auth popup (a remote
            // HTTPS page, NO capability/IPC). It NEVER signs or reports a settled
            // membership — the money + grant are server-side; the store polls
            // /userinfo and only advances S3 when the REAL entitlement lands.
            membership::membership_checkout,
            membership::membership_enterprise_lead,
            // membership grant status — BC-1.3 (@rule8 · T1 money-path READ). Reads
            // the REAL on-chain grant: MembershipStakeVault.attributedStake +
            // attributedShares(member) and CitrateMemberSBT.balanceOf(member) via
            // eth_call on 40204. The S5 membership grant ceremony settles ONLY from these
            // real reads (Rule 1 — no fabricated settlement). A PURE READ: it signs
            // nothing and returns only decoded public chain data (@rule8 / Rule 3).
            grant_status::membership_grant_status,
            membership_price::membership_price,
            // The grant signer's readiness from core-membership's public /api/health (a pure
            // read; S3 uses it only to say a grant is delayed, never to settle one).
            membership_health::membership_signer_health,
            // SBT emblem — BC-5.3 (T1 identity READ). Reads the AUTHORITATIVE
            // wholly-on-chain member emblem: isSubBound(keccak256(sub)) ->
            // tokenIdForSub -> tokenURI on CitrateMemberSBT (40204), decoded to the
            // `data:image/svg+xml;base64,...` the UI renders — or None honestly when
            // the member has no SBT (Rule 1 — no fabricated art). A PURE READ: it
            // signs nothing and touches no key material (@rule8 / Rule 3).
            sbt_art::sbt_token_uri,
            wallet_link::wallet_link_request,
            wallet_link::wallet_link_approve,
            wallet_link::wallet_link_reject,
            // HUP-S1.6 — hardware tier recommendation + persisted override (local only).
            tier::tier_recommend,
            tier::tier_set_override,
            // HUP-S4.3 — get_verified_source (read-only CitrateScan lookup).
            verified_source::contract_verified_source,
            // HUP-S6.2 / US-6.4 — contract templates (render into a granted folder).
            template_forge::template_list,
            template_forge::template_render,
            // 0.5.4: the Hermes chat history, sealed on this device (P-11).
            hermes_history::hermes_history_list,
            hermes_history::hermes_history_save,
            hermes_history::hermes_history_delete,
            hermes_history::hermes_history_delete_all,
            hermes_history::hermes_history_set_retention,
            // HUP-S6.7 — the Contract reader (reads; writes only open a ceremony).
            contract_reader::contract_source,
            contract_reader::contract_code_size,
            contract_reader::contract_view_call,
            contract_reader::contract_view_call_at,
            contract_reader::contract_write_propose,
            // CORE-A1 / CORE-A2 — find, identify and decode contracts (read-only).
            contract_intel::contract_address_book_lookup,
            contract_intel::contract_get_code,
            contract_intel::contract_identify,
            contract_intel::contract_abi_resolve,
            contract_intel::abi_signature_lookup,
            contract_intel::chain_tx_fetch,
            // HUP-S1.5 — escalation router: member endpoints (key in the OS keyring), the daily
            // spend budget, quote-then-run, and the registry route's (disabled) status.
            // D-11 — cloud providers: status, usage (signed log), caps, halt/resume, one-way keys,
            // gated custom endpoints. No command returns a key.
            providers_ipc::providers_status,
            providers_ipc::providers_usage_summary,
            providers_ipc::providers_caps_set,
            providers_ipc::providers_price_set,
            providers_ipc::providers_halt,
            providers_ipc::providers_resume,
            providers_ipc::providers_key_set,
            providers_ipc::providers_key_test,
            providers_ipc::providers_key_remove,
            providers_ipc::providers_custom_add,
            providers_ipc::providers_custom_confirm,
            providers_ipc::providers_custom_remove,
            providers_ipc::providers_usage_log_reset,
            providers_ipc::providers_preferred_get,
            providers_ipc::providers_preferred_set,
            // D-11 (P-8) — the Cognition delegate: per-task approval of the exact scope.
            cognition::delegation_prepare,
            cognition::delegation_decline,
            cognition::delegation_approve,
            cognition::delegation_status,
            escalation::escalation_endpoints,
            escalation::escalation_endpoint_add,
            escalation::escalation_endpoint_remove,
            escalation::escalation_budget,
            escalation::escalation_budget_set,
            escalation::escalation_quote,
            escalation::escalation_confirm_prepare,
            escalation::escalation_run,
            hic_records::hic_record_decision,
            // The decision records read back, newest first (Hermes › Controls › Records).
            hic_records::hic_records_list,
            escalation::escalation_registry_status,
            inference_router::escalation_registry_quote,
            inference_router::escalation_registry_request,
            inference_router::escalation_registry_result,
            inference_router::escalation_registry_mine,
            inference_router::escalation_registry_claim_refund,
            inference_router::escalation_registry_expire,
            web_budgets::web_budget_status,
            web_budgets::web_budget_grant,
            web_budgets::web_budget_revoke,
            web_budgets::web_budget_revoke_all,
            web_budgets::web_budget_reset,
            web_budgets::web_signing_request,
            web_budgets::web_signing_approve,
            web_budgets::web_signing_reject,
            telemetry::diagnostics_bundle,
            telemetry::telemetry_send,
            skill_registry::skills_registry_list,
            storage::storage_add,
            storage::storage_list,
            storage::storage_retrieve,
            storage::storage_unpin,
            // K4: local pins (no bond), the IPFSIncentivesV3 parameters, and verified backup.
            storage::storage_pin_local,
            storage::storage_pin_params,
            storage_backup::storage_backup_export,
            storage_backup::storage_backup_verify,
            storage_backup::storage_backup_restore,
            hermes::personas::hermes_personas,
            hermes::personas::hermes_workflows,
            hermes::personas::hermes_persona_check,
            hermes::personas::hermes_track_workflow_run,
            hermes::mcp_cards::hermes_mcp_pending,
            hermes::mcp_cards::hermes_mcp_decide,
            hermes::workers::hermes_workers,
            hermes::decide::hermes_decide_stats,
            hermes_learn::hermes_workflow_run,
            hermes_learn::hermes_workflow_status,
            hermes_learn::hermes_learn_status,
            hermes_learn::hermes_learn_proposals,
            hermes_learn::hermes_learn_propose,
            hermes_learn::hermes_learn_accept,
            hermes_learn::hermes_learn_reject,
            hermes_learn::hermes_learn_memories,
            hermes_learn::hermes_learn_store_pending,
            hermes_learn::hermes_learn_resolve,
            hermes_learn::hermes_learn_publish,
            hermes_web::hermes_web_settings_get,
            hermes_web::hermes_web_settings_set,
            chat_consent::chat_consent_prepare,
            chat_consent::chat_consent_drop,
            chat_consent::media_consent_prepare,
            chat_route::chat_route_get,
            chat_route::chat_route_set,
            // WP-H7 / V6 (0.5.4): the on-device private-information scanner.
            pii_scan::pii_scan_text,
            // node — the real citrate-node under the SidecarSupervisor (C1.1).
            // Replaces the A1.3 seam stubs: node_status returns REAL height/peers
            // from the node's local RPC; node_start spawns the node with an
            // encrypted data dir (@rule8 keyring storage key); node_stop releases
            // the supervisor (SIGTERM→grace→SIGKILL, no orphan).
            node::node_status,
            node::node_start,
            node::node_stop,
            // W1.3 — activate the member's validator bond (bond-clone model): the
            // ceremony sends MemberBond.activate to the funded clone; the clone (=
            // staker) forwards its principal to registerValidator.
            node::node_register_validator,
            // Join-8 — the Activate-validator onboarding step's one read (pure, no signing).
            validator_activation::validator_activation_status,
            // Join-8 — Forget an unseen activation send in the durable submitted-tx record.
            validator_activation::validator_activation_forget,
            // W1.x — arm the producer once synced (consensus-gated); the respawn
            // mints proposer.key, the validator identity the bond activation needs.
            node::node_arm_mining,
            // W1.4 — the REAL validator earnings: rewardsOf(proposerPubkey) on the
            // ValidatorRegistry (the correct block-subsidy source, not the old
            // ContributionAccounting.claimable read).
            node::node_validator_earnings,
            // Watchdog: why did the supervised node stop? (crash-records.jsonl tail)
            node::node_last_crash,
            // Real device fingerprint (hash of the device-bound custody pubkey).
            provisioning::device_id,
            // node logs — Q-A.2/Q-B.2 REAL streamed stdout+stderr from the
            // supervised node's bounded ring buffer. Fills the Node LOG panel in
            // a packaged build (was permanently empty: stdout was inherited then
            // dropped in the GUI process). Honest empty when the node is off; no
            // fabricated template ever crosses this boundary (Rule 1).
            node::node_logs,
            // CORE-N3 follow-up: Remove old network data (chain.<profile>.bak in the node dir).
            node::node_old_network_data,
            node::node_remove_old_network_data,
            // node-agent — the node-agent under the SidecarSupervisor (C1.2).
            // agent_status returns the supervisor state + whether a bearer
            // session exists (NEVER the token); agent_start spawns the daemon
            // with a minted 0600 bearer file (@rule8); agent_stop releases the
            // supervisor + wipes the session bearer. Signature requests bridge
            // through the ceremony only (no direct-sign path — ADV-7).
            agent::agent_status,
            agent::agent_start,
            agent::agent_stop,
            // user_claim (C2-F-1, @rule8) — the USER Claim button. Reads the REAL
            // claimable (eth_call), and either returns an HONEST "nothing to claim"
            // for 0, or bridges the real claimRewards() intent into a PENDING
            // ceremony the human approves via sign_and_broadcast (B1.4). NEVER a sim
            // balance mutation presented as a settled chain claim (Rule 1).
            agent::user_claim,
            // memory — the real citrate-memories mcp_serve daemon under the
            // SidecarSupervisor (C3). Replaces the A1.3 memory_recall seam stub:
            // memory_status returns the supervisor state + socket path (never the
            // store key); memory_start spawns the daemon (a keyring wrapping key is
            // minted for forward-compat, but the store is NOT encrypted at rest
            // today — plaintext on a fresh store; see memory.rs honest residual);
            // recall/search/neighbors speak JSON-RPC over the daemon's Unix
            // socket; constellation feeds the Storage graph with REAL nodes.
            memory::memory_status,
            memory::memory_start,
            memory::memory_stop,
            memory::memory_recall,
            memory::memory_search,
            memory::memory_neighbors,
            // W3.2 — first-run docs preload into the citrate-docs tenant (gated +
            // idempotent; no-op until the corpus is curated + BGE is wired).
            memory::memory_ingest_docs,
            // Seed the constellation tenants (chain-state + personal) with real network/node/stake
            // facts on daemon-connect, so the graph has content on open (gated + idempotent).
            memory::memory_seed_context,
            // HUP-S3.1 — first-run import of the bundled knowledge corpus (verified by the
            // importer against its manifest; idempotent; progress on memory://knowledge-import-progress).
            knowledge_import::memory_import_knowledge,
            memory::memory_constellation,
            // Hermes "remember this fact": the approved memory_assert card writes one
            // fact into the personal tenant (a local mem-dag write, not a signature).
            memory::memory_assert,
            // earnings — the REAL on-chain claimable read (C2). agent_earnings
            // reads ContributionAccounting.claimable(vaultAddress) via eth_call on
            // 40204 (Rule 1: the single real value; no sim per-source breakdown).
            // A claim is a SIGNED value-bearing write that routes through the
            // SignatureCeremony (agent bridge → B1.4), never signed here (@rule8).
            earnings::agent_earnings,
            // wallet provisioning (@rule8) — seamless device-bound init+unlock of
            // the custody vault (auto passphrase in the OS keyring, no user
            // passphrase) + silent first-run wallet mint. Returns ONLY the public
            // address (never key/seed). Idempotent; the onboarding link calls it
            // after sign-in and before the membership grant so the REAL device
            // wallet is what gets linked + paid. Signs nothing (Rule 3 untouched).
            provisioning::wallet_ensure_ready,
            // wallet balances — REAL liquid (eth_getBalance) + claimable read.
            earnings::wallet_balances,
            // wallet send — a native SALT transfer bridged into a PENDING ceremony
            // (@rule8; signs nothing — the human approves via sign_and_broadcast).
            transfer::wallet_send,
            skills_local::skills_local_list,
            skills_local::skills_local_write,
            skills_local::skills_local_read,
            skills_local::skills_local_delete,
            skills_local::skills_local_migrate,
            // HUP-S10.4 — journal encrypted export/import (passphrase-sealed file; plaintext never on disk).
            journal_export::journal_export_encrypted,
            journal_export::journal_import_encrypted,
            // HUP-S10.3 — daemons: scheduled Hermes tasks inside a budget.
            daemons::daemons_list,
            daemons::daemon_save,
            daemons::daemon_set_paused,
            daemons::daemons_set_all_paused,
            daemons::daemon_delete,
            daemons::daemons_claim_due,
            daemons::daemons_finish_run,
            daemons::daemon_runs_between,
            // HUP-S10.3 — widgets: sandboxed tiles with a read-only bridge.
            widgets::widgets_list,
            widgets::widget_save,
            widgets::widget_delete,
            widgets::widget_source,
            // HUP-S10.2 — Hermes's own schedule (local), shown as a calendar.
            hermes_schedule::hermes_schedule_list,
            hermes_schedule::hermes_schedule_add,
            hermes_schedule::hermes_schedule_remove,
            hermes_schedule::hermes_schedule_set_enabled,
            hermes_schedule::hermes_schedule_reset,
            hermes_schedule::hermes_schedule_due,
            // HUP-S5.5 — signed first-run components. Update refuses until the component key is set.
            components::components_status,
            components::components_update,
            components::components_rollback,
            // K1a-2 — the services registry snapshot Node › Services reads (read-only).
            services::services_snapshot,
            // CORE-D4 — every sidecar's health: state, counters, last crash, slow retry, log tail.
            services::sidecars_health,
            // CORE-D6: the pre-update drain (hold starts, stop every sidecar) and its cancel.
            update_drain::update_prepare,
            update_drain::update_prepare_cancel,
            update_drain::update_install_begin,
            // 0.5.4: the signed minimum version the update gate reads.
            update_policy::update_policy_apply,
            update_policy::update_policy_status,
            update_policy::update_policy_refresh,
            // CORE-D5: the scrubbed diagnostics bundle, written to a file the member chose.
            sidecar_logs::diagnostics_logs_export,
            // One service's log file, read back for Node › Advanced (scrubbed again).
            sidecar_logs::sidecar_logs_tail,
            // HUP-S10.5 — device-key recovery kit + "delete my local data".
            recovery_kit::recovery_kit_status,
            recovery_kit::recovery_kit_save_phrase,
            recovery_kit::recovery_kit_save_file,
            recovery_kit::recovery_kit_restore_phrase,
            recovery_kit::recovery_kit_restore_file,
            local_data::local_data_plan,
            local_data::local_data_delete,
            // HUP-S7.4 — Hermes identity: AgentSBT status read + the onboarding mint (a pending
            // SignatureCeremony; signs nothing here).
            agent_sbt::agent_sbt_status,
            agent_sbt::agent_sbt_mint,
            // IPFS — the bundled kubo daemon (artifact/model pin/add on 127.0.0.1:5001). Started
            // here; it stops with the app.
            ipfs::ipfs_start,
            // open an external federation link (https only) in the system browser.
            shell::open_external,
            // wallet activity — REAL indexed 40204 tx history from the CitrateScan
            // `txlist` endpoint (public, no-auth read; source: citrate-explorer
            // /api/v1?module=account&action=txlist). Honest empty on a fresh /
            // not-provisioned index; never fabricated (Rule 1). NOT @rule8.
            activity::wallet_activity,
            popout::popout_open,
            popout_contract::popout_contract_send,
            popout_contract::popout_contract_take,
            popout::popout_monitor_facts,
            browser::hermes_browser_status,
            browser::hermes_browser_frame,
            browser::hermes_browser_stop,
            browser::hermes_browser_resume,
            browser::hermes_browser_attach,
            browser::hermes_browser_detach,
            browser::hermes_browser_origin,
            browser::hermes_browser_decide,
            // HUP-S7.3 + S7.5 — nightly anchor (off until AnchorRegistry is deployed; the anchor
            // key signs only inside the anchor ceremony) + the daily metering report.
            chain_agent::hermes_chain_status,
            chain_agent::hermes_chain_settings_set,
            chain_agent::hermes_metering_daily,
            chain_agent::hermes_anchor_approve,
            chain_agent::hermes_anchor_reject,
            // HUP-S7.3 + S7.5 (rest): prove a past decision (core checks the proof and reads
            // AnchorRegistry itself) and opt-in benchmark sharing (one wallet card per metric).
            anchor_proof::hermes_anchor_records,
            anchor_proof::hermes_anchor_proof,
            benchmark_share::hermes_benchmark_share,
        ])
    };
}
