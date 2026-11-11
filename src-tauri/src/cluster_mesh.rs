//! HUP-S8.4 prep (US-8.2 "mesh on by default"): the policy that decides whether the cluster daemon
//! runs the cross-machine libp2p transport, built now and OFF by default.
//!
//! Today the transport is on only when the operator sets `CITRATE_CLUSTER_LISTEN` (CL-S3, soak-gated).
//! US-8.2 asks for the mesh to be on by default once it is safe. That needs, in this order:
//!
//! 1. the CL-S4 transport sign-off (citrate-cluster Rule 8, security lead + owner):
//!    [`TRANSPORT_SIGNED_OFF`];
//! 2. a daemon that serves every group from one process: [`MULTI_GROUP_DAEMON`]. citrate-cluster
//!    main serves every group (HUP-S8.4: one swarm per group, group seeds, opt-in mDNS; merged in
//!    citrate-cluster #11), and the bundled daemon is built from main at the rev pinned in
//!    `release/components.json`. V60 MESH-1: the flag is `true`, and a test ties it to the pin
//!    ([`MULTI_GROUP_DAEMON_REVS`]), so a pin bump to an unchecked rev fails the build;
//! 3. the two-machine soak and a 50-node ladder step on separate machines (DGX team).
//!
//! [`TRANSPORT_SIGNED_OFF`] is still `false` (pending owner + security sign-off), so [`decide`] gives
//! exactly today's answer: the operator's env, or off. When both flip, the mesh is on by default on [`DEFAULT_LISTEN`] and a
//! member can turn it off (`<app data>/cluster/mesh.json`, `cluster_mesh_set_enabled`). The member
//! sees the honest state either way (`cluster_mesh_status`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// CL-S4 transport sign-off recorded in this build. PENDING OWNER + SECURITY SIGN-OFF: `false`.
pub(crate) const TRANSPORT_SIGNED_OFF: bool = false;

/// The bundled daemon serves every group from one process (citrate-cluster HUP-S8.4, on main since
/// #11). V60 MESH-1: `true`, because the pinned `cluster-daemon` rev is in
/// [`MULTI_GROUP_DAEMON_REVS`]. On its own this turns nothing on: the mesh default also needs
/// [`TRANSPORT_SIGNED_OFF`].
pub(crate) const MULTI_GROUP_DAEMON: bool = true;

/// V60 MESH-1: the citrate-cluster revs checked to serve every group from one daemon process. The
/// test `the_multi_group_flag_matches_the_bundled_daemon` requires the `cluster-daemon` rev pinned in
/// `release/components.json` to be one of these while [`MULTI_GROUP_DAEMON`] is true. Add a rev here
/// only after checking that daemon (citrate-cluster `tests/multigroup_multiprocess.rs` and
/// `tests/mesh1_multigroup.rs` green at that rev).
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const MULTI_GROUP_DAEMON_REVS: &[&str] = &[
    // citrate-cluster main, P-fleet (#11); shipped in 0.5.0 and 0.5.1.
    "755cf217226094147e4e67909ab96ccbb5aeeb2a",
];

/// Listen address when the mesh is on by default. PENDING OWNER SIGN-OFF (port 4211, all interfaces).
pub(crate) const DEFAULT_LISTEN: &str = "/ip4/0.0.0.0/tcp/4211";

const ENV_LISTEN: &str = "CITRATE_CLUSTER_LISTEN";

/// Where the mesh decision came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MeshSource {
    /// `CITRATE_CLUSTER_LISTEN` is set.
    Operator,
    /// On by default (both preconditions met, member did not turn it off).
    Default,
    /// Off.
    Off,
}

/// The decision: the listen address to use, or none (in-process transport).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MeshDecision {
    pub source: MeshSource,
    pub listen: Option<String>,
}

