// WP-H7: the dataset scan is recorded against the version's rows, licenses are checked per source,
// and the publish gate refuses every case the owner's policy names. Datasets are real store
// datasets in a scratch folder under an in-memory keyring (the OS keyring is a test seam only).

use super::*;
use crate::custody::{CustodyError, Keyring};
use crate::fl_datasets::{
    ingest_folder, CollectedItem, ConsentInput, CreateRequest, CreateSource, LicenseRecord,
    RobotsVerdict, TosClass,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct MemKeyring(StdMutex<HashMap<String, Vec<u8>>>);

impl Keyring for MemKeyring {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.0.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.lock().unwrap().insert(a.to_string(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        self.0.lock().unwrap().remove(a);
        Ok(())
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fl-gate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A granted folder with `license` (a LICENSE file, or none) and one note per entry of `notes`.
fn folder(tag: &str, license: Option<&str>, notes: &[&str]) -> PathBuf {
    let d = tmp(tag).join("notes");
    std::fs::create_dir_all(&d).unwrap();
    if let Some(l) = license {
        std::fs::write(d.join("LICENSE"), format!("SPDX-License-Identifier: {l}\n")).unwrap();
    }
    for (i, n) in notes.iter().enumerate() {
        std::fs::write(d.join(format!("n{i}.txt")), format!("{n}\n")).unwrap();
    }
    d
}

fn create(s: &DatasetStore<'_>, root: &std::path::Path) -> (String, u32) {
    let ingest = ingest_folder(root, 1_000).unwrap();
    let out = s
        .create(
            &CreateRequest {
                name: "Orchard notes".into(),
                data_class: "orchard-notes".into(),
                source: CreateSource::Folder {
                    path: String::new(),
                },
                consent_statement: "Use the files in notes for this dataset.".into(),
            },
            ingest,
            1_000,
        )
        .unwrap();
    (out.version.dataset.clone(), out.version.version)
}

const CLEAN: &str = "Prune apple trees in late winter, above an outward bud.";

fn publish(license: &str, privacy: PrivacyLabel) -> PublishRequest<'_> {
    PublishRequest { license, privacy }
}

#[test]
fn a_clean_version_scans_clean_and_the_record_binds_its_rows() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("clean-store"), &kr);
    let (id, v) = create(&s, &folder("clean", Some("CC-BY-4.0"), &[CLEAN]));
    assert_eq!(recorded_scan(&s, &id, v).unwrap(), None, "not scanned yet");
    let r = scan_version(&s, &id, v, 2_000).unwrap();
    assert!(r.passed, "{r:?}");
    let rec = recorded_scan(&s, &id, v).unwrap().expect("recorded");
    assert!(rec.pass);
    assert_eq!(rec.scanner, crate::pii_scan::SCANNER);
    let (m, _) = s.rows_of(&id, v).unwrap();
    assert_eq!(rec.content_hash, m.content_hash);
    let check = publish_gate(&m, Some(&rec), &publish("CC-BY-4.0", PrivacyLabel::Public));
    assert!(check.allowed, "{check:?}");
}

#[test]
fn found_private_information_blocks_publish_and_is_counted_by_class() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("pii-store"), &kr);
    let mail = ["dana", "@", "example.org"].concat();
    let (id, v) = create(
        &s,
        &folder(
            "pii",
            Some("CC0-1.0"),
            &[CLEAN, &format!("Ask Dana at {mail} or 202-555-0143.")],
        ),
    );
    let r = scan_version(&s, &id, v, 2_000).unwrap();
    assert!(!r.passed);
    let rec = recorded_scan(&s, &id, v).unwrap().unwrap();
    assert_eq!(rec.counts.get("email"), Some(&1));
    assert_eq!(rec.counts.get("phone"), Some(&1));
    let (m, _) = s.rows_of(&id, v).unwrap();
    for privacy in [
        PrivacyLabel::Public,
        PrivacyLabel::Members,
        PrivacyLabel::Group,
    ] {
        let c = publish_gate(&m, Some(&rec), &publish("CC0-1.0", privacy));
        assert!(!c.allowed);
        assert!(
            c.refusals
                .iter()
                .any(|r| r.contains("found 1 email, 1 phone")),
            "{c:?}"
        );
    }
}

