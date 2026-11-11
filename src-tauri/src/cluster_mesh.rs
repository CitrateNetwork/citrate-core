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
/// V60 MESH-2: it may be `true` only with a sign-off record at `release/signoffs/CL-S4.json` that
/// VERIFIES: the owner's and the security lead's EIP-191 signatures recover to the keys registered
/// for those roles in `release/signoffs/signers.json`, and the packet copy committed beside the
/// record hashes to the signed sha256. The test `signed_flag_requires_signoff_record` fails the
/// build otherwise.
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

/// V60 MESH-2: the CL-S4 sign-off record and its check. `validate` checks the shape; `verify`
/// (required before `TRANSPORT_SIGNED_OFF` may be true) checks each role's signature against the
/// registered role key and the committed packet copy against the signed digest. It is a build gate: only the tests read it
/// (the record is evidence for whoever flips [`TRANSPORT_SIGNED_OFF`], not something the app loads).
#[cfg(test)]
mod signoff {
    use serde::Deserialize;

    /// Where the record lives, relative to `src-tauri/`.
    pub(super) const RECORD: &str = "../release/signoffs/CL-S4.json";
    /// The exact packet the signers reviewed, committed beside the record; its SHA-256 must equal
    /// the record's `packet.sha256` (a reviewer diffs it against citrate-cluster at `packet.rev`).
    pub(super) const PACKET: &str = "../release/signoffs/CL-S4_SIGNOFF_PACKET.md";
    /// Who may sign each role: `{ "owner": "<address>", "security-lead": "<address>" }`. Set by the
    /// owner; a role missing here means no record can verify.
    pub(super) const SIGNERS: &str = "../release/signoffs/signers.json";

    /// The roles that must both sign, each by a different person.
    const ROLES: [&str; 2] = ["owner", "security-lead"];

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Record {
        pub wp: String,
        pub packet: Packet,
        pub signers: Vec<Signer>,
    }

    /// The packet that was reviewed, pinned to a commit and a digest of the file at that commit.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Packet {
        pub repo: String,
        pub path: String,
        pub rev: String,
        pub sha256: String,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Signer {
        pub role: String,
        pub github: String,
        pub date: String,
        /// EIP-191 signature (65 bytes, hex) over [`signing_message`] by the role's registered key.
        pub sig: String,
    }

    /// What a signer signs: the role, the person, the date and the pinned packet, one field per
    /// line so no field can run into another.
    pub(super) fn signing_message(
        role: &str,
        github: &str,
        date: &str,
        packet: &serde_json::Value,
    ) -> String {
        let f = |k: &str| packet[k].as_str().unwrap_or_default().to_string();
        format!(
            "citrate-core CL-S4 sign-off v1\nrole: {role}\ngithub: {github}\ndate: {date}\nrepo: {}\npath: {}\nrev: {}\nsha256: {}",
            f("repo"),
            f("path"),
            f("rev"),
            f("sha256")
        )
    }

