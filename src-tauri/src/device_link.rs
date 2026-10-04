//! HUP-S8.1: this machine's own device key, and the DeviceLink that ties it to the member.
//!
//! ## Why
//!
//! The cluster mesh identifies a peer by its secp256k1 key. Until now every machine meshed as the
//! member's comms identity, which CONNECT-S5 derives from the wallet, so all of one member's
//! machines shared one key and one libp2p PeerId: the mesh could not tell them apart, and could not
//! drop one without dropping all. Planset decision D-31 (owner amendment 2026-09-30): a random
//! device key per machine plus a wallet-signed DeviceLink.
//!
//! ## What lives here
//!
//! * **The device key.** A random secp256k1 key minted on first use and sealed in the OS keyring
//!   (account [`DEVICE_KEY_ACCOUNT`]), like the comms key was before CONNECT-S5. It never leaves
//!   this machine except into the cluster daemon's 0600 seed file, where it becomes the Noise /
//!   libp2p identity. It is not derived from the wallet and carries no value.
//! * **The DeviceLink.** `{member, device, wallet, index, label, issued_at}` signed three times over
//!   one human-readable EIP-191 message (the same bytes `cluster-core::device` builds; both repos pin
//!   the golden vector):
//!   - the **wallet** signature goes through the [`SignatureCeremony`] like every other use of the
//!     custody key: the person sees the exact text and approves it by id (Rule 3, HIC). Nothing in
//!     this module can produce it on its own;
//!   - the **member** signature is the comms key (the roster identity the mesh checks), and the
//!     **device** signature is this machine's device key (proof of possession). Neither is the
//!     wallet; both are non-value scoped keys this process already holds, like the comms key the
//!     member daemon signs MLS traffic with. They are produced only inside the approve step, after
//!     the person approved the wallet signature.
//! * **Revocation.** The member's comms key signs a revocation; the cluster daemon honours it for
//!   good (the device key is never admitted again). Revoking THIS machine also deletes its device
//!   key, so linking it again mints a fresh one.
//! * **The local store** (`<app data>/cluster/device-links.json`): the links and revocations this
//!   machine knows, sent to the cluster daemon with every roster update. Signatures are public
//!   attestations, not secrets; the file is still written owner-only.
//!
//! ## What does not change by default
//!
//! The mesh only switches its identity to the device key when the operator has turned the
//! cross-machine transport on (`CITRATE_CLUSTER_LISTEN`, soak-gated) AND this machine has an active
//! link. Otherwise the cluster keeps meshing as the comms identity exactly as before.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::ceremony::{CeremonyView, IntentKind, SignatureCeremony, SignatureIntent};
use crate::custody::{CustodyVault, Keyring};

/// OS keyring account holding this machine's random device key (32 raw bytes).
pub(crate) const DEVICE_KEY_ACCOUNT: &str = "cluster-device-key-v1";

/// DeviceLink message version. Must equal `cluster_core::device::DEVICE_LINK_VERSION`.
pub(crate) const DEVICE_LINK_VERSION: u32 = 1;

/// Longest device label (matches `cluster_core::device::MAX_LABEL_LEN`).
pub(crate) const MAX_LABEL_LEN: usize = 48;

/// Highest device index (matches `cluster_core::device::MAX_DEVICE_INDEX`).
pub(crate) const MAX_DEVICE_INDEX: u32 = 1023;

/// Most links the daemon accepts per roster update (matches `MAX_LINKS_PER_UPDATE`).
pub(crate) const MAX_LINKS: usize = 64;

/// The ceremony origin shown to the person: they started this from the Cluster screen.
const ORIGIN: &str = "local-user";

const SEED_LEN: usize = 32;

/// Canonical address: lowercase hex, no `0x`, exactly 40 hex chars.
pub(crate) fn canonical_address(raw: &str) -> Option<String> {
    let h = raw.trim().trim_start_matches("0x").to_ascii_lowercase();
    (h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())).then_some(h)
}

/// The label alphabet `cluster-core` accepts: 1..=48 chars of ASCII letters, digits, space, `.`,
/// `_`, `-`, `'`, no leading/trailing space. Keeps the signed text unambiguous.
pub(crate) fn label_is_valid(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_LEN
        && label.trim() == label
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'.' | b'_' | b'-' | b'\''))
}

/// The unsigned DeviceLink body (canonical addresses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceLinkBody {
    pub member: String,
    pub device: String,
    pub wallet: String,
    pub index: u32,
    pub label: String,
    pub issued_at: u64,
}