#[test]
fn personal_data_may_be_shared_with_members_but_never_public() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("label-store"), &kr);
    let mail = ["bo", "@", "example.org"].concat();
    let (id, v) = create(
        &s,
        &folder("label", Some("CC0-1.0"), &[&format!("Mail {mail}")]),
    );
    scan_version(&s, &id, v, 2_000).unwrap();
    let rec = recorded_scan(&s, &id, v).unwrap().unwrap();
    let (mut m, _) = s.rows_of(&id, v).unwrap();
    m.contains_personal_data = true;
    assert!(publish_gate(&m, Some(&rec), &publish("CC0-1.0", PrivacyLabel::Members)).allowed);
    let public = publish_gate(&m, Some(&rec), &publish("CC0-1.0", PrivacyLabel::Public));
    assert!(!public.allowed);
    assert!(public
        .refusals
        .iter()
        .any(|r| r.contains("can never be Public")));
    // Even a clean scan does not make a personal-data dataset Public.
    let mut clean = rec.clone();
    clean.pass = true;
    clean.counts.clear();
    assert!(!publish_gate(&m, Some(&clean), &publish("CC0-1.0", PrivacyLabel::Public)).allowed);
}

#[test]
fn no_scan_a_stale_scan_or_private_visibility_is_refused() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("stale-store"), &kr);
    let (id, v) = create(&s, &folder("stale", Some("MIT"), &[CLEAN]));
    let (m, _) = s.rows_of(&id, v).unwrap();
    let none = publish_gate(&m, None, &publish("MIT", PrivacyLabel::Public));
    assert!(none
        .refusals
        .iter()
        .any(|r| r.contains("has not been scanned")));
    let mut stale = scan_record(&crate::pii_scan::scan_rows([CLEAN]), &m.content_hash, 1);
    stale.content_hash = crate::fl_datasets::ZERO_HASH.to_string();
    let c = publish_gate(&m, Some(&stale), &publish("MIT", PrivacyLabel::Public));
    assert!(c.refusals.iter().any(|r| r.contains("other rows")));
    // The store refuses to record it, too.
    assert!(s.record_pii_scan(&id, v, &stale).is_err());
    let ok = scan_record(&crate::pii_scan::scan_rows([CLEAN]), &m.content_hash, 1);
    let private = publish_gate(&m, Some(&ok), &publish("MIT", PrivacyLabel::Private));
    assert!(private
        .refusals
        .iter()
        .any(|r| r.contains("stays on this device")));
}

#[test]
fn every_source_needs_a_known_publishable_license() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("lic-store"), &kr);
    let (id, v) = create(&s, &folder("lic", None, &[CLEAN]));
    // The member's own folder with no license file: the dataset license covers it.
    let (m, _) = s.rows_of(&id, v).unwrap();
    let r = license_check(&m);
    assert!(r.ok, "{r:?}");
    assert_eq!(r.sources[0].verdict, LicenseVerdict::MemberOwned);
    // A web source with no license, and one under a NonCommercial license.
    let item = |locator: &str, license: LicenseRecord| CollectedItem {
        kind: SourceKind::Web,
        locator: locator.into(),
        fetched_at_ms: 3_000,
        content_sha256: format!("0x{}", "ab".repeat(32)),
        license,
        robots: RobotsVerdict::Allowed,
        tos_class: TosClass::MemberApproved,
        collector_version: "citrate-collector/1".into(),
        rows: vec![serde_json::json!({ "text": CLEAN })],
    };
    let v2 = s
        .append(
            &id,
            v,
            ConsentInput::New {
                source_kind: SourceKind::Web,
                scope: "example.org".into(),
                statement: "Collect two pages from example.org.".into(),
            },
            vec![
                item("https://example.org/a", LicenseRecord::unknown()),
                item(
                    "https://example.org/b",
                    LicenseRecord {
                        id: Some("CC-BY-NC-4.0".into()),
                        basis: LicenseBasis::Found,
                        evidence: Some("page footer".into()),
                    },
                ),
            ],
            3_000,
        )
        .unwrap();
    let (m2, _) = s.rows_of(&id, v2.version).unwrap();
    let r2 = license_check(&m2);
    assert!(!r2.ok);
    let verdicts: Vec<LicenseVerdict> = r2.sources.iter().map(|s| s.verdict).collect();
    assert_eq!(
        verdicts,
        vec![
            LicenseVerdict::MemberOwned,
            LicenseVerdict::Unknown,
            LicenseVerdict::NotPublishable
        ]
    );
    scan_version(&s, &id, v2.version, 4_000).unwrap();
    let rec = recorded_scan(&s, &id, v2.version).unwrap().unwrap();
    let c = publish_gate(&m2, Some(&rec), &publish("CC0-1.0", PrivacyLabel::Public));
    assert!(!c.allowed);
    assert_eq!(c.refusals.len(), 2, "{c:?}");
}

