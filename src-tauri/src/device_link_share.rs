//! HUP-S8.1 follow-on: other members' DeviceLinks, shared over the group relay.
//!
//! ## Why
//!
//! A machine that meshes under its own device key is admitted by another member's node only if that
//! node knows the device's signed link. Until now a member could only paste links between their OWN
//! machines, so with the cross-machine transport on, one member's linked machines were refused by
//! everybody else's nodes. This module carries each member's links and revocations to the other
//! members of the groups they share, over the end-to-end encrypted group relay (server-blind), the
//! same way verified social bindings travel: a control message the chat hides.
//!
//! ## Trust
//!
//! The channel is not trusted. Every link is self-verifying (three EIP-191 signatures: member, device,
//! wallet) and every revocation is signed by its member, so a message is accepted only when:
//! * the relay attributes it to the member it speaks for (`sender == member`, our comms roster key),
//! * every signature recovers to the address it names, and
//! * the store stays under its caps.
//!
//! Our own links never come from here (they live in `device_link`'s store). A revocation wins over
//! any link of that device, in either order of arrival, and is kept for good.
//!
//! ## Into the daemon
//!
//! [`roster_update`] builds what `cluster.rs` sends with every roster: this machine's own links first,
//! then peers' links of members on the group roster, never a revoked one, newest revocations first,
//! all within the daemon's per-update caps and its 64 KiB IPC line.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::device_link::{
    canonical_address, recover_eip191, revocation_message, verify_link, DeviceLinkStore,
    DeviceLinkWire, RevocationWire, MAX_LINKS,
};

/// Group-message prefix of a DeviceLink share (hidden from the conversation). It starts with U+0001,
/// like the social-binding sentinel (`\u{1}cbind1:`), so nothing a member types can match it.
pub(crate) const DEVICE_LINKS_MSG_PREFIX: &str = "\u{1}cdlink1:";

/// Longest share message accepted (a member's 64 links are about 45 KiB of JSON).
pub(crate) const MAX_MESSAGE_BYTES: usize = 56 * 1024;

/// Most peer links this machine keeps across all members.
pub(crate) const MAX_PEER_LINKS: usize = 1024;

/// Most peer revocations this machine keeps across all members.
pub(crate) const MAX_PEER_REVOCATIONS: usize = 2048;

/// Budget for one `setRoster` line (the daemon reads at most 64 KiB; keep headroom).
pub(crate) const MAX_UPDATE_LINE: usize = 60 * 1024;

/// The JSON a member shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SharePayload {
    pub v: u8,
    #[serde(default)]
    pub links: Vec<DeviceLinkWire>,
    #[serde(default)]
    pub revocations: Vec<RevocationWire>,
}

/// A message ready to send to a group, and the digest that says whether this exact set was sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareOffer {
    pub body: String,
    pub digest: String,
}

/// This member's own links and revocations as a share message, or `None` when there is nothing to
/// share (a member who never linked a device sends nothing, so defaults change nothing).
pub(crate) fn share_offer(own: &DeviceLinkStore, own_member: &str) -> Option<ShareOffer> {
    let me = canonical_address(own_member)?;
    let mut links: Vec<DeviceLinkWire> = own
        .links
        .iter()
        .filter(|l| l.member == me)
        .cloned()
        .collect();
    links.sort_by(|a, b| (a.index, &a.device).cmp(&(b.index, &b.device)));
    let mut revocations: Vec<RevocationWire> = own
        .revocations
        .iter()
        .filter(|r| r.member == me)
        .cloned()
        .collect();
    revocations.sort_by(|a, b| (&a.device, a.revoked_at).cmp(&(&b.device, b.revoked_at)));
    if links.is_empty() && revocations.is_empty() {
        return None;
    }
    let json = serde_json::to_string(&SharePayload {
        v: 1,
        links,
        revocations,
    })
    .ok()?;
    let digest = hex::encode(Sha256::digest(json.as_bytes()));
    Some(ShareOffer {
        body: format!("{DEVICE_LINKS_MSG_PREFIX}{json}"),
        digest,
    })
}

/// Other members' links and revocations this machine has accepted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PeerLinkStore {
    #[serde(default)]
    pub links: Vec<DeviceLinkWire>,
    #[serde(default)]
    pub revocations: Vec<RevocationWire>,
}

impl PeerLinkStore {
    /// Load from `path`; a missing file is empty, a corrupt one is an error (never silently empty,
    /// which would drop revocations).
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "the shared device link store is corrupt".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("reading the shared device link store: {}", e.kind())),
        }
    }

    /// Persist owner-only, atomically.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        citrate_core_kit::fsutil::write_secret_file(&tmp, &json)
            .map_err(|e| format!("writing the shared device link store: {}", e.kind()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("saving the shared device link store: {}", e.kind()))
    }

    fn is_revoked(&self, member: &str, device: &str) -> bool {
        self.revocations
            .iter()
            .any(|r| r.member == member && r.device == device)
    }
}