impl DeviceLinkBody {
    /// Validate + canonicalize. Mirrors `cluster_core::device::DeviceLink::new`.
    pub fn new(
        member: &str,
        device: &str,
        wallet: &str,
        index: u32,
        label: &str,
        issued_at: u64,
    ) -> Result<Self, String> {
        let member = canonical_address(member).ok_or("member address is malformed")?;
        let device = canonical_address(device).ok_or("device address is malformed")?;
        let wallet = canonical_address(wallet).ok_or("wallet address is malformed")?;
        if device == member || device == wallet {
            return Err("the device key must be its own key".into());
        }
        if index > MAX_DEVICE_INDEX {
            return Err("too many devices for this member".into());
        }
        if !label_is_valid(label) {
            return Err(format!(
                "device name must be 1 to {MAX_LABEL_LEN} letters, digits, spaces or . _ - '"
            ));
        }
        Ok(DeviceLinkBody {
            member,
            device,
            wallet,
            index,
            label: label.to_string(),
            issued_at,
        })
    }

    /// The exact text all three keys sign. Byte-identical to `cluster-core`'s
    /// `DeviceLink::signing_message` (golden vector in the tests of both repos).
    pub fn signing_message(&self) -> String {
        format!(
            "Citrate DeviceLink v{DEVICE_LINK_VERSION}\n\
             Link this device to my Citrate member identity.\n\
             member: 0x{}\n\
             device: 0x{}\n\
             wallet: 0x{}\n\
             index: {}\n\
             label: {}\n\
             issued_at: {}",
            self.member, self.device, self.wallet, self.index, self.label, self.issued_at
        )
    }
}

/// The text a member signs to revoke a device. Byte-identical to `cluster-core`'s
/// `DeviceRevocation::signing_message`.
pub(crate) fn revocation_message(member: &str, device: &str, revoked_at: u64) -> String {
    format!(
        "Citrate DeviceRevocation v{DEVICE_LINK_VERSION}\n\
         Remove this device from my Citrate member identity.\n\
         member: 0x{member}\n\
         device: 0x{device}\n\
         revoked_at: {revoked_at}"
    )
}

/// A signed link as stored locally and sent to the cluster daemon (the daemon's `DeviceLinkWire`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceLinkWire {
    pub member: String,
    pub device: String,
    pub wallet: String,
    pub index: u32,
    pub label: String,
    pub issued_at: u64,
    pub member_sig: String,
    pub device_sig: String,
    pub wallet_sig: String,
}

impl DeviceLinkWire {
    fn body(&self) -> Result<DeviceLinkBody, String> {
        DeviceLinkBody::new(
            &self.member,
            &self.device,
            &self.wallet,
            self.index,
            &self.label,
            self.issued_at,
        )
    }
}

/// A member-signed revocation (the daemon's `RevocationWire`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevocationWire {
    pub member: String,
    pub device: String,
    pub revoked_at: u64,
    pub member_sig: String,
}

// ---------------------------------------------------------------------------
// EIP-191 with the non-value scoped keys (member/comms key, device key)
// ---------------------------------------------------------------------------

fn signing_key(seed_hex: &str) -> Result<k256::ecdsa::SigningKey, String> {
    let bytes = Zeroizing::new(hex::decode(seed_hex.trim()).map_err(|_| "key is not hex")?);
    k256::ecdsa::SigningKey::from_slice(&bytes)
        .map_err(|_| "key is not a valid secp256k1 scalar".into())
}

/// EIP-191 `personal_sign` with a scoped key: 65 bytes `r||s||v`, `v` in {27,28}, low-s.
pub(crate) fn sign_eip191(seed_hex: &str, message: &str) -> Result<String, String> {
    let key = signing_key(seed_hex)?;
    let digest = crate::wallet::eip191_prehash(message.as_bytes());
    let (sig, recid) = key
        .sign_prehash_recoverable(&digest)
        .map_err(|_| "signing failed".to_string())?;
    let mut out = sig.to_bytes().to_vec();
    out.push(27 + recid.to_byte());
    Ok(format!("0x{}", hex::encode(out)))
}

