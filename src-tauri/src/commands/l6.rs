//! The app commands lane L6 registers (connections and marketplace).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L6 edits this file. A command added here is also allowed in
//! `permissions/l6-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l6 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // W4 — MCP connections (OAuth). connection_start runs the loopback-PKCE
            // flow in the system browser and seals the token in the vault;
            // connection_status/disconnect read/forget it. NO command returns a
            // token or client secret (I-2 barrier; see connections::tests).
            connections::connection_start,
            connections::connection_status,
            connections::connection_disconnect,
            // Social identity (Connections · social discovery, ADR-2026-08-30). Public-client PKCE
            // link; token seals in the keyring, device-local binding store; NO command returns a
            // token. `verified` (the wallet-signed IdentityBinding) is a follow-up.
            social::social_status,
            social::social_start,
            social::social_set_visibility,
            social::social_disconnect,
            // Social verify (D3) — the wallet signs the IdentityBinding at the ceremony (Rule 3);
            // request opens it, approve records the binding + flips verified, forget drops a reject.
            social::social_verify_request,
            social::social_verify_approve,
            social::social_verify_forget,
            // Resolver: verified + group-visible addresses → faces.
            social::social_resolve,
            // D1 server-blind share: export a verified binding to ride the group relay; ingest a
            // peer's (recover-verified before trusting) so cross-member faces resolve.
            social::social_export_binding,
            social::social_ingest_binding,
            // #61 — self-published bindings directory (find-via-X): opt-in publish/revoke go through
            // the SignatureCeremony (request → approve); lookup/search are Bearer-gated authority reads.
            social::directory_publish_request,
            social::directory_publish_approve,
            social::directory_unpublish_request,
            social::directory_unpublish_approve,
            social::directory_forget,
            social::directory_lookup,
            social::directory_search,
            // HUP-S10.2 — Google Sheets + Calendar through Connections (disabled until configured).
            google_workspace::google_workspace_status,
            google_workspace::gsheets_read,
            google_workspace::gsheets_append,
            google_workspace::gcal_list,
            google_workspace::gcal_create,
            // K5a — Store v1: signed catalog, staged installs that arrive paused, one-call
            // removal. Every catalog command refuses until the catalog key is issued.
            store_catalog::store_catalog,
            store_catalog::store_stage,
            store_catalog::store_install,
            store_catalog::store_installed,
            store_catalog::store_remove,
            // HUP-S4.4 — user-added MCP servers (Settings > MCP servers): stored disabled, enabled
            // only after a dry-run review; writes the allowlist the Hermes sidecar reads.
            mcp_servers::mcp_servers_list,
            mcp_servers::mcp_server_save,
            mcp_servers::mcp_server_remove,
            mcp_servers::mcp_server_disable,
            mcp_servers::mcp_server_review,
            mcp_servers::mcp_server_enable,
            mcp_servers::mcp_servers_runtime,
            // L6-WP07: the Marketplace MCP catalog. The verified brand servers pinned in the binary;
            // the signed section says "signing key not issued" until the key ceremony.
            lanes::l6::mcp_catalog::mcp_catalog_list,
        ])
    };
}
