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
/// The keyring namespace citrate-core custody owns.
///
/// MUST differ from `OsKeyring::LEGACY_SERVICE`: the kit is shared with
/// citrate-quorum, which owns the legacy namespace and has a live vault sealed
/// there. Two apps on one service share the custody anchor while keeping
/// separate envelopes, which locks the second app out permanently with
/// `Corrupt`. Pinned by `custody_keyring_service_is_not_the_legacy_shared_one`.
pub(crate) const CUSTODY_KEYRING_SERVICE: &str = "ai.citrate.core.custody";

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
mod provisioning;
mod sbt_art;
mod seam;
mod serve;
mod shell;
mod staking;
mod transfer;
mod validator;
// CX (planset citrate-core-social) — host modules, one per feature lane. S0.3 registers all
// command names once here + in generate_handler! below; each lane fills in its own module's
// bodies (never this file). See .agentile/cx-ownership.map.
mod cluster;
mod comms;
mod hermes;
mod invites;
mod invite_seal;
mod model_catalog;
mod social;
mod storage;
mod training;

use tauri::Manager;

/// Kill orphaned sidecars left by a crashed previous instance (they'd hold the node LOCK / bound
/// sockets). Matches ONLY processes whose command line references this bundle's binary directory,
/// and never our own pid. Best-effort + macOS/Linux only (uses `pgrep`/`kill`); a no-op if `pgrep`
/// is unavailable. Runs once at startup, before any sidecar is spawned, so there is nothing of ours
/// to catch — every match is an orphan.
/// The sidecar binaries we own. An orphan of ANY of these holds a data-dir LOCK (RocksDB node store,
/// ipfs datastore, mem-mcp store) or a UDS socket, so a fresh launch can't open its own → the pinwheel
/// / "Resource temporarily unavailable" seen when a previous copy crashed or was run from a mounted DMG.
const OWNED_SIDECARS: &[&str] =
    &["citrate", "ipfs", "mem-mcp", "comms-member-daemon", "cluster-daemon", "hermes"];