/// What one share message added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IngestReport {
    /// Links added or refreshed.
    pub links: usize,
    /// Revocations newly recorded.
    pub revocations: usize,
    /// Secret-free reasons for refused items (addresses only).
    pub refused: Vec<String>,
}

fn revocation_verifies(r: &RevocationWire) -> bool {
    recover_eip191(&revocation_message(&r.member, &r.device, r.revoked_at), &r.member_sig).as_deref()
        == Some(r.member.as_str())
}

/// Accept a share message `body` (prefix included) that the relay attributes to `sender`. Refuses
/// the whole message when it is not a share, too large or not JSON; refuses single items that do not
/// verify, speak for someone other than `sender`, or would pass a cap. Our own announcements are
/// ignored. Never errors on a well-formed message whose items are all refused.
pub(crate) fn ingest(
    store: &mut PeerLinkStore,
    own_member: &str,
    sender: &str,
    body: &str,
) -> Result<IngestReport, String> {
    if body.len() > MAX_MESSAGE_BYTES {
        return Err("that device link message is too large".into());
    }
    let json = body
        .strip_prefix(DEVICE_LINKS_MSG_PREFIX)
        .ok_or("not a device link message")?;
    let payload: SharePayload =
        serde_json::from_str(json).map_err(|_| "a malformed device link message".to_string())?;
    if payload.v != 1 {
        return Err("an unsupported device link message version".into());
    }
    let mut report = IngestReport::default();
    let Some(sender) = canonical_address(sender) else {
        report.refused.push("sender is not an address".into());
        return Ok(report);
    };
    if Some(sender.as_str()) == canonical_address(own_member).as_deref() {
        return Ok(report); // our own links live in the device_link store
    }

    // 1. Revocations first, so a link and its revocation in one message never admit.
    for r in payload.revocations {
        let (Some(member), Some(device)) = (canonical_address(&r.member), canonical_address(&r.device))
        else {
            report.refused.push("a revocation with a malformed address".into());
            continue;
        };
        if member != sender {
            report.refused.push(format!("{device}: revocation not from its member"));
            continue;
        }
        let r = RevocationWire {
            member,
            device,
            ..r
        };
        if !revocation_verifies(&r) {
            report.refused.push(format!("{}: revocation signature", r.device));
            continue;
        }
        if store.is_revoked(&r.member, &r.device) {
            continue;
        }
        if store.revocations.len() >= MAX_PEER_REVOCATIONS {
            report.refused.push(format!("{}: revocation store full", r.device));
            continue;
        }
        store
            .links
            .retain(|l| !(l.member == r.member && l.device == r.device));
        store.revocations.push(r);
        report.revocations += 1;
    }

    // 2. Links: from the sender, fully verified, not revoked, within the caps.
    for l in payload.links {
        if verify_link(&l).is_err() {
            report.refused.push(format!("{}: link does not verify", l.device));
            continue;
        }
        // `verify_link` validated the addresses; store the canonical form.
        let (Some(member), Some(device), Some(wallet)) = (
            canonical_address(&l.member),
            canonical_address(&l.device),
            canonical_address(&l.wallet),
        ) else {
            continue;
        };
        if member != sender {
            report.refused.push(format!("{device}: link not from its member"));
            continue;
        }
        if store.is_revoked(&member, &device) {
            report.refused.push(format!("{device}: revoked"));
            continue;
        }
        let l = DeviceLinkWire {
            member,
            device,
            wallet,
            ..l
        };
        if let Some(existing) = store
            .links
            .iter_mut()
            .find(|x| x.member == l.member && x.device == l.device)
        {
            if l.issued_at > existing.issued_at {
                *existing = l;
                report.links += 1;
            }
            continue;
        }
        let of_member = store.links.iter().filter(|x| x.member == l.member).count();
        if false || store.links.len() >= MAX_PEER_LINKS {
            report.refused.push(format!("{}: link store full", l.device));
            continue;
        }
        store.links.push(l);
        report.links += 1;
    }
    Ok(report)
}