/// Recover the canonical signer of an EIP-191 signature (low-s only), or `None`.
pub(crate) fn recover_eip191(message: &str, sig_hex: &str) -> Option<String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::{Digest, Keccak256};
    let raw = hex::decode(sig_hex.trim().trim_start_matches("0x")).ok()?;
    if raw.len() != 65 {
        return None;
    }
    let sig = Signature::from_slice(&raw[..64]).ok()?;
    if sig.normalize_s().is_some() {
        return None;
    }
    let v = match raw[64] {
        0 | 27 => 0u8,
        1 | 28 => 1u8,
        _ => return None,
    };
    let key = VerifyingKey::recover_from_prehash(
        &crate::wallet::eip191_prehash(message.as_bytes()),
        &sig,
        RecoveryId::from_byte(v)?,
    )
    .ok()?;
    let point = key.to_encoded_point(false);
    Some(hex::encode(
        &Keccak256::digest(&point.as_bytes()[1..])[12..],
    ))
}

/// Check a stored link the way the daemon will: all three signatures recover to their keys.
pub(crate) fn verify_link(w: &DeviceLinkWire) -> Result<(), String> {
    let body = w.body()?;
    let msg = body.signing_message();
    for (which, sig, want) in [
        ("member", &w.member_sig, &body.member),
        ("device", &w.device_sig, &body.device),
        ("wallet", &w.wallet_sig, &body.wallet),
    ] {
        if recover_eip191(&msg, sig).as_deref() != Some(want.as_str()) {
            return Err(format!("the {which} signature does not verify"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The device key (random, sealed in the OS keyring)
// ---------------------------------------------------------------------------

/// Read this machine's device key (hex) without minting one. `Ok(None)` when none exists yet.
/// A stored value of the wrong shape is a hard fault (fail closed, never silently replaced).
pub(crate) fn read_device_seed(keyring: &dyn Keyring) -> Result<Option<Zeroizing<String>>, String> {
    let Some(mut bytes) = keyring
        .get(DEVICE_KEY_ACCOUNT)
        .map_err(|e| format!("device key keyring error: {e}"))?
    else {
        return Ok(None);
    };
    let valid = bytes.len() == SEED_LEN && k256::ecdsa::SigningKey::from_slice(&bytes).is_ok();
    let hex = Zeroizing::new(hex::encode(&bytes));
    use zeroize::Zeroize;
    bytes.zeroize();
    if !valid {
        return Err("the stored device key is not a valid secp256k1 key".into());
    }
    Ok(Some(hex))
}

/// Read this machine's device key, minting and sealing a fresh random one on first use. `mint` is
/// injected so tests stay deterministic; production passes [`mint_device_seed`].
pub(crate) fn load_or_mint_device_seed(
    keyring: &dyn Keyring,
    mint: impl FnOnce() -> Zeroizing<[u8; SEED_LEN]>,
) -> Result<Zeroizing<String>, String> {
    if let Some(seed) = read_device_seed(keyring)? {
        return Ok(seed);
    }
    let raw = mint();
    k256::ecdsa::SigningKey::from_slice(raw.as_ref())
        .map_err(|_| "minted device key is not a valid secp256k1 scalar".to_string())?;
    keyring
        .set(DEVICE_KEY_ACCOUNT, raw.as_ref())
        .map_err(|e| format!("device key keyring error: {e}"))?;
    Ok(Zeroizing::new(hex::encode(raw.as_ref())))
}

/// A fresh random secp256k1 secret from the OS RNG (retries the ~2^-128 invalid-scalar case).
pub(crate) fn mint_device_seed() -> Zeroizing<[u8; SEED_LEN]> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut b = Zeroizing::new([0u8; SEED_LEN]);
    loop {
        OsRng.fill_bytes(b.as_mut());
        if k256::ecdsa::SigningKey::from_slice(b.as_ref()).is_ok() {
            return b;
        }
    }
}

// ---------------------------------------------------------------------------
// The local store of links + revocations
// ---------------------------------------------------------------------------

/// The links and revocations this machine knows. Sent to the cluster daemon on every roster update.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceLinkStore {
    #[serde(default)]
    pub links: Vec<DeviceLinkWire>,
    #[serde(default)]
    pub revocations: Vec<RevocationWire>,
}

impl DeviceLinkStore {
    /// Load from `path`; a missing file is an empty store. A corrupt file is an error (never
    /// silently treated as empty, which would drop revocations).
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "the device link store is corrupt".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("reading the device link store: {}", e.kind())),
        }
    }

    /// Persist owner-only, atomically (temp file + rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);
        citrate_core_kit::fsutil::write_secret_file(&tmp, &json)
            .map_err(|e| format!("writing the device link store: {}", e.kind()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("saving the device link store: {}", e.kind()))
    }

    fn is_revoked(&self, member: &str, device: &str) -> bool {
        self.revocations
            .iter()
            .any(|r| r.member == member && r.device == device)
    }

    /// Add or replace the link for its device. Refuses a revoked device key.
    pub fn upsert_link(&mut self, link: DeviceLinkWire) -> Result<(), String> {
        if self.is_revoked(&link.member, &link.device) {
            return Err("this device key was revoked; it cannot be linked again".into());
        }
        self.links.retain(|l| l.device != link.device);
        if self.links.len() >= MAX_LINKS {
            return Err(format!("a member can link at most {MAX_LINKS} devices"));
        }
        self.links.push(link);
        self.links
            .sort_by(|a, b| (a.index, &a.device).cmp(&(b.index, &b.device)));
        Ok(())
    }

    /// Record a revocation and drop the device's link.
    pub fn revoke(&mut self, rev: RevocationWire) {
        self.links
            .retain(|l| !(l.device == rev.device && l.member == rev.member));
        if !self.is_revoked(&rev.member, &rev.device) {
            self.revocations.push(rev);
        }
    }

    /// The display ordinal for `device` under `member`: its existing index if linked, else the
    /// next unused one. Informational only (the device key is the identity).
    pub fn index_for(&self, member: &str, device: &str) -> u32 {
        if let Some(l) = self
            .links
            .iter()
            .find(|l| l.member == member && l.device == device)
        {
            return l.index;
        }
        self.links
            .iter()
            .filter(|l| l.member == member)
            .map(|l| l.index.saturating_add(1))
            .max()
            .unwrap_or(0)
            .min(MAX_DEVICE_INDEX)
    }

    /// The active link of `device` to `member`, if any.
    pub fn active_link(&self, member: &str, device: &str) -> Option<&DeviceLinkWire> {
        self.links
            .iter()
            .find(|l| l.member == member && l.device == device)
            .filter(|_| !self.is_revoked(member, device))
    }
}