/// Reap orphaned sidecars from a PREVIOUS/other instance before we spawn our own. The supervisor kills
/// its children on graceful teardown, but a crash (SIGKILL) can't run Drop — leaving an orphan that
/// holds the store LOCK. Single-instance stops the double-LAUNCH; this stops the crash-orphan case.
///
/// TWO passes:
///   1. Anything under THIS bundle's binary dir (covers a plain crash-restart, incl. dev builds).
///   2. Any process whose executable is one of OUR sidecars launched from *any* `Citrate Core.app`
///      bundle — INCLUDING a different path such as a still-mounted `/Volumes/Citrate Core*` DMG. Pass 1
///      missed those (they don't share our dir), so a DMG-run copy's orphaned node/ipfs/mem kept the
///      locks and the /Applications launch pinwheeled. We target only the known sidecar BASENAMES under
///      a Citrate bundle, so unrelated processes (and foreign MAIN-app processes — single-instance's
///      job) are never touched. At startup our own sidecars aren't spawned yet, so every match is an
///      orphan. Never kills self.
fn sweep_orphan_sidecars() {
    let self_pid = std::process::id();
    let mut victims: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();

    // Pass 1 — processes under our own binary dir.
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        if let Ok(out) = std::process::Command::new("pgrep").arg("-f").arg(dir.to_string_lossy().as_ref()).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Ok(pid) = line.trim().parse::<u32>() {
                    victims.insert(pid);
                }
            }
        } else {
            return; // no pgrep (or not Unix) — skip the sweep entirely
        }
    }

    // Pass 2 — our sidecars launched from ANY Citrate bundle (foreign path / mounted DMG). Match the
    // sidecar running under a `Citrate Core.app/Contents/MacOS/<sidecar>` path, then confirm the
    // process's executable basename is one we own (so a foreign MAIN-app binary is left alone).
    if let Ok(out) = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("Citrate Core.app/Contents/MacOS/")
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let pid = match line.trim().parse::<u32>() {
                Ok(p) => p,
                Err(_) => continue,
            };
            // The process's own executable name (argv[0] basename), not its arguments.
            let comm = std::process::Command::new("ps")
                .args(["-o", "comm=", "-p", &pid.to_string()])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            let base = std::path::Path::new(&comm)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or(comm);
            if OWNED_SIDECARS.iter().any(|s| base == *s) {
                victims.insert(pid);
            }
        }
    }

    for pid in victims {
        if pid != self_pid {
            let _ = std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux + NVIDIA black-screen fix. WebKitGTK's GPU-accelerated compositing / DMABUF renderer
    // hangs the display on NVIDIA drivers — observed on an NVIDIA GB10 (driver 580): launching the
    // app blacked out the ENTIRE screen and crashed the X session (not just the app window; the
    // window isn't even fullscreen). Forcing WebKit to software compositing avoids the GPU path
    // entirely. This runs before the webview is created, and only sets each var when unset so a user
    // whose stack renders fine can re-enable GPU compositing via the environment. macOS/Windows are
    // unaffected (WebKit env vars are Linux-only). Bakes in the manual workaround from
    // docs/RELEASE_LINUX.md (#236 "black screen" troubleshooting) so no user needs to set it by hand.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    tauri::Builder::default()
        // Single-instance FIRST (tauri requires it): a second launch focuses the running window
        // instead of spawning a duplicate app + duplicate sidecars that fight over the node LOCK.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        // GROW-S1 — register the `citrate://` scheme so a join link opens the app and hands off the
        // invite (the reliable one-tap path for a .dmg; the web half shipped in citrate-landing #44).
        // The frontend listens via @tauri-apps/plugin-deep-link's onOpenUrl → parseJoinLink → Groups.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        // W2.1 — in-app auto-updates (signed GitHub Releases feed; pubkey pinned in
        // tauri.conf.json). The JS API drives check/download/install via this plugin.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // W2.4 — relaunch into the freshly-installed version.
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Sidecar-lifecycle hardening: reap orphaned sidecars from a PREVIOUS instance before
            // we spawn our own. The supervisor kills its children on graceful teardown, but a crash
            // (SIGKILL) can't run Drop — leaving an orphaned node that still holds the RocksDB LOCK,
            // so the fresh node fails with "Resource temporarily unavailable". Single-instance (above)
            // stops the double-LAUNCH case; this sweep stops the crash-orphan case. Only processes
            // under THIS bundle's binary dir are touched, never us.
            sweep_orphan_sidecars();
            // CORE-A2 — build the process-wide custody vault (real OS keyring +
            // app-data envelope), seeded with the persisted config.autolock (the
            // A1 single source of truth). @rule8: no secret bytes cross invoke.
            let handle = app.handle();
            let autolock = config::config_read(handle.clone())
                .map(|c| c.autolock)
                .unwrap_or_else(|_| config::AppConfig::default().autolock);
            // Custody gets its OWN keyring namespace. The kit is shared with
            // citrate-quorum, which already owns the legacy `ai.citrate.core`
            // service: its live vault's master key is sealed there. Sharing one
            // service means sharing the custody ANCHOR while keeping separate
            // envelope files, so whichever app starts second sees "anchor says a
            // vault exists, my envelope says it does not" — the F-1 rollback
            // shape — and is locked out permanently with `Corrupt`.
            //
            // Observed live on the DGX 2026-08-04: onboarding step 3 failed with
            // "custody envelope corrupt or tampered" purely because quorum had
            // initialised first. Clearing the keyring would have DESTROYED
            // quorum's vault, so the namespace moves instead — and only for
            // citrate-core custody, which has no vault to orphan. The node
            // storage key and mem-store key deliberately STAY on the legacy
            // service (see node.rs / memory.rs) because their data already
            // exists under it.
            let state =
                custody::build_custody_state_with_service(handle, autolock, CUSTODY_KEYRING_SERVICE)
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
            // Social verify (ADR-2026-08-30): the pending-verification table, keyed by ceremony id.
            app.manage(social::build_social_bind_state());
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
            // Seamless device-bound re-unlock (passphrase-less model): re-provisions
            // + unlocks from the keyring device passphrase, for the Settings "Unlock"
            // control and app launch/resume. Without it an auto-locked vault (and the
            // wallet reads gated on it) stay stuck — no user passphrase exists to enter.
            custody::custody_ensure_unlocked,
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
            // Group claimable invites (ADR D4) — mint / list / verify-consume / revoke.
            invites::group_invite_create,
            invites::group_invite_submit_claim,
            invites::group_invite_poll_claims,
            invites::group_invites,
            invites::group_invite_verify_consume,
            invites::group_invite_revoke,
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
            // ── CX (planset citrate-core-social) — command names FROZEN in S0.3. Each is a
            // NotWired stub until its lane wires the body (in its own module, never here). ──
            model_catalog::model_catalog_local,
            model_catalog::model_catalog_search,
            model_catalog::model_catalog_download,
            model_catalog::model_catalog_select,
            storage::storage_add,
            storage::storage_pin,
            storage::storage_list,
            storage::storage_retrieve,
            storage::storage_unpin,
            comms::groups_create,
            comms::comms_relay_status,
            comms::groups_list,
            comms::groups_self_address,
            comms::groups_join,
            comms::groups_add_member,
            comms::groups_roster,
            comms::groups_assign_role,
            comms::groups_offboard,
            comms::groups_send,
            comms::groups_messages,
            cluster::cluster_status,
            cluster::cluster_join,
            cluster::cluster_peers,
            cluster::cluster_share_file,
            cluster::cluster_leave,
            training::training_start,
            training::training_status,
            training::training_contribute,
            training::training_reward,
            training::training_claim,
            hermes::hermes_start,
            hermes::hermes_status,
            hermes::hermes_skills,
            hermes::hermes_run_skill,
            hermes::hermes_pending_approvals,
            hermes::hermes_stop,
            hermes::hermes_bridge_pending,
            hermes::hermes_resolve,
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
            // W1.3 — activate the member's validator bond (bond-clone model): the
            // ceremony sends MemberBond.activate to the funded clone; the clone (=
            // staker) forwards its principal to registerValidator.
            node::node_register_validator,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Graceful sidecar teardown on quit. Without this, a hard quit orphaned the `citrate`
            // node (and ipfs/memory), which kept holding their RocksDB LOCKs — so the NEXT launch
            // failed to reopen the data dir ("Resource temporarily unavailable") and sync never
            // resumed. On exit we SIGTERM→grace→SIGKILL every supervised sidecar so their locks are
            // released and reopen is clean. (Belt-and-suspenders with the startup orphan sweep.)
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shutdown_all_sidecars(app_handle);
            }
        });
}

