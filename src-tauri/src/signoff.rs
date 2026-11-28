//! L0-WP09 (planset v060, L0_INTEGRATION_SEAMS 5): the sign-off record check that gates a
//! feature flag, generalized from V60 MESH-2's CL-S4 check (`cluster_mesh.rs`) with its logic
//! unchanged. A flag whose row in `release/flags.json` names a sign-off may be `true` only when
//! `release/signoffs/<id>.json` VERIFIES: the owner's and the security lead's EIP-191 signatures
//! recover to the keys registered for those roles in `release/signoffs/signers.json`, and the
//! packet copy committed beside the record (`<id>_SIGNOFF_PACKET.md`) hashes to the signed sha256.
//!
//! It is a build gate: only tests read it. A record for a security-sensitive item holds only a hash
//! of the reviewed packet, never the findings.

use serde::Deserialize;

/// Who may sign each role, repo-relative. Set by the owner; a role missing here means no record
/// can verify.
pub(crate) const SIGNERS: &str = "release/signoffs/signers.json";

/// The roles that must both sign, each by a different person.
const ROLES: [&str; 2] = ["owner", "security-lead"];

/// One sign-off: its id (the record's `wp`) and the repo the reviewed packet lives in. Both come
/// from the flag's row in `release/flags.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spec {
    pub id: String,
    pub packet_repo: String,
}

/// The sign-off `id` as `release/flags.json` declares it.
pub(crate) fn spec(id: &str) -> Result<Spec, String> {
    let table = crate::flags::load()?;
    table
        .flags
        .iter()
        .filter_map(|row| row.signoff.as_ref())
        .find(|s| s.id == id)
        .map(|s| Spec {
            id: s.id.clone(),
            packet_repo: s.packet_repo.clone(),
        })
        .ok_or_else(|| format!("release/flags.json names no sign-off {id}"))
}

/// The record of sign-off `id`, repo-relative.
pub(crate) fn record_path(id: &str) -> String {
    format!("release/signoffs/{id}.json")
}

/// The exact packet the signers reviewed, committed beside the record, repo-relative.
pub(crate) fn packet_path(id: &str) -> String {
    format!("release/signoffs/{id}_SIGNOFF_PACKET.md")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub wp: String,
    pub packet: Packet,
    pub signers: Vec<Signer>,
}

/// `release/signoffs/signers.json`: exactly one address per role.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    owner: String,
    #[serde(rename = "security-lead")]
    security_lead: String,
}

impl Registry {
    fn address(&self, role: &str) -> &str {
        if role == "owner" {
            &self.owner
        } else {
            &self.security_lead
        }
    }
}

/// The packet that was reviewed, pinned to a commit and a digest of the file at that commit.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Packet {
    pub repo: String,
    pub path: String,
    pub rev: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Signer {
    pub role: String,
    pub github: String,
    pub date: String,
    /// EIP-191 signature (65 bytes, hex) over [`signing_message`] by the role's registered key.
    pub sig: String,
}

/// What a signer signs: the sign-off id, the role, the person, the date and the pinned packet, one
/// field per line so no field can run into another.
pub(crate) fn signing_message(
    id: &str,
    role: &str,
    github: &str,
    date: &str,
    packet: &serde_json::Value,
) -> String {
    let f = |k: &str| packet[k].as_str().unwrap_or_default().to_string();
    format!(
        "citrate-core {id} sign-off v1\nrole: {role}\ngithub: {github}\ndate: {date}\nrepo: {}\npath: {}\nrev: {}\nsha256: {}",
        f("repo"),
        f("path"),
        f("rev"),
        f("sha256")
    )
}

/// Full check: [`validate`], then the committed packet copy hashes to `packet.sha256`, the
/// registry names a different address for each role, and each role's signature over
/// [`signing_message`] recovers to that role's registered address.
pub(crate) fn verify(spec: &Spec, text: &str, packet: &[u8], registry: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let r = validate(spec, text)?;
    if hex::encode(Sha256::digest(packet)) != r.packet.sha256.to_ascii_lowercase() {
        return Err("the committed packet does not match the signed sha256".into());
    }
    // A struct, not a map: serde refuses a repeated key ("duplicate field") and, with
    // deny_unknown_fields, any role other than the two (review P3 on #631).
    let reg: Registry =
        serde_json::from_str(registry).map_err(|e| format!("signers registry: {e}"))?;
    let canon = |a: &str| a.trim().trim_start_matches("0x").to_ascii_lowercase();
    let packet_json = serde_json::json!({
        "repo": r.packet.repo, "path": r.packet.path,
        "rev": r.packet.rev, "sha256": r.packet.sha256,
    });
    let mut seen: Vec<String> = Vec::new();
    for role in ROLES {
        let want = canon(reg.address(role));
        if seen.contains(&want) {
            return Err("the owner and the security lead must be different keys".into());
        }
        seen.push(want.clone());
        let s = r
            .signers
            .iter()
            .find(|s| s.role == role)
            .ok_or_else(|| format!("no {role} signature"))?;
        let msg = signing_message(&spec.id, role, &s.github, &s.date, &packet_json);
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
    let num = |r: std::ops::Range<usize>| s.get(r).and_then(|p| p.parse::<u32>().ok());
    matches!(
        (num(0..4), num(5..7), num(8..10)),
        (Some(y), Some(m), Some(d)) if y >= 2026 && (1..=12).contains(&m) && (1..=31).contains(&d)
    )
}

/// The record's shape: it is for `spec.id`, pins a packet in `spec.packet_repo` by commit and
/// digest, and carries exactly one well-formed signature per role, by different people.
pub(crate) fn validate(spec: &Spec, text: &str) -> Result<Record, String> {
    let r: Record =
        serde_json::from_str(text).map_err(|e| format!("not a sign-off record: {e}"))?;
    if r.wp != spec.id {
        return Err(format!("record is for {}, not {}", r.wp, spec.id));
    }
    if r.packet.repo != spec.packet_repo || r.packet.path.trim().is_empty() {
        return Err(format!("the packet must be a file in {}", spec.packet_repo));
    }
    if !hex_of_len(&r.packet.rev, 40) {
        return Err("packet.rev must be a full commit id".into());
    }
    if !hex_of_len(&r.packet.sha256, 64) {
        return Err("packet.sha256 must be the file's SHA-256".into());
    }
    let mut people: Vec<String> = Vec::new();
    if r.signers.len() != ROLES.len() {
        return Err(format!(
            "the record must have exactly one signature per role ({})",
            ROLES.join(", ")
        ));
    }
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

/// The gate a flag passes: `on` needs a record that [`verify`]s against the committed packet and
/// the signer registry; `off` needs nothing, but a record that has landed early must still
/// [`validate`]. `read` returns a repo-relative file's bytes, or `None` when it is absent.
pub(crate) fn gate(
    spec: &Spec,
    on: bool,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<(), String> {
    let record = read(&record_path(&spec.id));
    if !on {
        return match record {
            Some(r) => validate(spec, &String::from_utf8_lossy(&r)).map(|_| ()),
            None => Ok(()),
        };
    }
    let need = |path: String| read(&path).ok_or_else(|| format!("{path} is missing"));
    let record = record.ok_or_else(|| format!("{} is missing", record_path(&spec.id)))?;
    let packet = need(packet_path(&spec.id))?;
    let signers = need(SIGNERS.to_string())?;
    verify(
        spec,
        &String::from_utf8_lossy(&record),
        &packet,
        &String::from_utf8_lossy(&signers),
    )
}
