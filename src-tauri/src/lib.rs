//! citrate-core — the federation desktop house (Tauri full-node app).
//!
//! CORE-A1 bridge: the `config` domain is wired end-to-end for real (persisted
//! on-disk via tauri-plugin-store + OS-keyring status). Every other bridge
//! domain is registered but returns an honest `Unavailable` error — never
//! fabricated data (Rule 1). Chain reads happen in the webview via viem/wagmi
//! (D-13); signing routes through the Rust SignatureCeremony once it exists
//! (CORE-S2).
// Rule 8 (zero `.unwrap()` in shipped code): CI's clippy step (`-D warnings`) fails on any
// `.unwrap()` outside test code. Tests are compiled with `cfg(test)`, where this does not apply.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

// WP-S1.2: the safety-critical spine (ceremony, custody, oidc, supervisor,
// wallet, rpc, txdecode, config) now lives in `citrate-core-kit`, shared with
// citrate-quorum so there is ONE signing path in the federation, not a fork.
// Re-exported here so every in-crate reference (`crate::custody::…`,
// `crate::ceremony::…`, and the command paths in `commands/l<N>.rs`) keeps
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
    abi_book, ceremony, config, custody, oidc, payouts_gate, rpc, secret, supervisor, txdecode,
    wallet, wallet_link,
};

