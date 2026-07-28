//! citrate-core — the federation desktop house (Tauri full-node app).
//!
//! CORE-A1 bridge: the `config` domain is wired end-to-end for real (persisted
//! on-disk via tauri-plugin-store + OS-keyring status). Every other bridge
//! domain is registered but returns an honest `Unavailable` error — never
//! fabricated data (Rule 1). Chain reads happen in the webview via viem/wagmi
//! (D-13); signing routes through the Rust SignatureCeremony once it exists
//! (CORE-S2).

// WP-S1.2: the safety-critical spine (ceremony, custody, oidc, supervisor,
// wallet, rpc, txdecode, config) now lives in `citrate-core-kit`, shared with
// citrate-quorum so there is ONE signing path in the federation, not a fork.
// Re-exported here so every in-crate reference (`crate::custody::…`,
// `crate::ceremony::…`, and the `generate_handler!` command paths below) keeps
// resolving unchanged — the extraction is invisible above this line.
pub use citrate_core_kit::{
    ceremony, config, custody, oidc, rpc, supervisor, txdecode, wallet, wallet_link,
};

mod addresses;
mod activity;
mod agent;
mod ai;
mod connections;
mod docs_ingest;
mod earnings;
mod grant_status;
mod ipfs;
mod membership;
mod memory;
mod model;
mod node;
mod sbt_art;
mod seam;
mod serve;
mod shell;
mod staking;
mod transfer;
mod validator;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        // W2.1 — in-app auto-updates (signed GitHub Releases feed; pubkey pinned in
        // tauri.conf.json). The JS API drives check/download/install via this plugin.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // W2.4 — relaunch into the freshly-installed version.
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // CORE-A2 — build the process-wide custody vault (real OS keyring +
            // app-data envelope), seeded with the persisted config.autolock (the
            // A1 single source of truth). @rule8: no secret bytes cross invoke.
            let handle = app.handle();
            let autolock = config::config_read(handle.clone())
                .map(|c| c.autolock)
                .unwrap_or_else(|_| config::AppConfig::default().autolock);
            let state = custody::build_custody_state(handle, autolock)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(state);
            // CORE-A3 — the auth manager (real loopback-PKCE OIDC against
            // auth.citrate.ai; refresh token → the A2 vault; access token in
            // memory only; @rule8: no token crosses invoke). Consumes A2.
            app.manage(oidc::build_auth_state());
            // CORE-B1.2 — the SignatureCeremony: the SINGLE human-in-the-loop
            // signing path. Every signature intent (user or, later, agent /
            // micro-app) routes through one approval surface; the gated
            // `wallet::sign_message` is reachable ONLY from `approve`. @rule8:
            // this LIFTS Rule 3 — all signing goes through this ceremony. No
            // secret bytes cross invoke (sign_* return id / decoded / sig-hex).
            app.manage(ceremony::build_ceremony_state());
            // Wallet-link — bind THIS device's custody EOA to the member's Citrate
            // identity, through the ceremony above. Until a wallet is bound the
            // authority's `wallet_address` claim is the counterfactual smart-wallet
            // address, which no key can spend from — so the membership money path
            // would bond-fund an address the member cannot reach.
            app.manage(wallet_link::build_link_state());
            // CORE-C1.1 — the NodeManager: the real citrate-node under the
            // SidecarSupervisor with an encrypted data dir. @rule8: the 32-byte
            // storage master key lives in the OS keyring (never on disk clear)
            // and is handed to the spawned node via the CITRATE_STORAGE_KEY env.
            // node_status reads REAL height/peers from the node's local RPC.
            app.manage(node::build_node_state(&app.handle().clone())?);
            // CORE-C1.2 — the AgentManager: the node-agent under the
            // SidecarSupervisor with a bearer-authed supervision surface. @rule8:
            // the per-session bearer is minted with OsRng, handed to the child via
            // a 0600 token file (the grounded IPC channel), never logged/Debug'd,
            // and zeroized on stop. The node-agent's UNSIGNED SignatureRequests
            // route ONLY through the SignatureCeremony (origin "agent:node-agent")
            // — it holds no keys and can never sign directly (ADV-7).
            app.manage(agent::build_agent_state(&app.handle().clone())?);
            // CORE-C3 — the MemoryManager: the citrate-memories `mcp_serve`
            // daemon under the SidecarSupervisor, serving the per-user encrypted
            // memory graph over a Unix socket. @rule8: a per-user store wrapping
            // key lives in the OS keyring (never on disk clear) and is handed to
            // the daemon via the CITRATE_MEM_STORE_KEY env (forward-compat seam —
            // see memory.rs / sprint Concern 1). recall/search/neighbors speak the
            // daemon's JSON-RPC over the socket; the Storage constellation renders
            // the real graph (no fabricated nodes — Rule 1).
            app.manage(memory::build_memory_state(&app.handle().clone())?);
            // CORE-AI1 — the AiManager: real OpenAI-compatible inference with the
            // provider API key custodied in the OS keyring (service
            // "ai.citrate.core"). @rule8 (key custody + secret network egress): NO
            // command returns the key or the Authorization header (ai_provider_status
            // returns only {id,baseURL,model,configured}); the key is BOUND to its
            // https baseURL at set-time and ai_chat calls the STORED baseURL only —
            // the webview picks WHICH provider id, never the URL (exfil-binding).
            app.manage(ai::build_ai_state());
            // CORE-BC-3.1 — the ModelManager: the local Gemma GGUF download +
            // SHA-256 verify. model_status is honest (Ready ONLY after a real
            // verify — never mere presence, Rule 1); model_download is STREAMED +
            // resumable (HTTP Range) with a GGUF-magic gate; model_verify streams
            // the file through SHA-256 and quarantines any mismatch. No secrets.
            app.manage(model::build_model_state(&app.handle().clone())?);
            // CORE-BC-3.2 — the LlamaServerManager: the bundled `llama-server`
            // sidecar under the SidecarSupervisor, serving the verified local model
            // over an OpenAI-compatible loopback endpoint. model_serve_start fails
            // CLOSED unless the model is verified-Ready AND the binary is bundled
            // (WO-2 packaging gap surfaced honestly). ai.rs routes to the LOCAL
            // provider when the model is ready + the server healthy, else the
            // gateway (if a cgk_ key is configured), else the demo.
            app.manage(serve::build_serve_state(&app.handle().clone())?);
            // W4 — MCP connections (Google Drive / Notion / GitHub OAuth). The
            // loopback-PKCE flow + fixed-port callback + vaulted token custody. No
            // command returns a token; secrets are sealed in the custody vault.
            app.manage(connections::build_connection_state());
            // IPFS — the bundled kubo daemon under the SidecarSupervisor. The
            // `citrate` node reaches it on 127.0.0.1:5001 for artifact + model
            // pin/add/ls (block production does NOT need it). Repo lives in the app
            // data dir; started on demand via ipfs_start (alongside the node).
            app.manage(ipfs::build_ipfs_state(&app.handle().clone())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // config — the one genuinely-live domain (A1.4)
            config::config_read,
            config::config_write,
            config::config_keyring_status,
            // custody — the OS-keyring vault (A2). NOTE: custody_get is NOT here;
            // it is an in-process pub fn — no invoke command returns secret bytes.
            custody::custody_status,
            custody::custody_init,
            custody::custody_unlock,
            custody::custody_lock,
            custody::custody_put,
            custody::custody_list,
            custody::custody_keyring_status,
            // auth — real OIDC loopback-PKCE (A3). Replaces the A1 auth seam
            // stubs. Every command returns claim-derived AuthStatus or (); NO
            // command returns a token (ADV-8 boundary; see oidc::tests).
            oidc::auth_status,
            oidc::auth_login,
            oidc::auth_userinfo,
            oidc::auth_refresh,
            oidc::auth_logout,
            oidc::kyc_start,
            // W4 — MCP connections (OAuth). connection_start runs the loopback-PKCE
            // flow in the system browser and seals the token in the vault;
            // connection_status/disconnect read/forget it. NO command returns a
            // token or client secret (I-2 barrier; see connections::tests).
            connections::connection_start,
            connections::connection_status,
            connections::connection_disconnect,
            // membership — the D3.C checkout popup (@rule8 money seam). Opens the
            // REAL core-membership checkout ({coreMembershipUrl}/checkout) in an
            // in-app popup with the SAME isolation as the auth popup (a remote
            // HTTPS page, NO capability/IPC). It NEVER signs or reports a settled
            // membership — the money + grant are server-side; the store polls
            // /userinfo and only advances S3 when the REAL entitlement lands.
            membership::membership_checkout,
            // membership grant status — BC-1.3 (@rule8 · T1 money-path READ). Reads
            // the REAL on-chain grant: MembershipStakeVault.attributedStake +
            // attributedShares(member) and CitrateMemberSBT.balanceOf(member) via
            // eth_call on 40204. The S5 grant+stake ceremony settles ONLY from these
            // real reads (Rule 1 — no fabricated settlement). A PURE READ: it signs
            // nothing and returns only decoded public chain data (@rule8 / Rule 3).
            grant_status::membership_grant_status,
            // SBT emblem — BC-5.3 (T1 identity READ). Reads the AUTHORITATIVE
            // wholly-on-chain member emblem: isSubBound(keccak256(sub)) ->
            // tokenIdForSub -> tokenURI on CitrateMemberSBT (40204), decoded to the
            // `data:image/svg+xml;base64,...` the UI renders — or None honestly when
            // the member has no SBT (Rule 1 — no fabricated art). A PURE READ: it
            // signs nothing and touches no key material (@rule8 / Rule 3).
            sbt_art::sbt_token_uri,
            // signing — the B1.2 SignatureCeremony (the ONE HITL signing path).
            // sign_request returns a CeremonyId + decoded intent (NO signature);
            // sign_approve returns the signature hex ONLY (never key/seed/entropy
            // — I-2 compile barrier holds); sign_reject consumes with no sig. The
            // gated wallet::sign_message is reachable ONLY via approve (@rule8).
            ceremony::sign_request,
            ceremony::sign_approve,
            ceremony::sign_and_broadcast,
            ceremony::sign_reject,
            wallet_link::wallet_link_request,
            wallet_link::wallet_link_approve,
            wallet_link::wallet_link_reject,
            // node — the real citrate-node under the SidecarSupervisor (C1.1).
            // Replaces the A1.3 seam stubs: node_status returns REAL height/peers
            // from the node's local RPC; node_start spawns the node with an
            // encrypted data dir (@rule8 keyring storage key); node_stop releases
            // the supervisor (SIGTERM→grace→SIGKILL, no orphan).
            node::node_status,
            node::node_start,
            node::node_stop,
            // W1.1 — the node's proposer identity (coinbase + derived ed25519
            // proposer pubkey) for the validator status surface + registration.
            node::node_proposer_identity,
            // W1.3 — register the member's node as a block-producing validator
            // (registerValidator{value:32k} via the ceremony; staker = the EOA).
            node::node_register_validator,
            // W1.4 — the REAL validator earnings: rewardsOf(proposerPubkey) on the
            // ValidatorRegistry (the correct block-subsidy source, not the old
            // ContributionAccounting.claimable read).
            node::node_validator_earnings,
            // node logs — Q-A.2/Q-B.2 REAL streamed stdout+stderr from the
            // supervised node's bounded ring buffer. Fills the Node LOG panel in
            // a packaged build (was permanently empty: stdout was inherited then
            // dropped in the GUI process). Honest empty when the node is off; no
            // fabricated template ever crosses this boundary (Rule 1).
            node::node_logs,
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
            memory::memory_constellation,
            // seam domains — honest Unavailable until each later phase (A1.3).
            // memory_assert stays a seam stub: the assert WRITE path routes
            // through the SignatureCeremony (a later WP), not C3's read wiring.
            // earnings — the REAL on-chain claimable read (C2). agent_earnings
            // reads ContributionAccounting.claimable(vaultAddress) via eth_call on
            // 40204 (Rule 1: the single real value; no sim per-source breakdown).
            // A claim is a SIGNED value-bearing write that routes through the
            // SignatureCeremony (agent bridge → B1.4), never signed here (@rule8).
            earnings::agent_earnings,
            // wallet balances — REAL liquid (eth_getBalance) + claimable read.
            earnings::wallet_balances,
            // wallet send — a native SALT transfer bridged into a PENDING ceremony
            // (@rule8; signs nothing — the human approves via sign_and_broadcast).
            transfer::wallet_send,
            // wallet stake — a LiquidStakingPool deposit() bridged into a PENDING
            // ceremony (@rule8; signs nothing — approved via sign_and_broadcast).
            // The self-stake balance is read within wallet_balances (balanceOf).
            staking::wallet_stake,
            // wallet withdraw (WP2, @rule8) — the two-step, ~7-day-queued withdraw
            // of self-added stake. request_withdrawal burns stSALT shares (the
            // SALT→shares conversion is done from LIVE shares()+balanceOf() reads,
            // never fabricated); claim_withdrawal pays out a matured request. Both
            // sign NOTHING — the human approves via sign_and_broadcast (B1.4).
            // pending_withdrawals enumerates the wallet's real on-chain queue
            // (getLogs WithdrawalRequested + withdrawals(id) + block_number).
            staking::wallet_request_withdrawal,
            staking::wallet_claim_withdrawal,
            staking::wallet_pending_withdrawals,
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
            // model — BC-3.1 local Gemma download + verify. model_status is the
            // honest file-derived state (Ready ONLY after a real SHA-256 verify —
            // never mere presence, Rule 1); model_download is STREAMED + resumable
            // (HTTP Range) with a GGUF-magic gate + pinned-length finalize;
            // model_verify quarantines any checksum/size mismatch (never Ready).
            // No secrets, no key material cross this surface.
            model::model_status,
            model::model_download,
            model::model_verify,
            // model-serve — BC-3.2 the bundled llama-server sidecar under the
            // SidecarSupervisor. model_serve_start fails CLOSED unless the model is
            // verified-Ready AND the binary is bundled; model_serve_status carries
            // the loopback baseURL + a coarse health flag; model_inference_state
            // returns the HONEST route (ready/local-fallback/downloading/
            // gateway-only/no-model/demo). No secret crosses this surface.
            serve::model_serve_start,
            serve::model_serve_stop,
            serve::model_serve_status,
            serve::model_inference_state,
            // IPFS — the bundled kubo daemon (artifact/model pin/add on 127.0.0.1:5001).
            ipfs::ipfs_start,
            ipfs::ipfs_stop,
            ipfs::ipfs_status,
            // open an external federation link (https only) in the system browser.
            shell::open_external,
            // wallet activity — REAL indexed 40204 tx history from the CitrateScan
            // `txlist` endpoint (public, no-auth read; source: citrate-explorer
            // /api/v1?module=account&action=txlist). Honest empty on a fresh /
            // not-provisioned index; never fabricated (Rule 1). NOT @rule8.
            activity::wallet_activity,
            // seam domains — honest Unavailable until each later phase (A1.3)
            seam::memory_assert,
            seam::chat_backend,
            seam::membership_entitlement,
            seam::commissary_catalog,
            seam::comms_connections,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// The Tauri config must parse and carry the citrate-core identity.
    /// Guards against template cruft ("cta-citrate-core") reappearing.
    #[test]
    fn tauri_conf_parses_with_citrate_identity() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must be valid JSON");
        assert_eq!(conf["identifier"], "ai.citrate.core");
        assert_eq!(conf["productName"], "Citrate Core");
    }

    /// tauri.conf.json version and the Cargo package version must not drift.
    #[test]
    fn tauri_conf_version_matches_cargo_version() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must be valid JSON");
        assert_eq!(conf["version"], env!("CARGO_PKG_VERSION"));
    }

    /// ADV-8 boundary, asserted structurally against the real source: the
    /// invoke_handler registers exactly the seven metadata/status custody
    /// commands and NEVER `custody_get` (the in-process secret-reading pub fn).
    /// No invoke command returns secret bytes.
    #[test]
    fn no_custody_invoke_command_returns_secret_bytes() {
        let src = include_str!("lib.rs");
        // The registered custody commands (all return status / () / metadata).
        for cmd in [
            "custody::custody_status",
            "custody::custody_init",
            "custody::custody_unlock",
            "custody::custody_lock",
            "custody::custody_put",
            "custody::custody_list",
            "custody::custody_keyring_status",
        ] {
            assert!(src.contains(cmd), "custody command not registered: {cmd}");
        }
        // The secret-reading API must NEVER be registered as an invoke command.
        // Registration would take the form `<mod>::<fn>,` inside the
        // generate_handler![...] list. The needle is assembled from parts so
        // this test's own prose (which names the fn) cannot trip the check; the
        // only way it matches is a genuine handler-registration line.
        let getter = "custody_g".to_string() + "et";
        let needle = format!("custody::{getter},");
        assert!(
            !src.contains(&needle),
            "the in-process secret getter must not be an invoke command (ADV-8 boundary)"
        );
    }

    /// B1.1-ADV-2, relocated here in the WP-S1.2 kit extraction (was in the
    /// kit's `wallet_tests.rs`, which after the split could only see the kit's
    /// lib.rs — the real `generate_handler!` registry lives in THIS file). The
    /// wallet secret-touching fns (create / import / address / sign_message) are
    /// plain library fns and must NEVER be registered as invoke commands.
    /// NEGATIVE CONTROL: registering any of those fns as a handler entry (a
    /// `wallet::<fn>` line ending in a comma, in the generate_handler! list)
    /// fails this test. Needles are assembled from parts so this test's own
    /// prose cannot self-match `include_str!("lib.rs")` — do not write a literal
    /// `wallet` + `::` + fn-name + comma anywhere in this file's prose.
    #[test]
    fn no_wallet_secret_path_fn_is_an_invoke_command() {
        let src = include_str!("lib.rs");
        let w = "wallet::".to_string();
        for suffix in [
            "create",
            "import",
            "sign_message",
            "address",
            "read_entropy",
            "derive_key_from_entropy",
        ] {
            let needle = format!("{w}{suffix},");
            assert!(
                !src.contains(&needle),
                "no wallet secret-path fn may be an invoke command: wallet::{suffix}"
            );
        }
    }

    /// Command-registration checks relocated here in the WP-S1.2 kit extraction:
    /// the `generate_handler!` registry lives in THIS crate's lib.rs, so the
    /// "these commands are wired" assertions (formerly in the kit's oidc/ceremony
    /// tests, which after the split could only see the kit lib.rs) belong here.
    /// The kit-side tests retain their kit-SOURCE structural assertions
    /// (AuthStatus carries no token field, return types, Signature is secret-free).
    #[test]
    fn auth_commands_are_registered() {
        let src = include_str!("lib.rs");
        for cmd in [
            "auth_status",
            "auth_login",
            "auth_userinfo",
            "auth_refresh",
            "auth_logout",
            "kyc_start",
        ] {
            assert!(
                src.contains(&format!("oidc::{cmd}")),
                "auth command must be registered: oidc::{cmd}"
            );
        }
    }

    #[test]
    fn signing_commands_are_registered() {
        let src = include_str!("lib.rs");
        for cmd in [
            "sign_request",
            "sign_approve",
            "sign_and_broadcast",
            "sign_reject",
        ] {
            assert!(
                src.contains(&format!("ceremony::{cmd}")),
                "signing command must be registered: ceremony::{cmd}"
            );
        }
    }
}
