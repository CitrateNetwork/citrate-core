// HUP-S8.1 device-link tests: the golden message vector shared with citrate-cluster, the random
// device key in the keyring, EIP-191 sign/recover with the scoped keys, the local store, the mesh
// identity choice, and the full ceremony-gated flow over a REAL vault + ceremony (the wallet
// signature must come from the ceremony and must recover to the wallet).

use super::*;
use crate::custody::CustodyError;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

const M1: &str = "00000000000000000000000000000000000000a1";
const W1: &str = "00000000000000000000000000000000000000b1";
const D1: &str = "00000000000000000000000000000000000000d1";

fn seed(byte: u8) -> String {
    let mut b = [byte; 32];
    b[0] = byte | 1;
    hex::encode(b)
}

fn addr_of(seed_hex: &str) -> String {
    crate::comms::address_from_secret_hex(seed_hex).expect("valid seed")
}

// ---- the signed text: byte-identical to cluster-core ----

#[test]
fn signing_message_golden_vector_matches_cluster_core() {
    // The same vector is pinned in citrate-cluster crates/cluster-core/src/device_tests.rs.
    let b = DeviceLinkBody::new(
        "0x00000000000000000000000000000000000000A1",
        D1,
        W1,
        0,
        "Studio Mac",
        1_790_000_000,
    )
    .expect("valid");
    assert_eq!(
        b.signing_message(),
        "Citrate DeviceLink v1\n\
         Link this device to my Citrate member identity.\n\
         member: 0x00000000000000000000000000000000000000a1\n\
         device: 0x00000000000000000000000000000000000000d1\n\
         wallet: 0x00000000000000000000000000000000000000b1\n\
         index: 0\n\
         label: Studio Mac\n\
         issued_at: 1790000000"
    );
}

#[test]
fn revocation_message_golden_vector_matches_cluster_core() {
    assert_eq!(
        revocation_message(M1, D1, 1_790_000_100),
        "Citrate DeviceRevocation v1\n\
         Remove this device from my Citrate member identity.\n\
         member: 0x00000000000000000000000000000000000000a1\n\
         device: 0x00000000000000000000000000000000000000d1\n\
         revoked_at: 1790000100"
    );
}

#[test]
fn body_rules_match_cluster_core() {
    assert!(DeviceLinkBody::new(M1, M1, W1, 0, "x", 1).is_err());
    assert!(DeviceLinkBody::new(M1, W1, W1, 0, "x", 1).is_err());
    assert!(DeviceLinkBody::new(M1, D1, W1, MAX_DEVICE_INDEX + 1, "x", 1).is_err());
    assert!(DeviceLinkBody::new(M1, D1, W1, 0, "two\nlines", 1).is_err());
    assert!(DeviceLinkBody::new(M1, D1, W1, 0, "member: 0x1", 1).is_err());
    assert!(DeviceLinkBody::new(M1, D1, W1, 0, &"x".repeat(MAX_LABEL_LEN + 1), 1).is_err());
    assert!(DeviceLinkBody::new(M1, D1, W1, 0, "larry's linux-box_2.0", 1).is_ok());
}

// ---- EIP-191 with scoped keys ----

#[test]
fn scoped_key_signatures_recover_to_the_key_address() {
    let s = seed(0x11);
    let sig = sign_eip191(&s, "hello").expect("sign");
    assert_eq!(recover_eip191("hello", &sig), Some(addr_of(&s)));
    assert_ne!(recover_eip191("hellO", &sig), Some(addr_of(&s)));
    assert_eq!(recover_eip191("hello", "0x00"), None);
}

#[test]
fn scoped_signatures_are_low_s_and_v_27_28() {
    for b in 1u8..20 {
        let sig = sign_eip191(&seed(b), &format!("m{b}")).expect("sign");
        let raw = hex::decode(sig.trim_start_matches("0x")).expect("hex");
        assert_eq!(raw.len(), 65);
        assert!(raw[64] == 27 || raw[64] == 28);
        let s = k256::ecdsa::Signature::from_slice(&raw[..64]).expect("sig");
        assert!(s.normalize_s().is_none(), "signature must already be low-s");
    }
}

// ---- the device key ----

#[test]
fn device_key_is_minted_once_then_reused() {
    let kr = FakeKeyring::default();
    assert!(read_device_seed(&kr).expect("read").is_none());
    let first = load_or_mint_device_seed(&kr, || {
        Zeroizing::new(<[u8; 32]>::try_from(hex::decode(seed(0x21)).expect("hex")).expect("32"))
    })
    .expect("mint");
    let again = load_or_mint_device_seed(&kr, || panic!("must not mint twice")).expect("load");
    assert_eq!(*first, *again);
    assert_eq!(*first, seed(0x21));
}