mod activity;
mod addresses;
mod agent;
mod agent_grants;
mod agent_precompiles;
mod agent_sbt;
mod ai;
mod anchor_proof;
// First-run access check for invited installs (download gate); off unless CITRATE_ACCESS_GATE_URL is set at build time.
mod access_pass;
mod atomic_file;
mod benchmark_share;
mod blocking;
mod capsule_pins;
mod chain_agent;
mod components;
mod connections;
mod contract_deploy;
mod daemons;
// Join-13: deep links delivered to the running app on Linux/Windows; URL fragments never logged.
mod deep_link_delivery;
#[cfg(any(target_os = "linux", test))]
mod linux_webkit_env;
// 0.5.7: the start-up soft RLIMIT_NOFILE raise, inherited by every sidecar.
mod fd_limit;
// HUP-S6.7 — the Contract reader backend (verified source, view calls, ceremony-only writes).
mod contract_intel;
mod contract_reader;
mod deploy_gate;
// CORE-A3 (0.5.3): durable deploy records, encrypted per project.
mod deploy_records;
// 0.5.4 — the Hermes chat history, sealed on this device (ML-KEM-768 + AES-256-GCM, key in the keyring).
mod hermes_history;
// 0.5.4 FL design board 11 — worker terms and the HIC-2 envelope, sealed the same way.
mod fl_work;
// 0.5.4 FL design board 07 — a round's requirements file, published on the bundled IPFS node.
mod fl_publish;
// 0.5.4 Hermes x FL WP-H4 — the HIC-2 envelope evaluator, its own fl_envelope scope (not SIWE).
mod fl_envelope;
// HUP-S6.3 → S6.4: Hermes's toolchain reports into the deploy gate (retro A27).
mod deploy_gate_toolchain;
// HUP-S6.5: deploy-gas top-ups from the faucet, off by default (faucet ADR, proposed).
mod faucet;
// 0.5.4 WP-H6 — the member's local FL dataset store (sealed, hash-chained manifests).
mod fl_datasets;
mod fl_market;
mod fl_payouts;
mod fl_rounds;
mod fl_trajectories;
// HUP-S6.3: the member's toolchain switch and the sidecar environment it produces.
mod forge_toolchain;
mod fork_dry_run;
// HUP-S2.3 — Settings → Budgets + the budgeted SIWE entry point (ADR-2026-09-30, accepted).
mod hic_records;
mod inference_router;
mod web_budgets;
mod web_signin;
// HUP-S8.1 — per-device key + DeviceLink (ceremony-gated wallet signature).
mod chat_consent;
mod chat_route;
mod device_link;
mod device_link_share;
mod docs_ingest;
mod earnings;
mod escalation;
mod fleet;
mod fleet_mdns;
mod fleet_pairing;
mod fleet_tailscale;
mod google_workspace;
mod grant_status;
mod hermes_mcp;
mod hermes_schedule;
mod hf_auth;
mod ipc_name;
mod ipfs;
mod journal_export;
// L0-WP04 (planset v060): the per-lane module tree and start-up hooks (`lanes/l<N>/`). A lane's
// new module is declared in its own `lanes/l<N>/mod.rs`, not here.
mod lanes;
mod local_data;
mod mcp_servers;
mod media;
pub mod mem_mcp_bridge;
mod membership;
mod membership_health;
mod membership_price;
mod memory;
// HUP-S3.1 — first-run import of the bundled knowledge corpus into the memory store.
mod knowledge_import;
mod mining_gate;
mod model;
mod model_log;
mod model_register;
mod model_registry;
mod money_gas;
// K6 — the network this build targets (one profile: chain id, genesis, bootnodes, RPC, book).
mod network_profile;
// D-11 (RC52) — bring-your-own-key cloud providers (ADR-2026-10-26): the provider trait and
// internal format, and the pinned egress table.
// D-11 (RC52) — the Cognition (Devin) remote-agent delegate (ADR P-8).
pub(crate) mod cognition;
pub(crate) mod model_provider;
pub(crate) mod model_proxy;
pub(crate) mod model_proxy_http;
mod node;
mod node_genesis;
mod node_holder;
// CORE-D2: a wedged-but-running node is restarted after a sustained stall (never in initial sync).
mod node_liveness;
mod node_withdrawn;
// CORE-N3 follow-up: list and remove the previous network's set-aside chain data.
mod chain_backup;
// CORE-N4: the node's own syncState and its peers' tips (local RPC, feature-detected).
mod sync_truth;
// C1-V3 — "submitted, outcome unknown, tx <hash>, check before retrying" for agents and MCP clients.
mod broadcast_outcome;
pub(crate) mod provider_anthropic;
pub(crate) mod provider_chat_vendors;
pub(crate) mod provider_egress;
#[cfg(test)]
mod provider_fixtures_tests;
pub(crate) mod provider_gemini;
pub(crate) mod provider_guard;
// D-11 (P-5) — the member's preferred cloud model for Hermes (agent loop and chat).
pub(crate) mod provider_openai;
pub(crate) mod provider_openai_compat;
pub(crate) mod provider_preferred;
#[cfg(test)]
mod provider_tripwire_tests;
pub(crate) mod provider_usage_log;
mod providers_ipc;
// SCL-S0.1 — startup cleanup of this installation's own leftover sidecars (exact paths only).
mod owned_cleanup;
// HUP-S6.6 — after the deploy: receipt, verify, site switch, IPFS pin, Vercel export.
mod postdeploy;
// HUP-S4.2 + S8.5 — the citrate-node MCP server (loopback, connect token, writes via approval).
mod node_mcp;
mod node_mcp_approvals;
mod node_mcp_hermes;
mod node_mcp_http;
mod node_mcp_live;
mod node_mcp_protocol;
mod node_mcp_token;
mod node_mcp_tools;
mod startup_barrier;
// HUP-S5.4 — pop-out windows (allowlisted kinds, least-privilege capability, persisted geometry).
mod grant_deny;
mod popout;
mod popout_contract;
// HUP-S5.1 + S5.6 — the member's controls for Hermes's browser (the sidecar runs it).
mod browser;
mod provisioning;
mod recovery_kit;
mod sbt_art;
mod serve;
mod serve_plan;
mod services;
// CORE-D5 (0.5.3): sidecar log files and the diagnostics bundle.
mod sidecar_logs;
// #613 review: the secret shapes kept off the disk and out of the diagnostics bundle.
mod fl_collect;
mod fl_dataset_gate;
mod pii_scan;
mod secret_shapes;
mod shell;
mod skill_registry;
mod skills_local;
mod telemetry;
// CORE-D6 (0.5.3): drain the sidecars before an update is installed.
mod update_drain;
// 0.5.4: the signed minimum version (forced update).
mod update_policy;
// HUP-S6.2 / US-6.4: contract templates for members (citrate-templates renderer).
mod template_forge;
mod tier;
mod transfer;
mod validator;
mod validator_activation;
mod verified_source;
mod widgets;
// CX (planset citrate-core-social) — host modules, one per feature lane. S0.3 registers all
// command names once (now in `commands/l<N>.rs`, L0-WP03); each lane fills in its own module's
// bodies (never this file). See .agentile/cx-ownership.map.
mod cluster;
mod cluster_mesh;
// L0-WP03 (planset v060): the command registration, one list per lane (`commands/l<N>.rs`).
mod commands;
mod comms;
mod embed_serve;
mod hermes;
// 0.5.4: Hermes provisioning at launch + the setup self-check.
mod hermes_setup;
// SCL-S0.3: the managed browser does not outlive the Hermes sidecar.
mod hermes_headless;
mod hermes_learn;
mod hermes_web;
mod owned_browser;
// Hermes terminal commands: the member's switch for the sidecar's shell_run (on by default).
mod hermes_auto_approve;
mod hermes_terminal;
mod invite_seal;
// D-12: group invites, one flow (request and approve).
mod invites;
mod model_catalog;
// Deleting a downloaded model (member-only, Models screen; never an agent tool).
mod model_delete;
mod social;
mod storage;
// K4: verified export, backup and restore of Files and Memory.
mod storage_backup;
mod store_catalog;
mod training;

