//! 0.5.4 — the signed minimum version (a forced update for clients below it).
//!
//! `latest.json` is not signed: the updater plugin verifies only each downloaded artifact. A
//! `minimumVersion` read from it as is could be raised by anyone who can change the feed (a CDN
//! tamper) and lock every member out of the app. So the minimum travels as a separate signed
//! document inside the feed, and this module is the only reader:
//!
//! ```json
//! "citrate": { "updatePolicy": { "policy": "<base64 of the policy bytes>",
//!                                "signature": "<base64 of the minisign .sig text>" } }
//! ```
//!
//! The policy bytes are JSON: `{"domain":"citrate-core-update-policy/v1","minimumVersion":"0.5.4",
//! "issuedAt":<unix s>,"reason":"..."}`. The signature is checked exactly as the updater plugin
//! checks an artifact (`tauri-plugin-updater` `verify_signature`: the same pinned key from
//! `tauri.conf.json` `plugins.updater.pubkey`, the same minisign format), so `tauri signer sign`
//! on the policy file makes it. The `domain` field inside the signed bytes keeps an artifact
//! signature from ever passing as a policy, and a policy from ever installing as an artifact.
//!
//! Key reuse (@rule8, needs security sign-off): the updater key already decides which binary every
//! member runs, so letting it also say "this version or later" adds no power to whoever holds it.
//!
//! Fail open on anything unverified: a missing, malformed, unsigned or wrongly signed policy, or a
//! minimum above the version the same feed offers, never gates the app (it would lock members out
//! with nothing to update to). Sticky once verified: the kept policy is on disk (and checked again
//! at each read), so no network at launch does not lift a gate already seen.
//!
//! Ordering is by the signed `issuedAt` alone (#678 review): a verified policy replaces the kept
//! one only when its `issuedAt` is strictly later, whatever its minimum. So a minimum signed by
//! mistake, or one whose release was pulled, is lowered or rescinded by signing a newer policy
//! with a lower `minimumVersion`, or with `"minimumVersion": null` (no minimum). A replayed older
//! policy, or a different one with the same `issuedAt`, is refused and the kept one stays. A policy
//! dated more than 24 h ahead of the client's clock is refused ([`MAX_ISSUED_AHEAD_S`]).

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The domain inside every policy document.
pub const POLICY_DOMAIN: &str = "citrate-core-update-policy/v1";
/// Where the verified policy is kept (app data dir).
pub const POLICY_FILE: &str = "update-policy.json";
/// Longest policy or signature text accepted (base64 characters).
const MAX_FIELD: usize = 8 * 1024;

/// The signed document.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyDoc {
    pub domain: String,
    /// `None` (`null` or absent): no minimum, which rescinds a kept one when issued later.
    pub minimum_version: Option<String>,
    pub issued_at: u64,
    #[serde(default)]
    pub reason: Option<String>,
}

/// What the app shows. `update_required` is true only from a verified policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyVerdict {
    pub current_version: String,
    pub minimum_version: Option<String>,
    pub update_required: bool,
    pub reason: Option<String>,
    /// Why a policy offered was not used (it does not gate the app), or null.
    pub refused: Option<String>,
}

/// The policy as kept on disk: the exact signed text, checked again at every read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StoredPolicy {
    pub policy: String,
    pub signature: String,
}

/// A version as `major.minor.patch` with an optional pre-release (`0.5.4-rc.1` < `0.5.4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    core: (u64, u64, u64),
    pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut it = core.split('.');
        let num = |p: Option<&str>| -> Option<u64> {
            let p = p?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || p.len() > 9 {
                return None;
            }
            p.parse().ok()
        };
        let v = (num(it.next())?, num(it.next())?, num(it.next())?);
        if it.next().is_some() {
            return None;
        }
        Some(Version { core: v, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match self.core.cmp(&other.core) {
            Equal => match (&self.pre, &other.pre) {
                (None, None) => Equal,
                (None, Some(_)) => Greater,
                (Some(_), None) => Less,
                (Some(a), Some(b)) => a.cmp(b),
            },
            o => o,
        }
    }
}