/// `<app data>/cluster/device-links.json`.
pub(crate) fn store_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cluster")
        .join("device-links.json"))
}

// ---------------------------------------------------------------------------
// The mesh identity choice
// ---------------------------------------------------------------------------

/// Which key the cluster daemon meshes as. The device key only when the cross-machine transport is
/// on AND this machine holds an active link to the member; otherwise the comms identity, unchanged.
pub(crate) fn choose_mesh_identity(
    comms: crate::comms::DeviceIdentity,
    device: Option<crate::comms::DeviceIdentity>,
    store: &DeviceLinkStore,
    libp2p: bool,
) -> crate::comms::DeviceIdentity {
    match device {
        Some(d) if libp2p && store.active_link(&comms.address, &d.address).is_some() => d,
        _ => comms,
    }
}

/// The identity the cluster daemon should mesh as on this machine (see [`choose_mesh_identity`]).
/// Reads the keyring and the store, so call it off the main thread (cluster commands are async).
pub(crate) fn mesh_identity(
    app: &tauri::AppHandle,
    libp2p: bool,
) -> Result<crate::comms::DeviceIdentity, String> {
    let comms = crate::comms::device_identity(app)?;
    if !libp2p {
        return Ok(comms);
    }
    let device = match this_device_seed(false)? {
        Some(seed_hex) => {
            let address = crate::comms::address_from_secret_hex(&seed_hex)?;
            Some(crate::comms::DeviceIdentity { seed_hex, address })
        }
        None => None,
    };
    let store = DeviceLinkStore::load(&store_path(app)?)?;
    Ok(choose_mesh_identity(comms, device, &store, libp2p))
}

// ---------------------------------------------------------------------------
// The ceremony-gated link flow
// ---------------------------------------------------------------------------

/// What the UI sees of a link. No signatures cross the bridge (they are not secret, but the UI has
/// no use for them and they do not belong in logs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceLinkView {
    pub device: String,
    pub member: String,
    pub wallet: String,
    pub index: u32,
    pub label: String,
    pub issued_at: u64,
    pub this_device: bool,
}

/// This machine's device identity + the links and revocations it knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceLinksDto {
    /// This machine's device address, or `None` before its key is minted (first link).
    pub this_device: Option<String>,
    pub links: Vec<DeviceLinkView>,
    /// Revoked device addresses (permanent).
    pub revoked: Vec<String>,
}