// PBA-L4-008 class tripwire: no invoke command serializes a secret-bearing struct (I-2).
#[cfg(test)]
mod invoke_secret_scan_tests;
// L0-WP02 (planset v060): characterization of the command registration, the ACL and this
// module set; the seam refactors (L0-WP03, WP04) keep it byte-identical.
#[cfg(test)]
mod seams_tests;
// Every in-process vault write targets a slot the kit's invoke-path custody_put cannot address
// (citrate-core no longer registers custody_put; citrate-quorum, which shares the kit, may).
#[cfg(test)]
mod main_thread_tripwire;
#[cfg(test)]
mod vault_slot_reservation_tests;
// HUP-S10.5: offline matrix probes, telemetry consent field list, default budget ceilings.
#[cfg(test)]
mod privacy_contract_tests;
// HUP-S6 US-6.1 / g3-gate / g3-e2e (local): the hello-mint Gherkin, end to end on a local chain.
#[cfg(test)]
mod hello_mint_e2e_tests;
// G7: core's money paths on citrate-chain's genesis contract set (scripts/e2e/moneypath-anvil.sh).
#[cfg(test)]
mod moneypath_e2e_tests;
// SCL-S0.5: shutdown coverage (embedding server stops with Hermes; workers exit on stdin close).
#[cfg(test)]
mod shutdown_coverage_tests;
// SCL-S0.7: the Windows installer's pre-install hook (inputs checked on every OS).
#[cfg(test)]
mod windows_installer_hook_tests;
// 0.5.1 -> 0.5.2: the app data a 0.5.1 install holds, and the updater feed it trusts.
#[cfg(test)]
mod upgrade_updater_feed_tests;
// RC52 G8: the K1-L update-e2e driver (feature `updater-e2e`, test builds only; guard tests always).
#[cfg(any(test, feature = "updater-e2e"))]
mod updater_e2e;
#[cfg(test)]
mod upgrade_v051_tests;
// RC52 G2: a 0.5.1 install on r1005 started by this build on a new genesis (K6).
#[cfg(test)]
mod upgrade_xgenesis_tests;
// CORE-N4: the node's own syncState and its peers' tips, read from its local RPC.
#[cfg(test)]
mod sync_truth_tests;
// CORE-N3 follow-up: Remove old network data (chain.<profile>.bak).
#[cfg(test)]
mod chain_backup_tests;
// CORE-N3: a 0.5.2 install on r1006 started by this build on r1007 (chain data set aside).
#[cfg(test)]
mod upgrade_r1007_tests;
// D-9: the liquid staking pool is retired; no pool reference in the crate, its book or the kit.
#[cfg(test)]
mod lsp_retired_tests;
// 0.5.4: unused commands and seam stubs stay removed.
#[cfg(test)]
mod removed_commands_tests;

use tauri::Manager;

/// The sidecar binaries we own. A leftover of ANY of these from a crashed run holds a data-dir LOCK
/// (RocksDB node store, ipfs datastore, mem-mcp store), a UDS socket or a loopback port, so a fresh
/// launch can't open its own. Each name resolves through the same resolver the app launches it with
/// ([`resolve_owned_sidecar`]); `owned_cleanup` tests pin this list against the bundle configs.
///
/// CORE-D1: `llama-server` is the one bundled binary both llama sidecars run (local chat in
/// `serve.rs`, embeddings in `embed_serve.rs`). It resolves to the bundled `llama/llama-server` only,
/// never a developer override, so a member's own llama-server elsewhere is never a candidate.
const OWNED_SIDECARS: &[&str] = &[
    "citrate",
    "node-agent",
    "ipfs",
    "mem-mcp",
    "comms-member-daemon",
    "cluster-daemon",
    "hermes",
    "llama-server",
];