fn b64_text(s: &str, what: &str) -> Result<String, String> {
    if s.len() > MAX_FIELD {
        return Err(format!("the {what} is too long"));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|_| format!("the {what} is not base64"))?;
    String::from_utf8(bytes).map_err(|_| format!("the {what} is not text"))
}

/// Check `policy` (base64 of the signed bytes) against `signature` (base64 of the minisign
/// signature text) with `pubkey` (the `tauri.conf.json` updater pubkey), then the document.
pub fn verify(pubkey: &str, policy: &str, signature: &str) -> Result<PolicyDoc, String> {
    let key_text = b64_text(pubkey, "updater key")?;
    let key = minisign_verify::PublicKey::decode(&key_text)
        .map_err(|e| format!("the updater key does not parse ({e})"))?;
    if policy.len() > MAX_FIELD {
        return Err("the policy is too long".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(policy.trim())
        .map_err(|_| "the policy is not base64".to_string())?;
    let sig_text = b64_text(signature, "policy signature")?;
    let sig = minisign_verify::Signature::decode(&sig_text)
        .map_err(|e| format!("the policy signature does not parse ({e})"))?;
    // As the updater plugin checks an artifact (legacy and prehashed signatures both accepted).
    key.verify(&bytes, &sig, true)
        .map_err(|_| "the policy signature does not match the updater key".to_string())?;
    let doc: PolicyDoc =
        serde_json::from_slice(&bytes).map_err(|e| format!("the policy does not parse ({e})"))?;
    if doc.domain != POLICY_DOMAIN {
        return Err(format!(
            "the policy is for {:?}, not {POLICY_DOMAIN}",
            doc.domain
        ));
    }
    if let Some(m) = &doc.minimum_version {
        if Version::parse(m).is_none() {
            return Err(format!(
                "the policy's minimum version {m:?} is not a version"
            ));
        }
    }
    Ok(doc)
}

/// The verdict for `current` under a verified `doc` (or none).
pub fn verdict(current: &str, doc: Option<&PolicyDoc>, refused: Option<String>) -> PolicyVerdict {
    let required = match (doc, Version::parse(current)) {
        (Some(d), Some(cur)) => d
            .minimum_version
            .as_deref()
            .and_then(Version::parse)
            .is_some_and(|min| cur < min),
        _ => false,
    };
    PolicyVerdict {
        current_version: current.to_string(),
        minimum_version: doc.and_then(|d| d.minimum_version.clone()),
        update_required: required,
        reason: doc.and_then(|d| d.reason.clone()),
        refused,
    }
}

/// Read the kept policy and verify it again. A file that does not verify is ignored (and said).
pub fn load(path: &Path, pubkey: &str) -> (Option<(StoredPolicy, PolicyDoc)>, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None),
        Err(e) => {
            return (
                None,
                Some(format!("the kept update policy could not be read ({e})")),
            )
        }
    };
    let stored: StoredPolicy = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            return (
                None,
                Some(format!("the kept update policy does not parse ({e})")),
            )
        }
    };
    match verify(pubkey, &stored.policy, &stored.signature) {
        Ok(doc) => (Some((stored, doc)), None),
        Err(e) => (
            None,
            Some(format!("the kept update policy was ignored: {e}")),
        ),
    }
}

/// Whether `new` replaces `kept`: only a strictly later signed `issuedAt`, whatever the minimum
/// (so a later policy may lower or rescind it; an older or same-time one never replaces it).
fn supersedes(new: &PolicyDoc, kept: &PolicyDoc) -> bool {
    new.issued_at > kept.issued_at
}