/// The links and revocations `cluster.rs` sends with a group's roster: own links first, then peers'
/// links of members on `roster` (sorted, never one revoked by either store), newest revocations
/// first; at most [`MAX_LINKS`] of each, trimmed further (peer links first, then the oldest
/// revocations) so the whole `setRoster` line stays under [`MAX_UPDATE_LINE`].
pub(crate) fn roster_update(
    own: &DeviceLinkStore,
    peers: &PeerLinkStore,
    roster: &[(String, String)],
) -> (Vec<DeviceLinkWire>, Vec<RevocationWire>) {
    let members: BTreeSet<String> = roster
        .iter()
        .filter_map(|(a, _)| canonical_address(a))
        .collect();
    let revoked: BTreeSet<(&str, &str)> = own
        .revocations
        .iter()
        .chain(peers.revocations.iter())
        .map(|r| (r.member.as_str(), r.device.as_str()))
        .collect();

    let mut links: Vec<DeviceLinkWire> = own
        .links
        .iter()
        .filter(|l| !revoked.contains(&(l.member.as_str(), l.device.as_str())))
        .cloned()
        .collect();
    let own_count = links.len();
    let mut seen: BTreeSet<String> = links.iter().map(|l| l.device.clone()).collect();
    let mut theirs: Vec<&DeviceLinkWire> = peers
        .links
        .iter()
        .filter(|l| members.contains(&l.member))
        .filter(|l| !revoked.contains(&(l.member.as_str(), l.device.as_str())))
        .collect();
    theirs.sort_by(|a, b| (&a.member, a.index, &a.device).cmp(&(&b.member, b.index, &b.device)));
    for l in theirs {
        if seen.insert(l.device.clone()) {
            links.push(l.clone());
        }
    }
    links.truncate(MAX_LINKS);

    let mut revocations: Vec<RevocationWire> = own
        .revocations
        .iter()
        .chain(
            peers
                .revocations
                .iter()
                .filter(|r| members.contains(&r.member)),
        )
        .cloned()
        .collect();
    revocations.sort_by(|a, b| b.revoked_at.cmp(&a.revoked_at).then(a.device.cmp(&b.device)));
    revocations.dedup_by(|a, b| a.member == b.member && a.device == b.device);
    revocations.truncate(MAX_LINKS);

    // Fit the IPC line: the roster is fixed, so trim what we add.
    let roster_bytes = serde_json::to_string(roster).map(|s| s.len()).unwrap_or(0);
    let size = |l: &[DeviceLinkWire], r: &[RevocationWire]| {
        roster_bytes
            + 256
            + serde_json::to_string(l).map(|s| s.len()).unwrap_or(usize::MAX / 2)
            + serde_json::to_string(r).map(|s| s.len()).unwrap_or(usize::MAX / 2)
    };
    while size(&links, &revocations) > MAX_UPDATE_LINE {
        if links.len() > own_count.min(MAX_LINKS) {
            links.pop();
        } else if revocations.len() > 1 {
            revocations.pop();
        } else if !links.is_empty() {
            links.pop();
        } else {
            break;
        }
    }
    (links, revocations)
}

// ---------------------------------------------------------------------------
// Files + commands
// ---------------------------------------------------------------------------

/// `<app data>/cluster/peer-device-links.json`.
pub(crate) fn peer_store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::device_link::store_path(app)?.with_file_name("peer-device-links.json"))
}

/// `<app data>/cluster/device-links-shared.json`: group id -> digest of the set last sent there.
fn shared_marks_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::device_link::store_path(app)?.with_file_name("device-links-shared.json"))
}

type SharedMarks = std::collections::BTreeMap<String, String>;

fn load_marks(path: &Path) -> SharedMarks {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Most groups remembered in the shared-marks file.
const MAX_MARKS: usize = 256;

/// **device_links_share_offer**: the share message for `group`, or `None` when this member has
/// nothing to share or already sent this exact set to that group.
#[tauri::command]
pub async fn device_links_share_offer(
    app: tauri::AppHandle,
    group: String,
) -> Result<Option<ShareOffer>, String> {
    crate::blocking::off_main(move || {
        let member = crate::comms::device_identity(&app)?;
        let own = DeviceLinkStore::load(&crate::device_link::store_path(&app)?)?;
        let Some(offer) = share_offer(&own, &member.address) else {
            return Ok(None);
        };
        let marks = load_marks(&shared_marks_path(&app)?);
        Ok((marks.get(&group) != Some(&offer.digest)).then_some(offer))
    })
    .await
}

/// **device_links_mark_shared**: record that the set with `digest` was sent to `group`.
#[tauri::command]
pub async fn device_links_mark_shared(
    app: tauri::AppHandle,
    group: String,
    digest: String,
) -> Result<(), String> {
    crate::blocking::off_main(move || {
        if group.is_empty() || group.len() > 128 || digest.len() != 64 {
            return Err("bad share mark".into());
        }
        let path = shared_marks_path(&app)?;
        let mut marks = load_marks(&path);
        marks.insert(group, digest);
        while marks.len() > MAX_MARKS {
            let Some(first) = marks.keys().next().cloned() else {
                break;
            };
            marks.remove(&first);
        }
        let json = serde_json::to_vec(&marks).map_err(|e| e.to_string())?;
        citrate_core_kit::fsutil::write_secret_file(&path, &json)
            .map_err(|e| format!("writing share marks: {}", e.kind()))
    })
    .await
}

/// **device_links_ingest**: accept a share message the relay attributed to `sender`. Returns what
/// was added (never a signature).
#[tauri::command]
pub async fn device_links_ingest(
    app: tauri::AppHandle,
    sender: String,
    body: String,
) -> Result<IngestReport, String> {
    crate::blocking::off_main(move || {
        let member = crate::comms::device_identity(&app)?;
        let path = peer_store_path(&app)?;
        let mut store = PeerLinkStore::load(&path)?;
        let before = store.clone();
        let report = ingest(&mut store, &member.address, &sender, &body)?;
        if store != before {
            store.save(&path)?;
        }
        Ok(report)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("device_link_share_tests.rs");
}