/// Resolve one owned sidecar's executable path with the resolver its manager launches it with.
fn resolve_owned_sidecar<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    name: &str,
) -> std::result::Result<std::path::PathBuf, String> {
    match name {
        "citrate" => node::resolve_node_bin(app),
        "node-agent" => agent::resolve_agent_bin(app),
        "ipfs" => ipfs::resolve_ipfs_bin(app),
        "mem-mcp" => memory::resolve_mem_mcp_bin(app),
        "comms-member-daemon" => comms::resolve_comms_member_bin(app),
        "cluster-daemon" => cluster::resolve_cluster_daemon_bin(app),
        "hermes" => hermes::resolve_hermes_bin(app),
        "llama-server" => serve::bundled_llama_bin(app),
        other => Err(format!("not an owned sidecar: {other}")),
    }
}

/// Stop leftover sidecars of THIS installation from a crashed previous run before we spawn our own
/// (SCL-S0.1). The supervisor stops its children on graceful teardown, but a crash (SIGKILL) can't
/// run Drop. Single-instance stops the double-LAUNCH; this stops the crash-leftover case.
///
/// Matches only processes whose executable is exactly one of our resolved sidecar paths and whose
/// parent is not a live instance of this app; see [`owned_cleanup`] for the full rules. Runs once
/// at startup, before any sidecar is spawned.
fn sweep_orphan_sidecars<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let owned: Vec<std::path::PathBuf> = OWNED_SIDECARS
        .iter()
        .filter_map(|name| resolve_owned_sidecar(app, name).ok())
        .collect();
    let main_exe = std::env::current_exe().ok();
    let _ = owned_cleanup::cleanup(&owned, main_exe.as_deref());
}

/// HUP-S4.2 — `citrate-core --mcp-stdio`: the stdio shim for the citrate-node MCP server. Runs
/// without starting the app; forwards stdin JSON-RPC to the running app's loopback endpoint.
pub fn node_mcp_stdio_main() -> i32 {
    node_mcp_http::stdio_main()
}