/// Apply a policy offered by the feed whose own version is `feed_version`: verify it, keep it if
/// it was issued after the kept one (raising, lowering or rescinding it), and return the verdict from whichever is now kept. Never fails: a
/// refused policy is named in `refused` and gates nothing.
pub fn apply(
    path: &Path,
    pubkey: &str,
    current: &str,
    offered: &StoredPolicy,
    feed_version: &str,
) -> PolicyVerdict {
    apply_at(path, pubkey, current, offered, feed_version, now_unix())
}

/// How far ahead of this client's clock a policy's `issuedAt` may be (#678 review P3). A policy
/// dated further ahead would outrank every policy signed until then (ordering is by `issuedAt`),
/// so a mis-set signing clock could block the lower-or-rescind path for a long time.
pub const MAX_ISSUED_AHEAD_S: u64 = 24 * 60 * 60;

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// [`apply`] with this client's clock as `now` (unix seconds).
pub fn apply_at(
    path: &Path,
    pubkey: &str,
    current: &str,
    offered: &StoredPolicy,
    feed_version: &str,
    now: u64,
) -> PolicyVerdict {
    let (kept, load_note) = load(path, pubkey);
    let mut refused = load_note;
    let mut effective = kept.as_ref().map(|(_, d)| d.clone());
    match verify(pubkey, &offered.policy, &offered.signature) {
        Err(e) => refused = Some(format!("the feed's update policy was not used: {e}")),
        Ok(doc) => {
            let consistent = match doc.minimum_version.as_deref() {
                // No minimum: nothing to update to is needed.
                None => true,
                Some(m) => match (Version::parse(m), Version::parse(feed_version)) {
                    (Some(min), Some(feed)) => min <= feed,
                    _ => false,
                },
            };
            let same_as_kept = kept.as_ref().is_some_and(|(k, _)| k == offered);
            if doc.issued_at > now.saturating_add(MAX_ISSUED_AHEAD_S) {
                refused = Some(format!(
                    "the feed's update policy was not used: it is dated {} s ahead of this device's clock (at most {MAX_ISSUED_AHEAD_S} s is accepted)",
                    doc.issued_at - now
                ));
            } else if !consistent {
                refused = Some(format!(
                    "the feed's update policy was not used: it requires {} but the feed offers {feed_version}",
                    doc.minimum_version.as_deref().unwrap_or("none")
                ));
            } else if same_as_kept {
                // The feed still carries the kept policy: nothing to do.
            } else if effective.as_ref().is_some_and(|k| !supersedes(&doc, k)) {
                refused = Some(format!(
                    "the feed's update policy was not used: it was issued at {}, not after the kept policy ({})",
                    doc.issued_at,
                    effective.as_ref().map(|k| k.issued_at).unwrap_or_default()
                ));
            } else {
                match save(path, offered) {
                    Ok(()) => {}
                    Err(e) => refused = Some(format!("the update policy could not be kept ({e}); it applies to this session only")),
                }
                effective = Some(doc);
            }
        }
    }
    verdict(current, effective.as_ref(), refused)
}

/// The verdict from the kept policy alone (launch, offline).
pub fn status(path: &Path, pubkey: &str, current: &str) -> PolicyVerdict {
    let (kept, note) = load(path, pubkey);
    verdict(current, kept.as_ref().map(|(_, d)| d), note)
}

fn save(path: &Path, p: &StoredPolicy) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let body = serde_json::to_vec_pretty(p).map_err(std::io::Error::other)?;
    crate::atomic_file::replace(path, &body)
}

/// The updater pubkey this build pins (`plugins.updater.pubkey`).
fn pinned_pubkey<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<String, String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .map(str::to_string)
        .ok_or_else(|| "this build pins no updater key".to_string())
}

fn policy_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(POLICY_FILE))
}