/// The policy. `env_listen` is the operator's `CITRATE_CLUSTER_LISTEN`; `member_off` is the member's
/// own off switch. The operator's setting always wins.
pub(crate) fn decide(
    env_listen: Option<&str>,
    signed_off: bool,
    multi_group: bool,
    member_off: bool,
) -> MeshDecision {
    if let Some(l) = env_listen.map(str::trim).filter(|l| !l.is_empty()) {
        return MeshDecision {
            source: MeshSource::Operator,
            listen: Some(l.to_string()),
        };
    }
    if signed_off && multi_group && !member_off {
        return MeshDecision {
            source: MeshSource::Default,
            listen: Some(DEFAULT_LISTEN.to_string()),
        };
    }
    MeshDecision {
        source: MeshSource::Off,
        listen: None,
    }
}

/// What the member is told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MeshStatus {
    pub on: bool,
    pub source: MeshSource,
    pub signed_off: bool,
    pub note: String,
}

pub(crate) fn status_for(
    d: &MeshDecision,
    signed_off: bool,
    multi_group: bool,
    member_off: bool,
) -> MeshStatus {
    let note = match d.source {
        MeshSource::Operator => {
            "The cross-machine mesh is on (an operator setting on this machine).".to_string()
        }
        MeshSource::Default => "The cross-machine mesh is on.".to_string(),
        MeshSource::Off if !signed_off => "The cross-machine mesh is off. It turns on by default \
            once its security review is signed off; until then only an operator can turn it on."
            .to_string(),
        MeshSource::Off if !multi_group => "The cross-machine mesh is off. It turns on by default \
            once one Citrate Core can mesh every group you are in."
            .to_string(),
        MeshSource::Off if member_off => "You turned the cross-machine mesh off.".to_string(),
        MeshSource::Off => "The cross-machine mesh is off.".to_string(),
    };
    MeshStatus {
        on: d.listen.is_some(),
        source: d.source,
        signed_off,
        note,
    }
}

/// The member's switch, `<app data>/cluster/mesh.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MemberMesh {
    #[serde(default)]
    off: bool,
}

fn member_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::device_link::store_path(app)?.with_file_name("mesh.json"))
}

fn member_off(app: &tauri::AppHandle) -> bool {
    member_file(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<MemberMesh>(&b).ok())
        .is_some_and(|m| m.off)
}

/// The decision for this app run (env + build constants + the member's switch).
pub(crate) fn current(app: &tauri::AppHandle) -> MeshDecision {
    let env = std::env::var(ENV_LISTEN).ok();
    decide(
        env.as_deref(),
        TRANSPORT_SIGNED_OFF,
        MULTI_GROUP_DAEMON,
        member_off(app),
    )
}

/// **cluster_mesh_status**: whether the cross-machine mesh is on, why, and whether the transport
/// sign-off is recorded.
#[tauri::command]
pub async fn cluster_mesh_status(app: tauri::AppHandle) -> Result<MeshStatus, String> {
    crate::blocking::off_main(move || {
        let off = member_off(&app);
        let d = current(&app);
        Ok(status_for(
            &d,
            TRANSPORT_SIGNED_OFF,
            MULTI_GROUP_DAEMON,
            off,
        ))
    })
    .await
}