#[test]
fn a_corrupt_device_key_fails_closed() {
    let kr = FakeKeyring::default();
    kr.set(DEVICE_KEY_ACCOUNT, &[0u8; 32]).expect("set"); // zero is not a valid scalar
    assert!(read_device_seed(&kr).is_err());
    assert!(load_or_mint_device_seed(&kr, mint_device_seed).is_err());
    kr.set(DEVICE_KEY_ACCOUNT, &[1u8; 7]).expect("set");
    assert!(read_device_seed(&kr).is_err());
}

#[test]
fn minted_device_keys_are_random_and_distinct() {
    let a = mint_device_seed();
    let b = mint_device_seed();
    assert_ne!(*a, *b);
    assert!(k256::ecdsa::SigningKey::from_slice(a.as_ref()).is_ok());
}

#[test]
fn device_key_is_not_the_comms_key() {
    // The device key lives under its own keyring account, never the comms accounts.
    assert_ne!(DEVICE_KEY_ACCOUNT, "comms-member-key-v2");
    assert_ne!(DEVICE_KEY_ACCOUNT, "comms-member-key");
}

// ---- the store ----

fn wire(
    member_seed: &str,
    device_seed: &str,
    wallet_seed: &str,
    index: u32,
    label: &str,
) -> DeviceLinkWire {
    let b = DeviceLinkBody::new(
        &addr_of(member_seed),
        &addr_of(device_seed),
        &addr_of(wallet_seed),
        index,
        label,
        1_790_000_000,
    )
    .expect("valid");
    let msg = b.signing_message();
    DeviceLinkWire {
        member_sig: sign_eip191(member_seed, &msg).expect("sign"),
        device_sig: sign_eip191(device_seed, &msg).expect("sign"),
        wallet_sig: sign_eip191(wallet_seed, &msg).expect("sign"),
        member: b.member,
        device: b.device,
        wallet: b.wallet,
        index: b.index,
        label: b.label,
        issued_at: b.issued_at,
    }
}

#[test]
fn verify_link_checks_all_three_signatures() {
    let w = wire(&seed(1), &seed(2), &seed(3), 0, "laptop");
    assert_eq!(verify_link(&w), Ok(()));
    for which in 0..3 {
        let mut bad = w.clone();
        let other = sign_eip191(&seed(9), &"x".repeat(3)).expect("sign");
        match which {
            0 => bad.member_sig = other,
            1 => bad.device_sig = other,
            _ => bad.wallet_sig = other,
        }
        assert!(verify_link(&bad).is_err());
    }
}