pub(crate) fn links_dto(store: &DeviceLinkStore, this_device: Option<String>) -> DeviceLinksDto {
    DeviceLinksDto {
        links: store
            .links
            .iter()
            .map(|l| DeviceLinkView {
                device: l.device.clone(),
                member: l.member.clone(),
                wallet: l.wallet.clone(),
                index: l.index,
                label: l.label.clone(),
                issued_at: l.issued_at,
                this_device: this_device.as_deref() == Some(l.device.as_str()),
            })
            .collect(),
        revoked: store.revocations.iter().map(|r| r.device.clone()).collect(),
        this_device,
    }
}

/// Links awaiting the person's approval, keyed by ceremony id.
#[derive(Default)]
pub(crate) struct DeviceLinkFlow {
    pending: Mutex<HashMap<String, DeviceLinkBody>>,
}

impl DeviceLinkFlow {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, DeviceLinkBody>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Step 1: open a `personal_sign` ceremony over the link text. Signs nothing.
    pub fn open(
        &self,
        ceremony: &SignatureCeremony,
        chain_id: u64,
        body: DeviceLinkBody,
    ) -> CeremonyView {
        let view = ceremony.request(SignatureIntent {
            origin: ORIGIN.to_string(),
            kind: IntentKind::PersonalSign,
            chain_id,
            raw: hex::encode(body.signing_message().as_bytes()),
        });
        self.lock().insert(view.id.clone(), body);
        view
    }

    /// Step 2: the person approved. Checks the scoped keys still match the body BEFORE consuming
    /// the ceremony, then takes the wallet signature from the ceremony, adds the member and device
    /// signatures, and verifies all three before returning the link.
    pub fn approve(
        &self,
        vault: &CustodyVault,
        ceremony: &SignatureCeremony,
        id: &str,
        raw_ack: bool,
        member_seed_hex: &str,
        device_seed_hex: &str,
    ) -> Result<DeviceLinkWire, String> {
        let body = self
            .lock()
            .get(id)
            .cloned()
            .ok_or("no pending device link for that approval")?;
        if crate::comms::address_from_secret_hex(member_seed_hex)? != body.member {
            return Err("the member identity changed since this link was requested".into());
        }
        if crate::comms::address_from_secret_hex(device_seed_hex)? != body.device {
            return Err("this device's key changed since this link was requested".into());
        }
        let sig = ceremony
            .approve(vault, id, raw_ack)
            .map_err(|e| e.to_string())?;
        self.lock().remove(id);
        let msg = body.signing_message();
        let link = DeviceLinkWire {
            member_sig: sign_eip191(member_seed_hex, &msg)?,
            device_sig: sign_eip191(device_seed_hex, &msg)?,
            wallet_sig: format!("0x{}", sig.sig_hex.trim_start_matches("0x")),
            member: body.member,
            device: body.device,
            wallet: body.wallet,
            index: body.index,
            label: body.label,
            issued_at: body.issued_at,
        };
        verify_link(&link)?;
        Ok(link)
    }

    /// Drop a pending link the person declined.
    pub fn forget(&self, id: &str) {
        self.lock().remove(id);
    }

    #[cfg(test)]
    pub fn pending_count(&self) -> usize {
        self.lock().len()
    }
}

/// Build a member-signed revocation of `device`.
pub(crate) fn sign_revocation(
    member_seed_hex: &str,
    device: &str,
    revoked_at: u64,
) -> Result<RevocationWire, String> {
    let member = crate::comms::address_from_secret_hex(member_seed_hex)?;
    let device = canonical_address(device).ok_or("device address is malformed")?;
    let msg = revocation_message(&member, &device, revoked_at);
    Ok(RevocationWire {
        member_sig: sign_eip191(member_seed_hex, &msg)?,
        member,
        device,
        revoked_at,
    })
}

/// Longest link code accepted on import (a link is ~600 bytes of JSON).
const MAX_CODE_LEN: usize = 4096;

/// Add another of the member's OWN devices from its exported link code. The code is parsed,
/// canonicalized, required to name `own_member`, and verified on all three signatures before it is
/// stored. Links of other members' devices arrive through the roster in a follow-on, not here.
pub(crate) fn import_link(
    store: &mut DeviceLinkStore,
    own_member: &str,
    code: &str,
) -> Result<DeviceLinkWire, String> {
    if code.len() > MAX_CODE_LEN {
        return Err("that link code is too long".into());
    }
    let w: DeviceLinkWire = serde_json::from_str(code.trim())
        .map_err(|_| "that is not a device link code".to_string())?;
    let body = w.body()?;
    if Some(body.member.as_str()) != canonical_address(own_member).as_deref() {
        return Err("that code links a device to a different member".into());
    }
    let canon = DeviceLinkWire {
        member: body.member,
        device: body.device,
        wallet: body.wallet,
        index: body.index,
        label: body.label,
        issued_at: body.issued_at,
        member_sig: w.member_sig,
        device_sig: w.device_sig,
        wallet_sig: w.wallet_sig,
    };
    verify_link(&canon)?;
    store.upsert_link(canon.clone())?;
    Ok(canon)
}