/// The flag `main` checks for the stdio shim (shared with Hermes's built-in `node` entry).
pub const NODE_MCP_STDIO_FLAG: &str = node_mcp_http::STDIO_FLAG;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux black-window guard (#237, DGX GB10 report): WebKitGTK's DMA-BUF renderer and GPU
    // compositing off by default, never over a value the member set. Runs first, before the
    // builder creates GTK/WebKit and while the process is single-threaded; logs one line.
    #[cfg(target_os = "linux")]
    linux_webkit_env::apply();
    // 0.5.7: raise the soft open-file limit before any sidecar is spawned, so every child (the
    // node and its RocksDB first) inherits it. A Finder/Dock launch on macOS starts at 256, too
    // few for the node's SST files. Logs one line; never fails start-up.
    fd_limit::apply_at_startup();
    // K6 — install the network gate before anything can sign. Every wallet signature is
    // checked locally against the active network profile (refused when no network is set,
    // or when it commits to another chain id). Transaction sends additionally check the
    // RPC's chain id and block-0 hash against the profile before broadcasting.
    // L0-WP04: both gates are installed here, before the builder, because a plugin's own init
    // (a lane plugin from `lanes::plugins_all` included) runs while the app is built, before the
    // setup closure. The profile is compiled in, so nothing in setup is needed first.
    network_profile::install_network_gate();
    // G-SEC PAY-10/11/14: the FL payouts gate, from the same profile, before anything can
    // sign. Payouts are dark unless the profile opens them and the pinned code matches.
    network_profile::install_payouts_gate();

    let builder = tauri::Builder::default()
        // Single-instance FIRST (tauri requires it): a second launch focuses the running window
        // instead of spawning a duplicate app + duplicate sidecars that fight over the node LOCK.
        // Join-13: on Linux/Windows a clicked `citrate://` link arrives as that second launch's
        // argv; its accepted links go to the deep-link plugin's own path (see deep_link_delivery).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            use tauri::Manager;
            deep_link_delivery::deliver_second_instance_argv(app, &argv);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Register the `citrate://` scheme so an invite, pairing or claim link opens the app. The
        // frontend listens via @tauri-apps/plugin-deep-link's onOpenUrl → store.handleDeepLink.
        // Links clicked while the app runs on Linux/Windows reach it via single-instance above.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        // W2.1 — in-app auto-updates (signed GitHub Releases feed; pubkey pinned in
        // tauri.conf.json). The JS API drives check/download/install via this plugin.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // W2.4 — relaunch into the freshly-installed version.
        .plugin(tauri_plugin_process::init());
    // L0-WP04: each lane's plugins (`lanes/l<N>/hooks.rs`), l0 to l9, after every plugin above, so
    // single-instance stays first, and after the network and payouts gates (installed above), so a
    // lane plugin's init cannot sign ungated. A new plugin goes in a lane's hooks.rs, not here.
    crate::lanes::plugins_all(builder)
        // HUP-S10.3 — widget documents (`citrate-widget://localhost/<id>`), served from the widget
        // store with a strict CSP, to the main window only. See widgets.rs.
        .register_asynchronous_uri_scheme_protocol(widgets::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let label = ctx.webview_label().to_string();
            std::thread::spawn(move || responder.respond(widgets::respond(&app, &label, &request)));
        })
        .setup(|app| {
            // 0.5.4 (#677, owner 2026-11-14: on for new installs only): whether this install has
            // data from an earlier run, read before this launch writes anything.
            let had_existing_data = hermes_setup::had_existing_data(&hermes_setup::data_roots(app.handle()));
            // The network and payouts gates are already installed: run() installs them before the
            // builder, so they hold before any plugin (lane plugins included) initializes.
            // CORE-D5: the sidecar log folder is set before any sidecar can start.
            if let Ok(dir) = app.path().app_log_dir() {
                sidecar_logs::init(&dir);
            }
            // Sidecar-lifecycle hardening: reap orphaned sidecars from a PREVIOUS instance before
            // we spawn our own. The supervisor kills its children on graceful teardown, but a crash
            // (SIGKILL) can't run Drop — leaving an orphaned node that still holds the RocksDB LOCK,
            // so the fresh node fails with "Resource temporarily unavailable". Single-instance (above)
            // stops the double-LAUNCH case; this sweep stops the crash-orphan case. Only processes
            // running one of THIS installation's sidecar binaries (exact path) are touched.
            //
            // SCL-S8.5a: the cleanup runs through the startup barrier. No node spawn is admitted
            // (NodeManager::start waits on it) until the cleanup has returned; a panicking
            // cleanup leaves it closed. The barrier does not depend on what the cleanup does.
            let handle = app.handle();
            // Join-13: an AppImage has no .desktop install to register `citrate://`; do it here.
            #[cfg(target_os = "linux")]
            deep_link_delivery::register_appimage_schemes(handle);
            startup_barrier::StartupBarrier::global().run_cleanup(|| sweep_orphan_sidecars(handle));
            // SCL-S0.3: a managed browser left by an earlier Hermes sidecar is stopped and its
            // profile removed, from the record this app kept (never from anything the sidecar wrote).
            hermes::clean_leftover_browser(handle);
            // STO-2 P3: a backup staging copy an earlier (killed) run left behind is removed
            // before any backup can run. It may hold plaintext of the member's stores.
            if let Ok(dir) = app.path().app_data_dir() {
                match storage_backup::sweep_staging(&dir) {
                    Ok(0) => {}
                    Ok(n) => eprintln!("[storage] removed {n} leftover backup staging folder(s)"),
                    Err(e) => eprintln!("[storage] {e}"),
                }
            }
            // RC52 P-9a: spawn-time MCP files an earlier run left are removed, and member MCP env
            // values an older version kept in plain text move to the keychain (in the background).
            mcp_servers::startup(handle);
            // WP-T.2 — install the local panic hook (appends crash context to a local file the
            // diagnostics bundle later reads). No network; nothing egresses without consent (WP-T.1).
            telemetry::install_panic_hook(&app.handle().clone());
            // RC52 G8: a K1-L update-e2e test build checks, installs and restarts by itself.
            #[cfg(feature = "updater-e2e")]
            updater_e2e::start(app.handle());
            // HUP-S7.3 — the nightly anchor scheduler starts only when AnchorRegistry is in the
            // address book (it is, on 40204) AND the member turned anchoring on (off by default).
            chain_agent::start_nightly_if_ready(app.handle().clone());
            // HUP-S7.5 (D-27) — meter Hermes's own transactions (gas, SALT) once mined. Installs an
            // observer only; nothing is signed or sent by it.
            chain_agent::spend::install(app.handle().clone());
            // CORE-A2 — build the process-wide custody vault (real OS keyring +
            // app-data envelope), seeded with the persisted config.autolock (the
            // A1 single source of truth). @rule8: no secret bytes cross invoke.
            let handle = app.handle();
            let autolock = config::config_read_sync(handle.clone())
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
            let state = custody::build_custody_state_with_service(
                handle,
                autolock,
                CUSTODY_KEYRING_SERVICE,
            )
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(state);
            // CORE-A3 — the auth manager (real loopback-PKCE OIDC against
            // auth.citrate.ai; refresh token → the A2 vault; access token in
            // memory only; @rule8: no token crosses invoke). Consumes A2.
            app.manage(oidc::build_auth_state());
            // D-12: settle invite approvals left held by a crash or a lost answer (roster check).
            invites::recheck_held_on_start(app.handle().clone());
            // CORE-B1.2 — the SignatureCeremony: the SINGLE human-in-the-loop
            // signing path. Every signature intent (user or, later, agent /
            // micro-app) routes through one approval surface; the gated
            // `wallet::sign_message` is reachable ONLY from `approve`. @rule8:
            // this LIFTS Rule 3 — all signing goes through this ceremony. No
            // secret bytes cross invoke (sign_* return id / decoded / sig-hex).
            // Contract v1.1 §1: the first-submission record persists under the app data dir, so
            // a re-send after a restart is never mistaken for a first submission. It has one
            // section per network, opened on the genesis of the installed network gate (#511):
            // other networks' sections are archived, read-only, and decide nothing here. With no
            // gate genesis every send is refused anyway, and the record on disk is left as it is.
            let gate_genesis = rpc::network_gate()
                .filter(|g| g.is_set())
                .and_then(|g| g.genesis_hash.clone());
            app.manage(match (app.path().app_data_dir(), gate_genesis) {
                (Ok(dir), Some(genesis)) => {
                    ceremony::build_ceremony_state_at(
                        dir.join(citrate_core_kit::submitted_log::RECORD_FILE),
                        &genesis,
                    )
                }
                (Err(e), _) => {
                    eprintln!("[ceremony] no app data dir ({e}); the submitted-tx record is in memory only");
                    ceremony::build_ceremony_state_unpersisted()
                }
                (Ok(_), None) => {
                    eprintln!("[ceremony] no network genesis; the submitted-tx record is not opened");
                    ceremony::build_ceremony_state_unpersisted()
                }
            });
            // HUP-S6.4 — D-4 deploy gate records (memory only), consulted by contract_deploy.
            app.manage(deploy_gate::DeployGateState::default());
            app.manage(popout_contract::ContractInbox::default());
            // HUP-S9.4 — federated rounds: coordinator setting, start authorizations, eval-gate records.
            app.manage(fl_rounds::build_state(app.handle()));
            // HUP-S1.5 — the escalation router's endpoints + daily spend ledger (lazily loaded).
            app.manage(escalation::EscalationState::default());
            // D-11 — the core model proxy for cloud providers (opened, and its loopback endpoint
            // started, on first use off the main thread).
            app.manage(providers_ipc::ModelProxyState::default());
            // D-11 (P-8) — Cognition delegations: approvals shown and sessions started (memory).
            app.manage(cognition::DelegationState::default());
            // HUP-S2.3 — web-signing budgets (no budgets by default; the store opens on first use).
            app.manage(web_budgets::build_web_budget_state(app.handle()));
            app.manage(faucet::build_faucet_state(app.handle()));
            // Wallet-link — bind THIS device's custody EOA to the member's Citrate
            // identity, through the ceremony above. Until a wallet is bound the
            // authority's `wallet_address` claim is the counterfactual smart-wallet
            // address, which no key can spend from — so the membership money path
            // would bond-fund an address the member cannot reach.
            app.manage(wallet_link::build_link_state());
            // HUP-S8.1 — pending DeviceLink approvals, keyed by ceremony id.
            app.manage(device_link::build_device_link_state());
            // Social verify (ADR-2026-08-30): the pending-verification table, keyed by ceremony id.
            app.manage(social::build_social_bind_state());
            // #61 — in-flight directory publish/revoke ceremonies (bounded by open ceremonies).
            app.manage(social::build_directory_pending_state());
            // CORE-C1.1 — the NodeManager: the real citrate-node under the
            // SidecarSupervisor with an encrypted data dir. @rule8: the 32-byte
            // storage master key lives in the OS keyring (never on disk clear)
            // and is handed to the spawned node in an owner-only key file (CITRATE_STORAGE_KEY_FILE).
            // node_status reads REAL height/peers from the node's local RPC.
            let node_state = node::build_node_state(&app.handle().clone())?;
            // Settings › Node CPU cap, from the persisted config, for starts before the first
            // `node_start` (which re-reads it).
            if let Ok(cfg) = config::config_read_sync(app.handle().clone()) {
                node_state.0.set_cpu_cap(cfg.cpu_cap);
            }
            app.manage(node_state);
            // Mining watchdog: an armed producer is disarmed (respawned as a follower) as soon
            // as the mining gate stops allowing production (`mining_gate`). Off the main thread.
            node::spawn_mining_watchdog(app.handle().clone());
            // Core #326: a node that followed a withdrawn branch moves its chain data aside.
            node::spawn_withdrawn_watch(app.handle().clone());
            // CORE-D2: a running node whose head stands still while it has peers and the network
            // moves on is restarted (never in initial sync, with a growing gap between restarts).
            node::spawn_liveness_watch(app.handle().clone());
            // CORE-D3: the main window hears when a sidecar fails (and when it comes back).
            services::spawn_alert_watch(app.handle().clone());
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
            // key lives in the OS keyring and is handed to the daemon in an
            // owner-only key file named by CITRATE_MEM_STORE_KEY_FILE (see
            // memory.rs). recall/search/neighbors speak the
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
            // K2: the one-time consents remote chat sends need (in memory only).
            app.manage(chat_consent::ChatConsentState::default());
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
            // HUP-S4.2 — the citrate-node MCP server. OFF unless the member turned it on in
            // Settings, or (owner 2026-11-14: on for new installs only) a real first run turned it
            // on above; loopback only; every request needs a connect token; writes need approval.
            // 0.5.4: provision Hermes (data folders, pinned starter capsules) BEFORE the node MCP
            // state is built. Owner 2026-11-14: on for new installs only. A real first run also
            // offers Hermes the node MCP read tools and turns the server on; an upgraded install
            // keeps both off (its Setup card offers "Turn on node tools").
            hermes_setup::provision(app.handle(), had_existing_data);
            app.manage(node_mcp::build_node_mcp_state(&app.handle().clone()));
            // HUP-S1.1 (US-1.1 AC3): answer core-hosted tool calls of sessions the chat view is not
            // watching (turns from the node MCP server or the CLI), instead of leaving them to the
            // sidecar's 300 s deadline. Idle while the Hermes sidecar is not running.
            hermes_headless::spawn(app.handle().clone());
            // L0-WP04: each lane's setup step (`lanes/l<N>/hooks.rs`), l0 to l9, LAST: after the
            // whole existing setup body (the network and payouts gates were installed before the
            // builder). The first failing hook stops start-up with its lane id in the error.
            crate::lanes::setup_all(app)?;
            Ok(())
        })
        .invoke_handler(crate::v60_all_commands!())
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
                // L0-WP04: each lane's quit step (`lanes/l<N>/hooks.rs`), l0 to l9, after the
                // teardown. ExitRequested and Exit both reach here, so a quit hook may run twice.
                crate::lanes::on_exit_all(app_handle);
            }
        });
}