#[test]
fn store_round_trips_and_reissue_replaces_by_device() {
    let dir = std::env::temp_dir().join(format!("cdl-store-{}", std::process::id()));
    let path = dir.join("device-links.json");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        DeviceLinkStore::load(&path).expect("missing = empty"),
        DeviceLinkStore::default()
    );
    let mut st = DeviceLinkStore::default();
    st.upsert_link(wire(&seed(1), &seed(2), &seed(3), 0, "old"))
        .expect("add");
    st.upsert_link(wire(&seed(1), &seed(2), &seed(3), 0, "new"))
        .expect("replace");
    st.upsert_link(wire(&seed(1), &seed(4), &seed(3), 1, "box"))
        .expect("add");
    assert_eq!(st.links.len(), 2);
    st.save(&path).expect("save");
    let back = DeviceLinkStore::load(&path).expect("load");
    assert_eq!(back, st);
    assert_eq!(back.links[0].label, "new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    std::fs::write(&path, b"{not json").expect("write");
    assert!(
        DeviceLinkStore::load(&path).is_err(),
        "corrupt store fails closed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn revocation_drops_the_link_and_blocks_relinking_that_key() {
    let mut st = DeviceLinkStore::default();
    let w = wire(&seed(1), &seed(2), &seed(3), 0, "laptop");
    st.upsert_link(w.clone()).expect("add");
    let rev = sign_revocation(&seed(1), &w.device, 1_790_000_100).expect("revoke");
    // The revocation is a real member signature over the cluster-core text.
    assert_eq!(
        recover_eip191(
            &revocation_message(&rev.member, &rev.device, rev.revoked_at),
            &rev.member_sig
        ),
        Some(addr_of(&seed(1)))
    );
    st.revoke(rev.clone());
    st.revoke(rev); // idempotent
    assert!(st.links.is_empty());
    assert_eq!(st.revocations.len(), 1);
    assert!(
        st.upsert_link(w).is_err(),
        "a revoked key cannot be linked again"
    );
}

#[test]
fn index_reuses_an_existing_slot_and_otherwise_counts_up() {
    let mut st = DeviceLinkStore::default();
    let m = addr_of(&seed(1));
    assert_eq!(st.index_for(&m, &addr_of(&seed(2))), 0);
    st.upsert_link(wire(&seed(1), &seed(2), &seed(3), 0, "a"))
        .expect("add");
    assert_eq!(st.index_for(&m, &addr_of(&seed(2))), 0);
    assert_eq!(st.index_for(&m, &addr_of(&seed(4))), 1);
}

#[test]
fn store_caps_links_at_the_daemon_limit() {
    let mut st = DeviceLinkStore::default();
    for i in 0..MAX_LINKS {
        let mut w = wire(&seed(1), &seed(2), &seed(3), 0, "a");
        w.device = format!("{:040x}", i + 1);
        st.upsert_link(w).expect("under the cap");
    }
    let mut w = wire(&seed(1), &seed(2), &seed(3), 0, "a");
    w.device = format!("{:040x}", MAX_LINKS + 10);
    assert!(st.upsert_link(w).is_err());
}

// ---- the mesh identity choice (default changes nothing) ----

fn ident(s: &str) -> crate::comms::DeviceIdentity {
    crate::comms::DeviceIdentity {
        seed_hex: Zeroizing::new(s.to_string()),
        address: addr_of(s),
    }
}

#[test]
fn mesh_identity_stays_comms_unless_libp2p_and_linked() {
    let (member, device) = (seed(1), seed(2));
    let mut st = DeviceLinkStore::default();
    // No link: comms, whatever the transport.
    for libp2p in [false, true] {
        let got = choose_mesh_identity(ident(&member), Some(ident(&device)), &st, libp2p);
        assert_eq!(got.address, addr_of(&member));
    }
    st.upsert_link(wire(&member, &device, &seed(3), 0, "laptop"))
        .expect("add");
    // Linked but in-process transport: unchanged.
    let got = choose_mesh_identity(ident(&member), Some(ident(&device)), &st, false);
    assert_eq!(got.address, addr_of(&member));
    // Linked + cross-machine transport: the device key is the PeerId.
    let got = choose_mesh_identity(ident(&member), Some(ident(&device)), &st, true);
    assert_eq!(got.address, addr_of(&device));
    assert_eq!(*got.seed_hex, device);
    // Revoked: back to comms.
    st.revoke(sign_revocation(&member, &addr_of(&device), 5).expect("rev"));
    let got = choose_mesh_identity(ident(&member), Some(ident(&device)), &st, true);
    assert_eq!(got.address, addr_of(&member));
}

#[test]
fn mesh_identity_ignores_a_link_for_another_member() {
    let mut st = DeviceLinkStore::default();
    st.upsert_link(wire(&seed(7), &seed(2), &seed(3), 0, "x"))
        .expect("add");
    let got = choose_mesh_identity(ident(&seed(1)), Some(ident(&seed(2))), &st, true);
    assert_eq!(got.address, addr_of(&seed(1)));
}

// ---- the full ceremony-gated flow over a REAL vault + ceremony ----

const PASS: &[u8] = b"correct horse battery staple";
const CANONICAL_MNEMONIC: &str = "test test test test test test test test test test test junk";

fn vault_with_wallet() -> (CustodyVault, PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-devicelink-test-{}-{}.enc",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&p);
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), p.clone(), 0);
    v.init(&mut PASS.to_vec()).expect("init");
    v.unlock(&mut PASS.to_vec()).expect("unlock");
    crate::wallet::import(&v, CANONICAL_MNEMONIC).expect("import wallet");
    (v, p)
}

fn body_for(vault: &CustodyVault, member_seed: &str, device_seed: &str) -> DeviceLinkBody {
    let wallet = crate::wallet::address(vault).expect("wallet");
    DeviceLinkBody::new(
        &addr_of(member_seed),
        &addr_of(device_seed),
        &wallet.address,
        0,
        "Studio Mac",
        1_790_000_000,
    )
    .expect("valid")
}