/// What happened to a link code that arrived with a fleet pairing (S8.2): the other machine's link,
/// if it is one of this member's own devices, is verified and stored like a pasted code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PairedLink {
    /// No code came with the pairing (the other machine is not linked yet).
    None,
    /// Verified and stored: the other machine is one of this member's devices.
    Added,
    /// The code names a different member (another person's machine): not stored.
    OtherMember,
    /// The code did not verify or could not be stored.
    Refused,
}

/// Import a link code received while pairing. Never errors: the pairing itself stands; the result
/// says whether the link was added.
pub(crate) fn import_paired_code(
    store: &mut DeviceLinkStore,
    own_member: &str,
    code: Option<&str>,
) -> PairedLink {
    let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) else {
        return PairedLink::None;
    };
    let names_other_member = serde_json::from_str::<DeviceLinkWire>(code)
        .ok()
        .and_then(|w| canonical_address(&w.member))
        .is_some_and(|m| Some(m) != canonical_address(own_member));
    if names_other_member {
        return PairedLink::OtherMember;
    }
    match import_link(store, own_member, code) {
        Ok(_) => PairedLink::Added,
        Err(_) => PairedLink::Refused,
    }
}

/// This machine's active link code, if it has one (the same text `device_link_export` returns).
/// Read-only: never mints a key. Blocking (keyring + file): call off the main thread.
pub(crate) fn own_link_code(app: &tauri::AppHandle) -> Option<String> {
    let member = crate::comms::device_identity(app).ok()?;
    let seed = this_device_seed(false).ok()??;
    let device = crate::comms::address_from_secret_hex(&seed).ok()?;
    let store = DeviceLinkStore::load(&store_path(app).ok()?).ok()?;
    let link = store.active_link(&member.address, &device)?;
    serde_json::to_string(link).ok()
}

