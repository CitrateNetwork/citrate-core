//! HUP-S8.4 prep (US-8.2 "mesh on by default"): the policy that decides whether the cluster daemon
//! runs the cross-machine libp2p transport, built now and OFF by default.
//!
//! Today the transport is on only when the operator sets `CITRATE_CLUSTER_LISTEN` (CL-S3, soak-gated).
//! US-8.2 asks for the mesh to be on by default once it is safe. That needs, in this order:
//!
//! 1. the CL-S4 transport sign-off (citrate-cluster Rule 8, security lead + owner):
//!    [`TRANSPORT_SIGNED_OFF`];
//! 2. a daemon that serves every group from one process (today a libp2p daemon serves exactly one
//!    `CITRATE_CLUSTER_GROUP`): [`MULTI_GROUP_DAEMON`];
//! 3. the two-machine soak and a 50-node ladder step on separate machines (DGX team).
//!
//! Both constants are `false` and pending owner sign-off, so [`decide`] gives exactly today's answer:
//! the operator's env, or off. When both flip, the mesh is on by default on [`DEFAULT_LISTEN`] and a
//! member can turn it off (`<app data>/cluster/mesh.json`, `cluster_mesh_set_enabled`). The member
//! sees the honest state either way (`cluster_mesh_status`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// CL-S4 transport sign-off recorded in this build. PENDING OWNER + SECURITY SIGN-OFF: `false`.
pub(crate) const TRANSPORT_SIGNED_OFF: bool = false;

/// The daemon serves every group from one process (today: one group per libp2p daemon). `false`.
pub(crate) const MULTI_GROUP_DAEMON: bool = false;

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
        // The shipped constants: neither precondition is met (a tripwire: flipping either one is an
        // owner + security decision and must update this test on purpose).
        assert_eq!(
            [TRANSPORT_SIGNED_OFF, MULTI_GROUP_DAEMON],
            [false, false],
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