#[test]
fn approved_link_carries_a_ceremony_wallet_signature_that_recovers_to_the_wallet() {
    let (vault, path) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let flow = DeviceLinkFlow::default();
    let (member, device) = (seed(0x31), seed(0x32));
    let body = body_for(&vault, &member, &device);
    let view = flow.open(&ceremony, 40204, body.clone());
    assert_eq!(view.origin, "local-user");
    assert!(
        !view.requires_raw_ack,
        "the link text is legible and decodes"
    );
    assert_eq!(flow.pending_count(), 1);

    let link = flow
        .approve(&vault, &ceremony, &view.id, false, &member, &device)
        .expect("approve");
    assert_eq!(flow.pending_count(), 0);
    assert_eq!(verify_link(&link), Ok(()));
    let wallet =
        canonical_address(&crate::wallet::address(&vault).expect("w").address).expect("addr");
    assert_eq!(
        recover_eip191(&body.signing_message(), &link.wallet_sig),
        Some(wallet)
    );
    // The ceremony is spent: a second approve cannot mint another signature.
    assert!(flow
        .approve(&vault, &ceremony, &view.id, false, &member, &device)
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn approve_refuses_before_spending_the_ceremony_if_a_key_changed() {
    let (vault, path) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let flow = DeviceLinkFlow::default();
    let (member, device) = (seed(0x41), seed(0x42));
    let view = flow.open(&ceremony, 40204, body_for(&vault, &member, &device));
    // The device key was replaced between request and approve.
    assert!(flow
        .approve(&vault, &ceremony, &view.id, false, &member, &seed(0x43))
        .is_err());
    // Nothing was consumed: the right keys still complete the link.
    assert!(flow
        .approve(&vault, &ceremony, &view.id, false, &member, &device)
        .is_ok());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_declined_link_signs_nothing_and_cannot_be_approved() {
    let (vault, path) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let flow = DeviceLinkFlow::default();
    let (member, device) = (seed(0x51), seed(0x52));
    let view = flow.open(&ceremony, 40204, body_for(&vault, &member, &device));
    ceremony.reject(&view.id).expect("reject");
    flow.forget(&view.id);
    assert!(flow
        .approve(&vault, &ceremony, &view.id, false, &member, &device)
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_locked_vault_cannot_complete_a_link() {
    let (vault, path) = vault_with_wallet();
    let ceremony = SignatureCeremony::new();
    let flow = DeviceLinkFlow::default();
    let (member, device) = (seed(0x61), seed(0x62));
    let view = flow.open(&ceremony, 40204, body_for(&vault, &member, &device));
    vault.lock();
    assert!(flow
        .approve(&vault, &ceremony, &view.id, false, &member, &device)
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn links_dto_marks_this_device_and_carries_no_signatures() {
    let mut st = DeviceLinkStore::default();
    let w = wire(&seed(1), &seed(2), &seed(3), 0, "laptop");
    st.upsert_link(w.clone()).expect("add");
    let dto = links_dto(&st, Some(w.device.clone()));
    assert!(dto.links[0].this_device);
    let json = serde_json::to_string(&dto).expect("json");
    assert!(!json.contains(w.member_sig.trim_start_matches("0x")));
    assert!(json.contains("\"thisDevice\""));
    assert!(json.contains("\"issuedAt\""));
}

#[test]
fn a_store_holding_both_a_link_and_its_revocation_never_meshes_as_the_device() {
    // A store file edited by hand (or merged from two machines) can carry a link AND its
    // revocation; the revocation must win.
    let (member, device) = (seed(1), seed(2));
    let st = DeviceLinkStore {
        links: vec![wire(&member, &device, &seed(3), 0, "laptop")],
        revocations: vec![sign_revocation(&member, &addr_of(&device), 5).expect("rev")],
    };
    let got = choose_mesh_identity(ident(&member), Some(ident(&device)), &st, true);
    assert_eq!(got.address, addr_of(&member));
}

// ---- export / import of your own devices ----

#[test]
fn importing_your_own_other_device_verifies_and_stores_it() {
    let (member, wallet) = (seed(1), seed(3));
    let other = wire(&member, &seed(4), &wallet, 1, "Linux box");
    let code = serde_json::to_string(&other).expect("json");
    let mut st = DeviceLinkStore::default();
    let got = import_link(&mut st, &format!("0x{}", addr_of(&member)), &code).expect("import");
    assert_eq!(got.device, addr_of(&seed(4)));
    assert_eq!(st.links.len(), 1);
}

#[test]
fn import_refuses_another_members_device_a_forgery_and_junk() {
    let mut st = DeviceLinkStore::default();
    let theirs = wire(&seed(7), &seed(4), &seed(3), 0, "theirs");
    let code = serde_json::to_string(&theirs).expect("json");
    assert!(import_link(&mut st, &addr_of(&seed(1)), &code).is_err());
    let mut forged = wire(&seed(1), &seed(4), &seed(3), 0, "box");
    forged.device_sig = sign_eip191(&seed(9), "x").expect("sign");
    let code = serde_json::to_string(&forged).expect("json");
    assert!(import_link(&mut st, &addr_of(&seed(1)), &code).is_err());
    assert!(import_link(&mut st, &addr_of(&seed(1)), "not json").is_err());
    assert!(import_link(&mut st, &addr_of(&seed(1)), &"x".repeat(5000)).is_err());
    assert!(st.links.is_empty(), "nothing refused was stored");
}

#[test]
fn import_stores_canonical_addresses() {
    let member = seed(1);
    let mut w = wire(&member, &seed(4), &seed(3), 0, "box");
    w.device = format!("0x{}", w.device.to_uppercase());
    let code = serde_json::to_string(&w).expect("json");
    let mut st = DeviceLinkStore::default();
    let got = import_link(&mut st, &addr_of(&member), &code).expect("import");
    assert_eq!(got.device, addr_of(&seed(4)));
}