#[test]
fn a_share_alike_source_fixes_the_dataset_license_and_registry_rules_apply() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("sa-store"), &kr);
    let (id, v) = create(&s, &folder("sa", Some("CC-BY-SA-4.0"), &[CLEAN]));
    scan_version(&s, &id, v, 2_000).unwrap();
    let rec = recorded_scan(&s, &id, v).unwrap().unwrap();
    let (m, _) = s.rows_of(&id, v).unwrap();
    assert!(
        publish_gate(
            &m,
            Some(&rec),
            &publish("CC-BY-SA-4.0", PrivacyLabel::Public)
        )
        .allowed
    );
    let c = publish_gate(&m, Some(&rec), &publish("CC0-1.0", PrivacyLabel::Public));
    assert!(c
        .refusals
        .iter()
        .any(|r| r.contains("must be published under CC-BY-SA-4.0")));
    for bad in ["", "unknown", "NOASSERTION", " MIT", "CC-BY-NC-4.0"] {
        let c = publish_gate(&m, Some(&rec), &publish(bad, PrivacyLabel::Public));
        assert!(
            c.refusals
                .iter()
                .any(|r| r.starts_with("the dataset license is refused")),
            "{bad}"
        );
    }
}

#[test]
fn every_publishable_license_passes_the_registry_rule() {
    for l in PUBLISHABLE_LICENSES {
        assert!(registry_license_ok(l).is_ok(), "{l}");
        assert!(!l.contains("-NC") && !l.contains("-ND"), "{l}");
    }
}

#[test]
fn json_keys_numbers_and_key_value_pairs_are_scanned() {
    // Codex review of #734 (P1): the scan read only JSON string values, so object keys, numbers
    // and the key next to its value were never scanned, although ingest and publish keep them.
    let mail = ["alice", "@", "example.org"].concat();
    let card: u64 = ["4111", "1111", "1111", "1111"].concat().parse().unwrap();
    let pw = ["violet", "orchard", "7392"].join("-");
    let row = |data: serde_json::Value| crate::fl_datasets::Row {
        source: "s1".into(),
        data,
    };
    let mut key_and_number = serde_json::Map::new();
    key_and_number.insert(mail.clone(), serde_json::json!(card));
    let cases = [
        ("an email as a key, a card as a number", serde_json::Value::Object(key_and_number), &["email", "card"][..]),
        ("a password value next to its key", serde_json::json!({ "password": pw }), &["secret"][..]),
        ("a card as a nested number", serde_json::json!({ "pay": [{ "card": card }] }), &["card"][..]),
    ];
    for (what, data, classes) in cases {
        let r = scan_manifest_rows(&[row(data)]);
        assert!(!r.passed, "{what}: {r:?}");
        for c in classes {
            assert!(r.counts.keys().any(|k| k.key() == *c), "{what}: no {c} in {:?}", r.counts);
        }
    }
    // Plain structure is still clean.
    let clean = scan_manifest_rows(&[row(serde_json::json!({ "title": "Orchards", "year": 1987, "ok": true, "tags": ["apple", null] }))]);
    assert!(clean.passed, "{clean:?}");
}