    /// Full check: [`validate`], then the committed packet copy hashes to `packet.sha256`, the
    /// registry names a different address for each role, and each role's signature over
    /// [`signing_message`] recovers to that role's registered address.
    pub(super) fn verify(text: &str, packet: &[u8], registry: &str) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        let r = validate(text)?;
        if hex::encode(Sha256::digest(packet)) != r.packet.sha256.to_ascii_lowercase() {
            return Err("the committed packet does not match the signed sha256".into());
        }
        let reg: std::collections::BTreeMap<String, String> =
            serde_json::from_str(registry).map_err(|e| format!("signers registry: {e}"))?;
        let canon = |a: &str| a.trim().trim_start_matches("0x").to_ascii_lowercase();
        let packet_json = serde_json::json!({
            "repo": r.packet.repo, "path": r.packet.path,
            "rev": r.packet.rev, "sha256": r.packet.sha256,
        });
        let mut seen: Vec<String> = Vec::new();
        for role in ROLES {
            let want = reg
                .get(role)
                .map(|a| canon(a))
                .ok_or_else(|| format!("no {role} registered in the signers registry"))?;
            if seen.contains(&want) {
                return Err("the owner and the security lead must be different keys".into());
            }
            seen.push(want.clone());
            let s = r
                .signers
                .iter()
                .find(|s| s.role == role)
                .ok_or_else(|| format!("no {role} signature"))?;
            let msg = signing_message(role, &s.github, &s.date, &packet_json);
            if crate::device_link::recover_eip191(&msg, &s.sig).as_deref() != Some(want.as_str()) {
                return Err(format!(
                    "the {role} signature is not by the registered {role} key"
                ));
            }
        }
        Ok(())
    }

    fn hex_of_len(s: &str, n: usize) -> bool {
        s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit())
    }

    /// `YYYY-MM-DD` with a plausible month and day.
    fn is_date(s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return false;
        }
        let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().ok();
        matches!(
            (num(0..4), num(5..7), num(8..10)),
            (Some(y), Some(m), Some(d)) if y >= 2026 && (1..=12).contains(&m) && (1..=31).contains(&d)
        )
    }

    pub(super) fn validate(text: &str) -> Result<Record, String> {
        let r: Record =
            serde_json::from_str(text).map_err(|e| format!("not a sign-off record: {e}"))?;
        if r.wp != "CL-S4" {
            return Err(format!("record is for {}, not CL-S4", r.wp));
        }
        if r.packet.repo != "CitrateNetwork/citrate-cluster" || r.packet.path.trim().is_empty() {
            return Err("the packet must be a file in CitrateNetwork/citrate-cluster".into());
        }
        if !hex_of_len(&r.packet.rev, 40) {
            return Err("packet.rev must be a full commit id".into());
        }
        if !hex_of_len(&r.packet.sha256, 64) {
            return Err("packet.sha256 must be the file's SHA-256".into());
        }
        let mut people: Vec<String> = Vec::new();
        for role in ROLES {
            let s = r
                .signers
                .iter()
                .find(|s| s.role == role)
                .ok_or_else(|| format!("no {role} signature"))?;
            if s.github.trim().is_empty() || !is_date(&s.date) {
                return Err(format!(
                    "the {role} signature needs a GitHub handle and a date"
                ));
            }
            if !hex_of_len(s.sig.trim_start_matches("0x"), 130) {
                return Err(format!("the {role} signature must be 65 bytes of hex"));
            }
            let who = s.github.trim().to_ascii_lowercase();
            if people.contains(&who) {
                return Err("the owner and the security lead must be different people".into());
            }
            people.push(who);
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V60 MESH-2 (F-C3): `TRANSPORT_SIGNED_OFF` follows the CL-S4 sign-off record, not an edit.
    /// With the flag true, `release/signoffs/CL-S4.json` must exist and name the owner and the
    /// security lead against a pinned packet; without the record, flipping the flag fails the build.
    #[test]
    fn signed_flag_requires_signoff_record() {
        // Review P2 (#631): a well-formed record is not enough. With the flag true, the record must
        // VERIFY: each role's signature recovers to the address registered for that role, and the
        // packet copy committed beside the record hashes to the record's sha256.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |rel: &str| std::fs::read(dir.join(rel));
        if TRANSPORT_SIGNED_OFF {
            let missing = |what: &str| panic!("TRANSPORT_SIGNED_OFF is true but {what} is missing");
            let record = read(signoff::RECORD).unwrap_or_else(|_| missing(signoff::RECORD));
            let packet = read(signoff::PACKET).unwrap_or_else(|_| missing(signoff::PACKET));
            let signers = read(signoff::SIGNERS).unwrap_or_else(|_| missing(signoff::SIGNERS));
            signoff::verify(
                &String::from_utf8_lossy(&record),
                &packet,
                &String::from_utf8_lossy(&signers),
            )
            .expect("the CL-S4 sign-off record verifies");
        } else if let Ok(record) = read(signoff::RECORD) {
            // A record may land before the flag flips; it must still be well formed.
            signoff::validate(&String::from_utf8_lossy(&record))
                .expect("the CL-S4 sign-off record is valid");
        }
    }

    /// Review P2 (#631): signatures and the packet digest are checked, not just their format.
    #[test]
    fn a_signoff_record_verifies_only_with_registered_signers_and_the_signed_packet() {
        use sha2::{Digest, Sha256};
        let owner_key = "11".repeat(32);
        let sec_key = "22".repeat(32);
        let other_key = "33".repeat(32);
        let addr = |k: &str| crate::comms::address_from_secret_hex(k).expect("address");
        let packet = b"# CL-S4 packet\nclaims and evidence\n".to_vec();
        let sha = hex::encode(Sha256::digest(&packet));
        let base = |owner_sig_key: &str, sec_sig_key: &str, packet_sha: &str| {
            let pk = serde_json::json!({
                "repo": "CitrateNetwork/citrate-cluster",
                "path": ".agentile/planset/CL-S4_SIGNOFF_PACKET.md",
                "rev": "755cf217226094147e4e67909ab96ccbb5aeeb2a",
                "sha256": packet_sha,
            });
            let signer = |role: &str, gh: &str, date: &str, key: &str| {
                let msg = signoff::signing_message(role, gh, date, &pk);
                let sig = crate::device_link::sign_eip191(key, &msg).expect("sign");
                serde_json::json!({ "role": role, "github": gh, "date": date, "sig": sig })
            };
            serde_json::json!({
                "wp": "CL-S4",
                "packet": pk,
                "signers": [
                    signer("owner", "SaulBuilds", "2026-11-20", owner_sig_key),
                    signer("security-lead", "kurtatwork", "2026-11-21", sec_sig_key),
                ],
            })
        };
        let registry =
            serde_json::json!({ "owner": addr(&owner_key), "security-lead": addr(&sec_key) })
                .to_string();
        let good = base(&owner_key, &sec_key, &sha).to_string();
        assert!(signoff::verify(&good, &packet, &registry).is_ok());

        // Signed by a key that is not the registered one for its role.
        let wrong = base(&other_key, &sec_key, &sha).to_string();
        assert!(
            signoff::verify(&wrong, &packet, &registry).is_err(),
            "unregistered owner key"
        );
        // The committed packet copy is not the one that was signed.
        assert!(
            signoff::verify(&good, b"another packet", &registry).is_err(),
            "packet changed"
        );
        // A field changed after signing.
        let tampered = good.replace("2026-11-21", "2026-11-22");
        assert!(
            signoff::verify(&tampered, &packet, &registry).is_err(),
            "date changed after signing"
        );
        // The registry must name two different signers, one per role.
        let one =
            serde_json::json!({ "owner": addr(&owner_key), "security-lead": addr(&owner_key) })
                .to_string();
        let same = base(&owner_key, &owner_key, &sha).to_string();
        assert!(
            signoff::verify(&same, &packet, &one).is_err(),
            "one key for both roles"
        );
        let partial = serde_json::json!({ "owner": addr(&owner_key) }).to_string();
        assert!(
            signoff::verify(&good, &packet, &partial).is_err(),
            "no security-lead registered"
        );
        assert!(signoff::verify(&good, &packet, "not json").is_err());
    }

    #[test]
    fn a_signoff_record_needs_the_owner_and_the_security_lead_on_a_pinned_packet() {
        let good = serde_json::json!({
            "wp": "CL-S4",
            "packet": {
                "repo": "CitrateNetwork/citrate-cluster",
                "path": ".agentile/planset/CL-S4_SIGNOFF_PACKET.md",
                "rev": "755cf217226094147e4e67909ab96ccbb5aeeb2a",
                "sha256": "a".repeat(64),
            },
            "signers": [
                { "role": "owner", "github": "SaulBuilds", "date": "2026-11-20", "sig": format!("0x{}", "ab".repeat(65)) },
                { "role": "security-lead", "github": "kurtatwork", "date": "2026-11-21", "sig": format!("0x{}", "cd".repeat(65)) },
            ],
        });
        assert!(signoff::validate(&good.to_string()).is_ok());

        let mutate = |f: &dyn Fn(&mut serde_json::Value)| {
            let mut v = good.clone();
            f(&mut v);
            signoff::validate(&v.to_string())
        };
        assert!(mutate(&|v| v["wp"] = "CL-S3".into()).is_err(), "another WP");
        assert!(
            mutate(&|v| v["packet"]["repo"] = "someone/else".into()).is_err(),
            "another repo"
        );
        assert!(
            mutate(&|v| v["packet"]["rev"] = "main".into()).is_err(),
            "a branch, not a commit"
        );
        assert!(
            mutate(&|v| v["packet"]["sha256"] = "abc".into()).is_err(),
            "no packet digest"
        );
        assert!(
            mutate(&|v| v["signers"] = serde_json::json!([v["signers"][0].clone()])).is_err(),
            "the owner alone"
        );
        assert!(
            mutate(&|v| v["signers"][1]["github"] = "SaulBuilds".into()).is_err(),
            "one person in both roles"
        );
        assert!(
            mutate(&|v| v["signers"][0]["date"] = "soon".into()).is_err(),
            "a date that is not a date"
        );
        assert!(signoff::validate("not json").is_err());
    }

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
