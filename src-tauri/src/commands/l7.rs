//! The app commands lane L7 registers (networking).
//! L0-WP03, planset v060 L0_INTEGRATION_SEAMS section 3.1.
//!
//! Only L7 edits this file. A command added here is also allowed in
//! `permissions/l7-commands.toml`; `seams_tests` fails when the two differ. Write each path as
//! `module::command,` on its own line (no `crate::`): see `commands/mod.rs` for why.

#[macro_export]
#[doc(hidden)]
macro_rules! v60_commands_l7 {
    ($($rest:ident)* [$($acc:tt)*]) => {
        $crate::v60_commands_next!($($rest)* [$($acc)*
            // HUP-S8.2/S8.3 — fleet wizard: probe, opt-in mDNS, link/QR pairing, Tailscale (read-only).
            fleet::fleet_probe,
            fleet::fleet_set_label,
            fleet::fleet_roster,
            fleet::fleet_discovery_set,
            fleet::fleet_discovery_browse,
            fleet::fleet_pair_create,
            fleet::fleet_pair_inspect,
            fleet::fleet_pair_join,
            fleet::fleet_forget,
            fleet::fleet_tailscale,
            cluster::cluster_status,
            cluster::cluster_join,
            cluster::cluster_peers,
            cluster::cluster_share_file,
            cluster::cluster_leave,
            cluster::cluster_devices,
            cluster::cluster_group_seed,
            cluster::cluster_add_seed,
            device_link::device_link_request,
            device_link::device_link_approve,
            device_link::device_link_reject,
            device_link::device_links,
            device_link::device_link_revoke_prepare,
            device_link::device_link_revoke,
            device_link::device_link_export,
            device_link::device_link_import,
            device_link_share::device_links_share_offer,
            device_link_share::device_links_mark_shared,
            device_link_share::device_links_ingest,
            cluster_mesh::cluster_mesh_status,
            cluster_mesh::cluster_mesh_set_enabled,
        ])
    };
}