/// **cluster_mesh_set_enabled**: the member's own switch. Refused until the transport is signed off
/// (before that, only the operator's setting turns the mesh on). Takes effect at the next daemon
/// start.
#[tauri::command]
pub async fn cluster_mesh_set_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<MeshStatus, String> {
    crate::blocking::off_main(move || {
        if !TRANSPORT_SIGNED_OFF {
            return Err(
                "The cross-machine mesh cannot be turned on here until its security review is \
                 signed off."
                    .into(),
            );
        }
        let path = member_file(&app)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        let json = serde_json::to_vec(&MemberMesh { off: !enabled }).map_err(|e| e.to_string())?;
        std::fs::write(&path, json)
            .map_err(|e| format!("saving the mesh setting: {}", e.kind()))?;
        let d = current(&app);
        Ok(status_for(
            &d,
            TRANSPORT_SIGNED_OFF,
            MULTI_GROUP_DAEMON,
            !enabled,
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_build_changes_nothing_the_operator_env_or_off() {
        // The shipped constants (a tripwire: flipping either one is an owner + security decision
        // and must update this test on purpose). V60 MESH-1: the bundled daemon serves every group;
        // the transport sign-off is still open, so the mesh default is unchanged (off).
        assert_eq!(
            [TRANSPORT_SIGNED_OFF, MULTI_GROUP_DAEMON],
            [false, true],
            "the mesh default changed"
        );
        let off = decide(None, TRANSPORT_SIGNED_OFF, MULTI_GROUP_DAEMON, false);
        assert_eq!(off.source, MeshSource::Off);
        assert_eq!(off.listen, None);
        let op = decide(
            Some("/ip4/0.0.0.0/tcp/0"),
            TRANSPORT_SIGNED_OFF,
            MULTI_GROUP_DAEMON,
            false,
        );
        assert_eq!(op.source, MeshSource::Operator);
        assert_eq!(op.listen.as_deref(), Some("/ip4/0.0.0.0/tcp/0"));
        // A blank env value is not an operator setting.
        assert_eq!(
            decide(Some("  "), false, false, false).source,
            MeshSource::Off
        );
    }

    /// V60 MESH-1: `MULTI_GROUP_DAEMON` may be true only while the bundled `cluster-daemon` is a
    /// citrate-cluster rev recorded as serving every group. Bumping the pin in
    /// `release/components.json` without checking the new rev (and recording it) fails here.
    #[test]
    fn the_multi_group_flag_matches_the_bundled_daemon() {
        let components: serde_json::Value =
            serde_json::from_str(include_str!("../../release/components.json"))
                .expect("release/components.json parses");
        let rev = components["components"]
            .as_array()
            .and_then(|cs| cs.iter().find(|c| c["id"] == "cluster-daemon"))
            .and_then(|c| c["source"]["rev"].as_str())
            .expect("cluster-daemon is pinned");
        if MULTI_GROUP_DAEMON {
            assert!(
                MULTI_GROUP_DAEMON_REVS.contains(&rev),
                "bundled cluster-daemon {rev} is not recorded as multi-group"
            );
        }
        // Every recorded rev is a full 40-hex commit id.
        for r in MULTI_GROUP_DAEMON_REVS {
            assert!(
                r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()),
                "{r}"
            );
        }
    }

    #[test]
    fn default_on_needs_both_preconditions_and_respects_the_member() {
        assert_eq!(decide(None, true, false, false).source, MeshSource::Off);
        assert_eq!(decide(None, false, true, false).source, MeshSource::Off);
        let on = decide(None, true, true, false);
        assert_eq!(on.source, MeshSource::Default);
        assert_eq!(on.listen.as_deref(), Some(DEFAULT_LISTEN));
        assert_eq!(decide(None, true, true, true).source, MeshSource::Off);
        // The operator wins over the member's switch.
        assert_eq!(
            decide(Some("/ip4/127.0.0.1/tcp/1"), true, true, true).source,
            MeshSource::Operator
        );
    }

    #[test]
    fn the_member_is_told_why_in_plain_words() {
        let off = decide(None, false, false, false);
        let s = status_for(&off, false, false, false);
        assert!(!s.on && !s.signed_off);
        assert!(s.note.contains("security review"), "{}", s.note);
        let s = status_for(&decide(None, true, false, false), true, false, false);
        assert!(s.note.contains("every group"), "{}", s.note);
        let s = status_for(&decide(None, true, true, true), true, true, true);
        assert!(s.note.contains("You turned"), "{}", s.note);
        let s = status_for(
            &decide(Some("/ip4/0.0.0.0/tcp/0"), false, false, false),
            false,
            false,
            false,
        );
        assert!(s.on && s.note.contains("operator"));
    }
}