/// Store a link code received while pairing (see [`import_paired_code`]). Blocking.
pub(crate) fn store_paired_code(app: &tauri::AppHandle, code: Option<&str>) -> PairedLink {
    let Ok(member) = crate::comms::device_identity(app) else {
        return PairedLink::Refused;
    };
    let Ok(path) = store_path(app) else {
        return PairedLink::Refused;
    };
    let Ok(mut store) = DeviceLinkStore::load(&path) else {
        return PairedLink::Refused;
    };
    let r = import_paired_code(&mut store, &member.address, code);
    if r == PairedLink::Added && store.save(&path).is_err() {
        return PairedLink::Refused;
    }
    r
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tauri commands (all async: keyring + file I/O run on the blocking pool)
// ---------------------------------------------------------------------------

/// Managed state for the pending-link table.
pub struct DeviceLinkState(pub(crate) DeviceLinkFlow);

pub fn build_device_link_state() -> DeviceLinkState {
    DeviceLinkState(DeviceLinkFlow::default())
}

fn this_device_seed(create: bool) -> Result<Option<Zeroizing<String>>, String> {
    let keyring = crate::custody::OsKeyring::legacy();
    if create {
        load_or_mint_device_seed(&keyring, mint_device_seed).map(Some)
    } else {
        read_device_seed(&keyring)
    }
}

/// **device_link_request**: open the ceremony that links THIS machine to the member. Mints this
/// machine's device key on first use. Returns the ceremony view for the approval card; signs
/// nothing.
#[tauri::command]
pub async fn device_link_request(
    app: tauri::AppHandle,
    label: String,
) -> Result<CeremonyView, String> {
    crate::blocking::off_main(move || device_link_request_sync(&app, label)).await
}

fn device_link_request_sync(app: &tauri::AppHandle, label: String) -> Result<CeremonyView, String> {
    use tauri::Manager;
    let label = label.trim().to_string();
    if !label_is_valid(&label) {
        return Err(format!(
            "device name must be 1 to {MAX_LABEL_LEN} letters, digits, spaces or . _ - '"
        ));
    }
    let custody = app
        .try_state::<crate::custody::CustodyState>()
        .ok_or("internal: managed state unavailable")?;
    let ceremony = app
        .try_state::<crate::ceremony::CeremonyState>()
        .ok_or("internal: managed state unavailable")?;
    let flow = app
        .try_state::<DeviceLinkState>()
        .ok_or("internal: managed state unavailable")?;
    let wallet = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let member = crate::comms::device_identity(app)?;
    let seed = this_device_seed(true)?.ok_or("device key unavailable")?;
    let device = crate::comms::address_from_secret_hex(&seed)?;
    let store = DeviceLinkStore::load(&store_path(app)?)?;
    let body = DeviceLinkBody::new(
        &member.address,
        &device,
        &wallet.address,
        store.index_for(&member.address, &device),
        &label,
        now_secs(),
    )?;
    if store.is_revoked(&body.member, &body.device) {
        return Err("this device key was revoked; remove it to mint a new one first".into());
    }
    Ok(flow.0.open(&ceremony.0, crate::rpc::CITRATE_CHAIN_ID, body))
}

/// **device_link_approve**: the person approved the wallet signature. Completes the three-key
/// link, verifies it, and stores it. Returns the updated device list (no signatures).
#[tauri::command]
pub async fn device_link_approve(
    app: tauri::AppHandle,
    id: String,
    raw_ack: bool,
) -> Result<DeviceLinksDto, String> {
    crate::blocking::off_main(move || device_link_approve_sync(&app, id, raw_ack)).await
}

fn device_link_approve_sync(
    app: &tauri::AppHandle,
    id: String,
    raw_ack: bool,
) -> Result<DeviceLinksDto, String> {
    use tauri::Manager;
    let custody = app
        .try_state::<crate::custody::CustodyState>()
        .ok_or("internal: managed state unavailable")?;
    let ceremony = app
        .try_state::<crate::ceremony::CeremonyState>()
        .ok_or("internal: managed state unavailable")?;
    let flow = app
        .try_state::<DeviceLinkState>()
        .ok_or("internal: managed state unavailable")?;
    let member = crate::comms::device_identity(app)?;
    let seed = this_device_seed(false)?.ok_or("this device has no device key")?;
    let link = flow.0.approve(
        &custody.0,
        &ceremony.0,
        &id,
        raw_ack,
        &member.seed_hex,
        &seed,
    )?;
    let path = store_path(app)?;
    let mut store = DeviceLinkStore::load(&path)?;
    let this = link.device.clone();
    store.upsert_link(link)?;
    store.save(&path)?;
    // The mesh identity may now be this device's key: restart the daemon on the next cluster call
    // instead of waiting for an app restart. Best effort; the link is stored either way.
    let _ = crate::cluster::reload_mesh_identity(app);
    Ok(links_dto(&store, Some(this)))
}

/// **device_link_reject**: the person declined; nothing was signed.
#[tauri::command]
pub async fn device_link_reject(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        if let Some(c) = app.try_state::<crate::ceremony::CeremonyState>() {
            let _ = c.0.reject(&id);
        }
        if let Some(f) = app.try_state::<DeviceLinkState>() {
            f.0.forget(&id);
        }
        Ok(())
    })
    .await
}

/// **device_links**: this machine's device address and the links/revocations it knows. Never mints
/// a key.
#[tauri::command]
pub async fn device_links(app: tauri::AppHandle) -> Result<DeviceLinksDto, String> {
    crate::blocking::off_main(move || {
        let store = DeviceLinkStore::load(&store_path(&app)?)?;
        let this = match this_device_seed(false)? {
            Some(seed) => Some(crate::comms::address_from_secret_hex(&seed)?),
            None => None,
        };
        Ok(links_dto(&store, this))
    })
    .await
}

/// How long a prepared device revocation may be confirmed.
pub const REVOKE_CONFIRM_SECS: u64 = 120;

/// What the member confirms before a device is revoked.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePrepared {
    pub confirm_id: String,
    /// Canonical device address (lowercase hex, no `0x`).
    pub device: String,
    pub statement: String,
    pub confirm_by: u64,
}

/// One pending device revocation at a time: a one-shot id core minted for one device. A revocation
/// is permanent, so `device_link_revoke` runs only with the id `device_link_revoke_prepare` returned
/// when the member opened the confirmation (a caller cannot revoke by naming a device).
#[derive(Default)]
pub struct RevokeConfirmations(Mutex<Option<(String, String, u64)>>);