/// Stop every supervised sidecar so none is orphaned across an app quit. Managed states expose the
/// SidecarSupervisor via `.0.stop()`; the lazily-started daemons expose a module `shutdown()`. Every
/// stop is idempotent and a no-op when that sidecar was never started, so this is safe to call once
/// on exit regardless of what the session actually launched.
fn shutdown_all_sidecars(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(s) = app.try_state::<node::NodeState>() {
        s.0.stop();
    }
    if let Some(s) = app.try_state::<agent::AgentState>() {
        s.0.stop();
    }
    if let Some(s) = app.try_state::<memory::MemoryState>() {
        s.0.stop();
    }
    if let Some(s) = app.try_state::<serve::ServeState>() {
        s.0.stop();
    }
    if let Some(s) = app.try_state::<ipfs::IpfsState>() {
        s.0.stop();
    }
    // Lazily-started daemons (not managed state) — stop only if this session started them.
    comms::shutdown();
    cluster::shutdown();
    hermes::shutdown();
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
            "custody::custody_ensure_unlocked",
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

    /// UI-RESPONSIVENESS TRIPWIRE (2026-08-06).
    ///
    /// A SYNCHRONOUS `#[tauri::command]` runs on the MAIN THREAD. Any such command
    /// that performs a blocking network read freezes the window for the duration.
    /// `membership_grant_status` did exactly this — nine sequential ~240ms
    /// `eth_call`s, polled every 2s — and the app rendered but ignored clicks.
    ///
    /// Every command that touches the RPC is now `pub async fn`, so Tauri runs it
    /// off the UI thread. This test re-derives that from the SOURCE so the property
    /// cannot regress silently: a future `pub fn` command whose body reaches the
    /// network fails here, at the commit that introduces it.
    ///
    /// Deliberately source-scanning rather than a runtime assertion: the defect is
    /// a compile-time shape (sync vs async), invisible to any unit test of the
    /// command's own logic.
    #[test]
    fn no_synchronous_tauri_command_performs_network_io() {
        // Files whose commands legitimately reach the 40204 RPC.
        let sources: &[(&str, &str)] = &[
            ("activity.rs", include_str!("activity.rs")),
            ("agent.rs", include_str!("agent.rs")),
            ("earnings.rs", include_str!("earnings.rs")),
            ("grant_status.rs", include_str!("grant_status.rs")),
            ("node.rs", include_str!("node.rs")),
            ("sbt_art.rs", include_str!("sbt_art.rs")),
            ("staking.rs", include_str!("staking.rs")),
            ("transfer.rs", include_str!("transfer.rs")),
            ("validator.rs", include_str!("validator.rs")),
        ];
        // Assembled from parts so this test's own prose cannot self-match.
        let attr = "#[tauri".to_string() + "::command]";
        let mut offenders: Vec<String> = Vec::new();

        for (name, src) in sources {
            let lines: Vec<&str> = src.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.trim() != attr {
                    continue;
                }
                // The signature is the next non-attribute, non-doc line.
                let Some(sig_idx) = (i + 1..lines.len().min(i + 12)).find(|&j| {
                    let t = lines[j].trim_start();
                    t.starts_with("pub fn ") || t.starts_with("pub async fn ")
                }) else {
                    continue;
                };
                let sig = lines[sig_idx].trim_start();
                if sig.starts_with("pub async fn ") {
                    continue; // already off the UI thread
                }
                // Scan this sync command's body to the next command or EOF.
                let end = (sig_idx + 1..lines.len())
                    .find(|&j| lines[j].trim() == attr)
                    .unwrap_or(lines.len());
                let body = lines[sig_idx..end].join("\n");
                // Ignore comment lines so prose mentioning the RPC cannot trip this.
                let code: String = body
                    .lines()
                    .filter(|l| !l.trim_start().starts_with("//"))
                    .collect::<Vec<_>>()
                    .join("\n");
                // Direct network use, PLUS the helpers known to reach the network
                // one level down. `node_arm_mining` was missed by the direct-match
                // form: its body only calls `arm_mining_if_synced`, which calls
                // `remote_network_tip`. This list is the honest limit of a
                // source-level scan — it does not follow arbitrary call graphs, so
                // a NEW transitively-networking helper must be added here.
                let touches_network = code.contains("RpcClient::")
                    || code.contains("read_grant_status(")
                    || code.contains("remote_network_tip(")
                    || code.contains("arm_mining_if_synced(")
                    || code.contains("read_self_stake(")
                    || code.contains("read_claimable(");
                if touches_network {
                    let fn_name = sig
                        .trim_start_matches("pub fn ")
                        .split('(')
                        .next()
                        .unwrap_or(sig);
                    offenders.push(format!("{name}::{fn_name}"));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "these synchronous #[tauri::command]s do blocking network I/O on the MAIN \
             THREAD and will freeze the UI — make them `pub async fn`: {offenders:?}"
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

#[cfg(test)]
mod custody_namespace_tests {
    use super::CUSTODY_KEYRING_SERVICE;
    use citrate_core_kit::custody::OsKeyring;

    /// citrate-core custody MUST NOT share a keyring service with the kit's
    /// legacy namespace.
    ///
    /// The kit is shared with citrate-quorum, which owns the legacy service and
    /// has a LIVE vault sealed under it. Two apps on one service share the
    /// custody ANCHOR (`custody-generation`) while keeping SEPARATE envelope
    /// files, so whichever starts second sees "the anchor says a vault exists,
    /// my envelope says it does not" — the F-1 rollback shape — and is locked
    /// out permanently with `CustodyError::Corrupt`.
    ///
    /// Observed live on the DGX 2026-08-04: onboarding step 3 failed with
    /// "custody envelope corrupt or tampered" purely because quorum had
    /// initialised first. The tempting fix (clear the keyring) would have
    /// DESTROYED quorum's vault — `custody-master-key` is what seals its
    /// envelope. Hence: separate namespaces, and never repoint an app that
    /// already has a vault.
    #[test]
    fn custody_keyring_service_is_not_the_legacy_shared_one() {
        assert_ne!(
            CUSTODY_KEYRING_SERVICE,
            OsKeyring::LEGACY_SERVICE,
            "citrate-core custody must own its keyring namespace; sharing the \
             legacy one with citrate-quorum locks whichever app starts second \
             out of its vault forever"
        );
        assert!(
            !CUSTODY_KEYRING_SERVICE.is_empty(),
            "an empty service name would silently collide with anything else \
             using an empty service"
        );
    }
}