/// Stop every supervised sidecar so none is orphaned across an app quit. Managed states expose the
/// SidecarSupervisor via `.0.stop()`; the lazily-started daemons expose a module `shutdown()`. Every
/// stop is idempotent and a no-op when that sidecar was never started, so this is safe to call once
/// on exit regardless of what the session actually launched.
pub(crate) fn shutdown_all_sidecars(app: &tauri::AppHandle) {
    use tauri::Manager;
    // v0.5.0 C3: the knowledge importer (`mem-mcp import-corpus`) is a one-shot child, not a
    // supervised sidecar. Stop it first (it holds the memory store's RocksDB LOCK), which also
    // closes the import registry so no import starts and the memory daemon is not restarted.
    knowledge_import::shutdown();
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

    /// The release version is declared in four places (tauri.conf.json, src-tauri/Cargo.toml,
    /// package.json and package-lock.json; the root Cargo.lock is held by `--locked`). The updater
    /// compares tauri.conf's, so a bump that misses one ships a build whose parts disagree.
    #[test]
    fn package_json_and_lock_versions_match_cargo_version() {
        let pkg: serde_json::Value = serde_json::from_str(include_str!("../../package.json"))
            .expect("package.json must be valid JSON");
        assert_eq!(pkg["version"], env!("CARGO_PKG_VERSION"), "package.json");
        let lock: serde_json::Value = serde_json::from_str(include_str!("../../package-lock.json"))
            .expect("package-lock.json must be valid JSON");
        assert_eq!(
            lock["version"],
            env!("CARGO_PKG_VERSION"),
            "package-lock.json"
        );
        assert_eq!(
            lock["packages"][""]["version"],
            env!("CARGO_PKG_VERSION"),
            "package-lock.json packages[\"\"]"
        );
    }

    /// ADV-8 boundary, asserted structurally against the real sources: the invoke_handler
    /// registers exactly the `#[tauri::command]`s defined in kit's custody.rs (whose return types
    /// the kit ADV-8 test pins), and NEVER `custody_get` (the in-process secret-reading pub fn).
    #[test]
    fn no_custody_invoke_command_returns_secret_bytes() {
        let src = crate::commands::registry_source();
        let registered: std::collections::BTreeSet<String> = crate::commands::registered_paths()
            .iter()
            .filter_map(|p| p.strip_prefix("custody::"))
            .map(str::to_string)
            .collect();
        let kit = include_str!("../../kit/src/custody.rs");
        let mut defined = std::collections::BTreeSet::new();
        let mut lines = kit.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "#[tauri::command]" {
                continue;
            }
            let sig = lines.find(|l| l.contains("fn ")).expect("command fn");
            let name: String = sig[sig.find("fn ").expect("fn") + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            defined.insert(name);
        }
        assert!(
            defined.len() >= 7,
            "the scan must see the custody commands: {defined:?}"
        );
        // 0.5.4: kit custody commands citrate-core deliberately does not register (nothing in the
        // webview called them; custody_put was an invoke-path vault write). citrate-quorum, which
        // shares the kit, may still register them.
        let not_registered: std::collections::BTreeSet<String> =
            ["custody_put", "custody_keyring_status"]
                .into_iter()
                .map(String::from)
                .collect();
        let expected: std::collections::BTreeSet<String> =
            defined.difference(&not_registered).cloned().collect();
        assert_eq!(
            registered, expected,
            "the registered custody commands differ from those defined in kit custody.rs"
        );
        // The secret-reading API must NEVER be registered as an invoke command.
        // Registration would take the form `<mod>::<fn>,` inside a lane's
        // command list. The needle is assembled from parts so
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
                    || code.contains("recheck_mining(")
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
    /// lib.rs; the real command registry lives in THIS crate, `commands/l<N>.rs`). The
    /// wallet secret-touching fns (create / import / address / sign_message) are
    /// plain library fns and must NEVER be registered as invoke commands.
    /// NEGATIVE CONTROL: registering any of those fns as a handler entry (a
    /// `wallet::<fn>` line ending in a comma, in a lane's command list)
    /// fails this test. Needles are assembled from parts so this test's own
    /// prose cannot self-match `registry_source()`: do not write a literal
    /// `wallet` + `::` + fn-name + comma in the prose of lib.rs or any `commands/` file.
    #[test]
    fn no_wallet_secret_path_fn_is_an_invoke_command() {
        let src = crate::commands::registry_source();
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
    /// the command registry lives in THIS crate (`commands/l<N>.rs`), so the
    /// "these commands are wired" assertions (formerly in the kit's oidc/ceremony
    /// tests, which after the split could only see the kit lib.rs) belong here.
    /// The kit-side tests retain their kit-SOURCE structural assertions
    /// (AuthStatus carries no token field, return types, Signature is secret-free).
    #[test]
    fn auth_commands_are_registered() {
        let src = crate::commands::registry_source();
        for cmd in [
            "auth_status",
            "auth_login",
            "auth_login_cancel",
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
        let src = crate::commands::registry_source();
        for cmd in [
            "sign_request",
            "sign_approve",
            "sign_and_broadcast",
            "sign_prepare",
            "sign_send_prepared",
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