impl RevokeConfirmations {
    pub fn prepare(&self, device: &str, now: u64, id: String) -> Result<RevokePrepared, String> {
        let device = canonical_address(device).ok_or("device address is malformed")?;
        let confirm_by = now.saturating_add(REVOKE_CONFIRM_SECS);
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((id.clone(), device.clone(), confirm_by));
        Ok(RevokePrepared {
            statement: format!(
                "Remove device 0x{device} for good. Its key can never be linked again, and the group mesh drops it on the next roster update. This cannot be undone."
            ),
            confirm_id: id,
            device,
            confirm_by,
        })
    }

    /// The device for `id`, consumed. A wrong id leaves the pending one in place.
    pub fn consume(&self, id: &str, now: u64) -> Result<String, String> {
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match g.as_ref() {
            Some((pid, _, _)) if pid == id => {}
            _ => return Err("this confirmation is not the current one; start again".into()),
        }
        let (_, device, confirm_by) = g.take().ok_or("no confirmation is pending")?;
        if now >= confirm_by {
            return Err("the confirmation timed out; start again".into());
        }
        Ok(device)
    }
}

fn revoke_confirmations() -> &'static RevokeConfirmations {
    static C: std::sync::OnceLock<RevokeConfirmations> = std::sync::OnceLock::new();
    C.get_or_init(RevokeConfirmations::default)
}

/// **device_link_revoke_prepare**: step 1 of removing a device. Returns what the member confirms
/// and a one-shot id valid for [`REVOKE_CONFIRM_SECS`]. Signs nothing.
#[tauri::command]
pub async fn device_link_revoke_prepare(device: String) -> Result<RevokePrepared, String> {
    crate::blocking::off_main(move || {
        use rand::RngCore;
        let mut b = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut b);
        revoke_confirmations().prepare(&device, now_secs(), hex::encode(b))
    })
    .await
}

/// **device_link_revoke**: the member confirmed `confirm_id`; their comms key signs a revocation of
/// that device. Permanent for that key; the cluster daemon evicts it on the next roster update.
/// Revoking THIS machine also deletes its device key, so a later link mints a fresh one.
#[tauri::command]
pub async fn device_link_revoke(
    app: tauri::AppHandle,
    confirm_id: String,
) -> Result<DeviceLinksDto, String> {
    crate::blocking::off_main(move || {
        let device = revoke_confirmations().consume(&confirm_id, now_secs())?;
        let member = crate::comms::device_identity(&app)?;
        let rev = sign_revocation(&member.seed_hex, &device, now_secs())?;
        let path = store_path(&app)?;
        let mut store = DeviceLinkStore::load(&path)?;
        let revoked_device = rev.device.clone();
        store.revoke(rev);
        store.save(&path)?;
        let keyring = crate::custody::OsKeyring::legacy();
        let mut this = match read_device_seed(&keyring)? {
            Some(seed) => Some(crate::comms::address_from_secret_hex(&seed)?),
            None => None,
        };
        if this.as_deref() == Some(revoked_device.as_str()) {
            keyring
                .delete(DEVICE_KEY_ACCOUNT)
                .map_err(|e| format!("device key keyring error: {e}"))?;
            this = None;
            // This machine falls back to the comms identity: restart the daemon on the next call.
            let _ = crate::cluster::reload_mesh_identity(&app);
        }
        Ok(links_dto(&store, this))
    })
    .await
}

/// **device_link_export**: this machine's signed link as a code to paste on another of the
/// member's devices. The code is a public attestation (addresses + signatures), not a secret.
#[tauri::command]
pub async fn device_link_export(app: tauri::AppHandle) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let member = crate::comms::device_identity(&app)?;
        let seed = this_device_seed(false)?.ok_or("link this device first")?;
        let device = crate::comms::address_from_secret_hex(&seed)?;
        let store = DeviceLinkStore::load(&store_path(&app)?)?;
        let link = store
            .active_link(&member.address, &device)
            .ok_or("link this device first")?;
        serde_json::to_string(link).map_err(|e| e.to_string())
    })
    .await
}

/// **device_link_import**: add another of the member's own devices from its link code.
#[tauri::command]
pub async fn device_link_import(
    app: tauri::AppHandle,
    code: String,
) -> Result<DeviceLinksDto, String> {
    crate::blocking::off_main(move || {
        let member = crate::comms::device_identity(&app)?;
        let path = store_path(&app)?;
        let mut store = DeviceLinkStore::load(&path)?;
        import_link(&mut store, &member.address, &code)?;
        store.save(&path)?;
        let this = match this_device_seed(false)? {
            Some(seed) => Some(crate::comms::address_from_secret_hex(&seed)?),
            None => None,
        };
        Ok(links_dto(&store, this))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("device_link_tests.rs");
}