/// `update_policy_apply` — the policy the feed carried (`latest.json` `citrate.updatePolicy`).
#[tauri::command]
pub async fn update_policy_apply(
    app_h: tauri::AppHandle,
    policy: String,
    signature: String,
    feed_version: String,
) -> Result<PolicyVerdict, String> {
    crate::blocking::off_main(move || {
        let key = pinned_pubkey(&app_h)?;
        let path = policy_path(&app_h)?;
        let current = app_h.package_info().version.to_string();
        Ok(apply(
            &path,
            &key,
            &current,
            &StoredPolicy { policy, signature },
            &feed_version,
        ))
    })
    .await
}

/// Longest feed read when the policy is fetched directly (bytes).
const MAX_FEED: u64 = 1024 * 1024;

/// The verdict for a whole feed document (`latest.json`), read directly rather than through the
/// updater plugin. Needed because the plugin hands back the feed only when it offers a version
/// newer than this one: after a release is pulled and the feed goes back to the version a gated
/// client already runs, the plugin reports "no update", and the signed policy that lowers or
/// rescinds the minimum would never be read. A feed with no policy leaves the kept one in force.
pub fn apply_feed(
    path: &Path,
    pubkey: &str,
    current: &str,
    feed: &serde_json::Value,
) -> PolicyVerdict {
    let version = feed.get("version").and_then(|v| v.as_str());
    let pol = feed.get("citrate").and_then(|c| c.get("updatePolicy"));
    let pair = pol.and_then(|p| Some((p.get("policy")?.as_str()?, p.get("signature")?.as_str()?)));
    match (version, pair) {
        (Some(v), Some((policy, signature))) => apply(
            path,
            pubkey,
            current,
            &StoredPolicy {
                policy: policy.to_string(),
                signature: signature.to_string(),
            },
            v,
        ),
        _ => status(path, pubkey, current),
    }
}

/// The updater endpoints this build pins (`plugins.updater.endpoints`), in order.
fn pinned_endpoints<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Vec<String> {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("endpoints"))
        .and_then(|e| e.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The first endpoint that answers 2xx with JSON (the plugin's order), or why none did.
fn fetch_feed(endpoints: &[String]) -> Result<serde_json::Value, String> {
    let agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(5)
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .new_agent();
    let mut last = "this build pins no updater endpoint".to_string();
    for url in endpoints {
        let mut resp = match agent.get(url).call() {
            Ok(r) => r,
            Err(e) => {
                last = format!("{url}: {e}");
                continue;
            }
        };
        if !resp.status().is_success() {
            last = format!("{url}: HTTP {}", resp.status().as_u16());
            continue;
        }
        match resp
            .body_mut()
            .with_config()
            .limit(MAX_FEED)
            .read_to_string()
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(v) => return Ok(v),
            Err(e) => last = format!("{url}: {e}"),
        }
    }
    Err(last)
}

/// `update_policy_refresh` — read the feed directly and apply its policy (used while gated and the
/// plugin found no update). A feed that cannot be read leaves the kept policy in force.
#[tauri::command]
pub async fn update_policy_refresh(app_h: tauri::AppHandle) -> Result<PolicyVerdict, String> {
    crate::blocking::off_main(move || {
        let key = pinned_pubkey(&app_h)?;
        let path = policy_path(&app_h)?;
        let current = app_h.package_info().version.to_string();
        match fetch_feed(&pinned_endpoints(&app_h)) {
            Ok(feed) => Ok(apply_feed(&path, &key, &current, &feed)),
            Err(e) => {
                let mut v = status(&path, &key, &current);
                v.refused = Some(format!("the update feed could not be read ({e})"));
                Ok(v)
            }
        }
    })
    .await
}

/// `update_policy_status` — the verdict from the kept policy (no network).
#[tauri::command]
pub async fn update_policy_status(app_h: tauri::AppHandle) -> Result<PolicyVerdict, String> {
    crate::blocking::off_main(move || {
        let key = pinned_pubkey(&app_h)?;
        let path = policy_path(&app_h)?;
        let current = app_h.package_info().version.to_string();
        Ok(status(&path, &key, &current))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("update_policy_tests.rs");
}
