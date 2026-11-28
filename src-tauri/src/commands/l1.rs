//! The app commands lane L1 registers (agent runtime).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L1 edits this file. A command added here is also allowed in
//! `permissions/l1-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l1 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            hermes::hermes_start,
            hermes::hermes_status,
            // 0.5.4: the Hermes setup self-check.
            hermes_setup::hermes_setup_check,
            hermes_setup::hermes_setup_repair,
            hermes_setup::hermes_setup_enable_node_tools,
            hermes::hermes_skills,
            hermes::hermes_run_skill,
            hermes::hermes_pending_approvals,
            hermes::hermes_stop,
            hermes::hermes_session_open,
            // D-11 (P-5, P-6) — the agent loop on a cloud model through the core proxy.
            hermes::hermes_session_open_cloud,
            hermes::hermes_session_send,
            hermes::hermes_session_events,
            hermes::hermes_session_tool_result,
            hermes::hermes_session_stop,
            // HUP-S10.3 — daemon runs: unattended sessions, closed when the run ends.
            hermes::hermes_session_open_unattended,
            hermes::hermes_session_close,
            hermes::hermes_tracks,
            hermes::hermes_brief_create,
            hermes::hermes_brief_check,
            hermes::hermes_bridge_pending,
            hermes::hermes_resolve,
            hermes::hermes_resolve_outcome_unknown,
            // HUP-S4.3 — the MCP servers Hermes may use (mem-mcp, CitrateScan); default off.
            hermes_mcp::hermes_mcp_settings,
            hermes_mcp::hermes_mcp_set,
            hermes_mcp::hermes_node_mcp_trusted,
            // ai — real OpenAI-compatible inference (AI1, @rule8). The provider key
            // is sealed in the OS keyring, BOUND to its https baseURL at set-time;
            // ai_provider_status returns only {id,baseURL,model,configured} (never
            // the key); ai_chat reads the STORED baseURL for the given provider id
            // and POSTs /v1/chat/completions with the sealed Bearer — the webview
            // picks WHICH provider, never the URL (exfil-binding). Errors are coarse
            // + secret-free.
            ai::ai_set_provider,
            ai::ai_provider_status,
            ai::ai_clear_provider,
            ai::ai_chat,
            // W3.3 — one agentic turn: gateway completion WITH tools; returns the
            // assistant message (content and/or tool_calls). The webview loops.
            ai::ai_chat_tools,
            // BC-3.2 — LOCAL inference against the bundled llama-server on the
            // loopback endpoint (NO key). Fails closed on any non-loopback URL.
            ai::ai_chat_local,
            ai::ai_chat_local_tools,
            // HUP-S2.1 — Hermes folder grants (store in app data; sent to agent sessions on change).
            agent_grants::agent_grants_view,
            agent_grants::agent_grants_add_folder,
            agent_grants::agent_grants_revoke,
            agent_grants::agent_grants_full_access_prepare,
            agent_grants::agent_grants_full_access_confirm,
            agent_grants::agent_grants_reset,
            // HUP-S4.2 — citrate-node MCP server (Settings, API endpoints & keys).
            node_mcp::node_mcp_status,
            node_mcp::node_mcp_set_enabled,
            node_mcp::node_mcp_token_create,
            node_mcp::node_mcp_token_revoke,
            node_mcp::node_mcp_requests,
            node_mcp::node_mcp_decide,
            // L1-WP14b (v060) — per-agent sign-in homes: status, sign-out, uninstall cleanup.
            hermes::agent_auth::agent_runtime_auth_status,
            hermes::agent_auth::agent_runtime_sign_out,
            hermes::agent_auth::agent_runtime_remove,
        ])
    };
}
