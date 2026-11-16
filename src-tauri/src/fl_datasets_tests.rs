//! 0.5.4 WP-H6: the local dataset store. Sealed at rest, hash-chained versions that are never
//! edited, consent per source, and the collector's entry point checked by core.
use super::*;
use crate::custody::CustodyError;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
}

impl MemKeyring {
    fn has_key(&self) -> bool {
        self.store.lock().unwrap().contains_key(KEY_ACCOUNT)
    }
}

impl Keyring for MemKeyring {
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

struct DeadKeyring;

impl Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> std::result::Result<(), CustodyError> {
        Err(CustodyError::KeyringUnavailable)
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fl-ds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    d
}

const SECRET: &str = "the harvest ledger password is plum-orchard-77";

/// A folder with a license, two text files, a JSON Lines file, a hidden file and an image.
fn project(tag: &str) -> PathBuf {
    let d = tmp(tag).join("orchard-notes");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::write(
        d.join("LICENSE"),
        "MIT License\n\nCopyright (c) 2026 a member\n",
    )
    .unwrap();
    std::fs::write(d.join("a.txt"), format!("{SECRET}\n")).unwrap();
    std::fs::write(d.join("b.md"), "# Pruning\nCut above the bud.\n").unwrap();
    std::fs::write(
        d.join("sub/rows.jsonl"),
        "{\"q\":\"when\",\"a\":\"march\"}\n{\"q\":\"when\",\"a\":\"march\"}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("sub/tagged.txt"),
        "SPDX-License-Identifier: CC-BY-4.0\nA tagged note.\n",
    )
    .unwrap();
    std::fs::write(d.join(".env"), "TOKEN=1").unwrap();
    std::fs::write(d.join("photo.png"), [0u8, 1, 2]).unwrap();
    d
}

fn req(name: &str) -> CreateRequest {
    CreateRequest {
        name: name.into(),
        data_class: "orchard-notes".into(),
        source: CreateSource::Folder {
            path: String::new(),
        },
        consent_statement: "Use the files in orchard-notes for this dataset.".into(),
    }
}

fn make(s: &DatasetStore<'_>, tag: &str) -> (String, CreateOutcome) {
    let root = project(tag);
    let ingest = ingest_folder(&root, 1_000).unwrap();
    let out = s.create(&req("Orchard notes"), ingest, 1_000).unwrap();
    (out.version.dataset.clone(), out)
}

fn all_bytes(dir: &Path) -> Vec<u8> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                out.extend_from_slice(&b);
            }
        }
    }
    out
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn web_item(url: &str, text: &str) -> CollectedItem {
    CollectedItem {
        kind: SourceKind::Web,
        locator: url.into(),
        fetched_at_ms: 2_000,
        content_sha256: sha256_hex(text.as_bytes()),
        license: LicenseRecord {
            id: Some("CC-BY-4.0".into()),
            basis: LicenseBasis::Found,
            evidence: Some(url.into()),
        },
        robots: RobotsVerdict::Allowed,
        tos_class: TosClass::Allowlisted,
        collector_version: "citrate-collector/1".into(),
        rows: vec![serde_json::json!({ "text": text })],
    }
}

fn new_web_consent() -> ConsentInput {
    ConsentInput::New {
        source_kind: SourceKind::Web,
        scope: "docs.example.org".into(),
        statement: "Collect the pages on the approved plan.".into(),
    }
}

#[test]
fn a_folder_becomes_version_one_with_a_linked_manifest() {
    let d = tmp("v1");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    assert!(s.list().unwrap().datasets.is_empty());
    assert!(!kr.has_key(), "listing nothing mints no key");
    let (id, out) = make(&s, "v1src");
    assert!(valid_dataset_id(&id));
    let v = &out.version;
    assert_eq!(v.version, 1);
    assert_eq!(v.prev, ZERO_HASH);
    assert_eq!(v.privacy_label, PrivacyLabel::Private);
    // a.txt, b.md, sub/rows.jsonl (2 rows), sub/tagged.txt; LICENSE is read as a license, not a row.
    assert_eq!(v.rows, 5, "{out:?}");
    assert_eq!(
        v.unknown_license_sources, 0,
        "the root LICENSE covers every file"
    );
    assert!(
        out.skipped.iter().any(|x| x.starts_with("photo.png")),
        "{:?}",
        out.skipped
    );
    assert!(
        !out.skipped.iter().any(|x| x.contains(".env")),
        "hidden files are never listed or read"
    );
    let l = s.list().unwrap();
    assert_eq!(l.datasets.len(), 1);
    assert!(l.datasets[0].chain_ok, "{:?}", l.datasets[0].chain_problem);
    let m = s.manifest(&id, 1).unwrap();
    assert_eq!(
        m.manifest_hash,
        keccak_hex(m.manifest_json.as_bytes()),
        "manifestHash is keccak256 of the exact bytes"
    );
    assert_eq!(m.manifest_hash, v.manifest_hash);
    assert_eq!(
        serde_json::to_string(&m.manifest).unwrap(),
        m.manifest_json,
        "one canonical form"
    );
    assert_eq!(m.manifest.schema, MANIFEST_SCHEMA);
    let rows: u32 = m.manifest.sources.iter().map(|s| s.rows).sum();
    assert_eq!(rows, m.manifest.rows);
    for src in &m.manifest.sources {
        assert!(
            src.locator.starts_with("orchard-notes/"),
            "relative locator only: {}",
            src.locator
        );
        assert!(
            !src.locator.contains(&*d.to_string_lossy()),
            "no absolute path in a manifest"
        );
        assert_eq!(src.consent_ref, "c-1");
        assert!(is_hash(&src.content_sha256));
        assert_eq!(src.robots, RobotsVerdict::NotApplicable);
    }
    let tagged = m
        .manifest
        .sources
        .iter()
        .find(|s| s.locator.ends_with("tagged.txt"))
        .unwrap();
    assert_eq!(
        tagged.license.id.as_deref(),
        Some("CC-BY-4.0"),
        "a file's own SPDX tag wins"
    );
    let plain = m
        .manifest
        .sources
        .iter()
        .find(|s| s.locator.ends_with("a.txt"))
        .unwrap();
    assert_eq!(plain.license.id.as_deref(), Some("MIT"));
    assert_eq!(plain.license.basis, LicenseBasis::Found);
}

#[test]
fn nothing_is_stored_in_plaintext() {
    let d = tmp("rest");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "restsrc");
    let bytes = all_bytes(&d.join("datasets"));
    assert!(!bytes.is_empty());
    assert!(!contains(&bytes, SECRET.as_bytes()), "rows are sealed");
    assert!(
        !contains(&bytes, b"Orchard notes"),
        "the dataset record is sealed"
    );
    assert!(
        !contains(&bytes, MANIFEST_SCHEMA.as_bytes()),
        "manifests are sealed"
    );
    let sample = s.sample(&id, 1, 10, 0).unwrap();
    assert!(
        sample
            .rows
            .iter()
            .any(|r| r.text.contains("plum-orchard-77")),
        "it opens with the key"
    );
}

#[test]
fn a_file_copied_to_another_name_does_not_open() {
    let d = tmp("aad");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "aadsrc");
    let dir = d.join("datasets").join(&id);
    std::fs::copy(
        dir.join("v1.manifest.sealed"),
        dir.join("v2.manifest.sealed"),
    )
    .unwrap();
    std::fs::copy(dir.join("v1.rows.sealed"), dir.join("v2.rows.sealed")).unwrap();
    let l = s.list().unwrap();
    assert!(!l.datasets[0].chain_ok, "a copied version is refused");
    assert_eq!(
        l.datasets[0].versions.len(),
        1,
        "only the real version is shown"
    );
    assert!(s.manifest(&id, 2).is_err());
}

#[test]
fn a_changed_byte_is_refused() {
    let d = tmp("tamper");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "tampersrc");
    let p = d.join("datasets").join(&id).join("v1.rows.sealed");
    let mut b = std::fs::read(&p).unwrap();
    let last = b.len() - 1;
    b[last] ^= 1;
    std::fs::write(&p, b).unwrap();
    assert!(s.sample(&id, 1, 3, 0).is_err(), "changed rows do not open");
    assert!(s.rows_of(&id, 1).is_err());
    // A changed manifest breaks the listed chain.
    let m = d.join("datasets").join(&id).join("v1.manifest.sealed");
    let mut b = std::fs::read(&m).unwrap();
    b[40] ^= 1;
    std::fs::write(&m, b).unwrap();
    let l = s.list().unwrap();
    assert!(!l.datasets[0].chain_ok);
    assert!(
        l.datasets[0].versions.is_empty(),
        "a version that does not open is never shown"
    );
}

#[test]
fn clean_previews_then_writes_a_new_version_and_keeps_the_old() {
    let d = tmp("clean");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, out) = make(&s, "cleansrc");
    let ops = vec![CleanOp::Dedupe, CleanOp::RedactSecrets];
    let p = s.clean_preview(&id, 1, &ops).unwrap();
    assert_eq!(p.removed, 1, "one duplicate JSON Lines row");
    assert_eq!(p.kept, out.version.rows - 1);
    assert!(p.diff.iter().any(|r| r.kind == "removed"));
    assert_eq!(
        s.list().unwrap().datasets[0].versions.len(),
        1,
        "a preview writes nothing"
    );
    let wrong = s.clean_apply(&id, 1, &ops, ZERO_HASH, 3_000).unwrap_err();
    assert!(
        wrong.contains("differ from the approved preview"),
        "{wrong}"
    );
    let v2 = s
        .clean_apply(&id, 1, &ops, &p.result_content_hash, 3_000)
        .unwrap();
    assert_eq!(v2.version, 2);
    assert_eq!(v2.prev, out.version.manifest_hash, "v2 links to v1");
    assert_eq!(v2.rows, p.kept);
    assert_eq!(v2.content_hash, p.result_content_hash);
    assert_eq!(v2.derived_by[0].detail, "dedupe, redact-secrets");
    // v1 is untouched.
    assert_eq!(
        s.manifest(&id, 1).unwrap().manifest_hash,
        out.version.manifest_hash
    );
    assert_eq!(
        s.sample(&id, 1, 20, 0).unwrap().total_rows,
        out.version.rows
    );
    // An approval for an older base is refused.
    let stale = s
        .clean_apply(&id, 1, &ops, &p.result_content_hash, 4_000)
        .unwrap_err();
    assert!(stale.contains("no longer the latest"), "{stale}");
    let l = s.list().unwrap();
    assert!(l.datasets[0].chain_ok);
    assert_eq!(l.datasets[0].versions.len(), 2);
}

#[test]
fn clean_steps_are_checked() {
    let d = tmp("cleanchk");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "cleanchksrc");
    assert!(s.clean_preview(&id, 1, &[]).is_err());
    assert!(s
        .clean_preview(&id, 1, &[CleanOp::DropRows { rows: vec![999] }])
        .unwrap_err()
        .contains("does not exist"));
    assert!(s
        .clean_preview(
            &id,
            1,
            &[CleanOp::DropSource {
                source: "s-99".into()
            }]
        )
        .is_err());
    assert!(s
        .clean_preview(
            &id,
            1,
            &[CleanOp::Redact {
                find: String::new(),
                replace: "x".into()
            }]
        )
        .is_err());
    let nothing = s
        .clean_preview(
            &id,
            1,
            &[CleanOp::Redact {
                find: "zzzz-not-there".into(),
                replace: "x".into(),
            }],
        )
        .unwrap_err();
    assert!(nothing.contains("change nothing"), "{nothing}");
    let r = s
        .clean_preview(
            &id,
            1,
            &[CleanOp::Redact {
                find: "plum-orchard-77".into(),
                replace: "[removed]".into(),
            }],
        )
        .unwrap();
    assert_eq!(r.changed, 1);
    assert!(r.diff[0].after.as_deref().unwrap().contains("[removed]"));
    let drop = s
        .clean_preview(
            &id,
            1,
            &[CleanOp::DropSource {
                source: "s-1".into(),
            }],
        )
        .unwrap();
    assert!(drop.removed >= 1);
}

#[test]
fn samples_are_bounded_and_mask_emails() {
    let d = tmp("sample");
    let root = tmp("samplesrc").join("mail");
    std::fs::create_dir_all(&root).unwrap();
    let long = "x".repeat(MAX_SAMPLE_CHARS + 50);
    std::fs::write(root.join("a.txt"), "write to grower@example.org about it").unwrap();
    std::fs::write(root.join("b.txt"), &long).unwrap();
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let out = s
        .create(&req("Mail"), ingest_folder(&root, 5).unwrap(), 5)
        .unwrap();
    let id = out.version.dataset;
    let sm = s.sample(&id, 1, 500, 0).unwrap();
    assert_eq!(
        sm.rows.len(),
        2,
        "n is capped and offset 0 starts at the first row"
    );
    assert!(
        sm.rows[0].text.starts_with("write to <"),
        "{}",
        sm.rows[0].text
    );
    assert!(!sm.rows[0].text.contains("grower@example.org"));
    assert!(sm.rows[1].truncated);
    assert!(sm.rows[1].text.chars().count() <= MAX_SAMPLE_CHARS);
    assert!(!sm.untrusted, "the member's own files are not untrusted");
    assert_eq!(s.sample(&id, 1, 5, 1).unwrap().rows[0].index, 1);
}

#[test]
fn the_collector_appends_with_provenance_under_consent() {
    let d = tmp("append");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "appendsrc");
    let v2 = s
        .append(
            &id,
            1,
            new_web_consent(),
            vec![
                web_item("https://docs.example.org/a", "page a"),
                web_item("https://docs.example.org/b", "page b"),
            ],
            2_000,
        )
        .unwrap();
    assert_eq!(v2.version, 2);
    assert!(v2.untrusted, "web content is untrusted");
    let m = s.manifest(&id, 2).unwrap().manifest;
    let web: Vec<_> = m
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::Web)
        .collect();
    assert_eq!(web.len(), 2);
    assert!(web.iter().all(|w| w.consent_ref == "c-2"
        && w.untrusted
        && w.rows == 1
        && w.collector_version == "citrate-collector/1"));
    let ids: BTreeSet<_> = m.sources.iter().map(|s| s.id.clone()).collect();
    assert_eq!(
        ids.len(),
        m.sources.len(),
        "source ids stay unique across versions"
    );
    assert_eq!(m.consent, vec!["c-1".to_string(), "c-2".to_string()]);
    assert!(s.sample(&id, 2, 20, 0).unwrap().untrusted);
    // The base must be the latest.
    assert!(s
        .append(
            &id,
            1,
            ConsentInput::Existing { id: "c-2".into() },
            vec![web_item("https://docs.example.org/c", "c")],
            3
        )
        .unwrap_err()
        .contains("not the latest"));
    // An existing consent of another source kind does not cover web pages.
    assert!(s
        .append(
            &id,
            2,
            ConsentInput::Existing { id: "c-1".into() },
            vec![web_item("https://docs.example.org/c", "c")],
            3
        )
        .unwrap_err()
        .contains("does not cover"));
}

#[test]
fn core_refuses_collected_items_without_full_provenance() {
    let d = tmp("prov");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "provsrc");
    type Change = Box<dyn Fn(&mut CollectedItem)>;
    let cases: Vec<(&str, Change)> = vec![
        (
            "robots",
            Box::new(|i| i.robots = RobotsVerdict::NotApplicable),
        ),
        ("terms", Box::new(|i| i.tos_class = TosClass::NotApplicable)),
        (
            "content hash",
            Box::new(|i| i.content_sha256 = "abc".into()),
        ),
        ("fetch time", Box::new(|i| i.fetched_at_ms = 0)),
        (
            "collector version",
            Box::new(|i| i.collector_version = " ".into()),
        ),
        ("URL", Box::new(|i| i.locator = "file:///etc/passwd".into())),
        (
            "not a license",
            Box::new(|i| i.license.id = Some("NOASSERTION".into())),
        ),
        (
            "core's own ingest",
            Box::new(|i| i.kind = SourceKind::Folder),
        ),
        ("no rows", Box::new(|i| i.rows.clear())),
    ];
    for (why, change) in cases {
        let mut it = web_item("https://docs.example.org/x", "x");
        change(&mut it);
        let e = s
            .append(&id, 1, new_web_consent(), vec![it], 2)
            .unwrap_err();
        assert!(e.contains("refused"), "{why}: {e}");
    }
    assert_eq!(
        s.list().unwrap().datasets[0].versions.len(),
        1,
        "nothing was added"
    );
    assert_eq!(
        s.consent_list(&id).unwrap().len(),
        1,
        "a refused append records no consent"
    );
}

#[test]
fn withdrawing_consent_drops_its_sources_from_a_new_version() {
    let d = tmp("withdraw");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, out) = make(&s, "withdrawsrc");
    let v2 = s
        .append(
            &id,
            1,
            new_web_consent(),
            vec![web_item("https://docs.example.org/a", "page a")],
            2_000,
        )
        .unwrap();
    let w = s.consent_withdraw(&id, "c-2", 3_000).unwrap();
    assert_eq!(w.consent.withdrawn_at_ms, Some(3_000));
    assert!(w.earlier_versions_kept);
    let v3 = w
        .new_version
        .expect("the latest version had sources under c-2");
    assert_eq!(v3.version, 3);
    assert_eq!(v3.prev, v2.manifest_hash);
    assert_eq!(v3.rows, out.version.rows);
    assert!(!v3.untrusted, "the untrusted web source is gone");
    assert_eq!(v3.derived_by[0].op, "consent-withdrawn");
    assert!(s
        .manifest(&id, 3)
        .unwrap()
        .manifest
        .sources
        .iter()
        .all(|s| s.consent_ref == "c-1"));
    assert!(s
        .consent_withdraw(&id, "c-2", 4)
        .unwrap_err()
        .contains("already withdrawn"));
    let again = s
        .append(
            &id,
            3,
            ConsentInput::Existing { id: "c-2".into() },
            vec![web_item("https://docs.example.org/b", "b")],
            5,
        )
        .unwrap_err();
    assert!(again.contains("withdrawn"), "{again}");
    // Withdrawing a consent with nothing in the latest version writes no version.
    let none = s.consent_withdraw(&id, "c-404", 6);
    assert!(none.is_err());
}

#[test]
fn a_pii_scan_record_binds_to_the_rows_it_scanned() {
    let d = tmp("scan");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, out) = make(&s, "scansrc");
    let mut scan = PiiScanRecord {
        scanner: "citrate-pii/1".into(),
        scanned_at_ms: 9,
        content_hash: ZERO_HASH.into(),
        pass: true,
        counts: BTreeMap::from([("email".to_string(), 0u32)]),
    };
    assert!(s
        .record_pii_scan(&id, 1, &scan)
        .unwrap_err()
        .contains("other rows"));
    scan.content_hash = out.version.content_hash.clone();
    scan.counts.insert("email".into(), 2);
    assert!(s
        .record_pii_scan(&id, 1, &scan)
        .unwrap_err()
        .contains("verdict"));
    scan.pass = false;
    s.record_pii_scan(&id, 1, &scan).unwrap();
    let v = &s.list().unwrap().datasets[0].versions[0];
    assert_eq!(v.pii_scan.as_ref().map(|x| x.pass), Some(false));
    // A clean writes new rows: the old scan does not carry over.
    let ops = vec![CleanOp::Dedupe];
    let p = s.clean_preview(&id, 1, &ops).unwrap();
    let v2 = s
        .clean_apply(&id, 1, &ops, &p.result_content_hash, 10)
        .unwrap();
    assert!(v2.pii_scan.is_none());
}

#[test]
fn deleting_the_last_dataset_removes_the_key() {
    let d = tmp("del");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (a, _) = make(&s, "dela");
    let (b, _) = make(&s, "delb");
    s.delete(&a).unwrap();
    assert!(kr.has_key(), "another dataset still needs it");
    s.delete(&b).unwrap();
    assert!(!kr.has_key());
    assert!(s.list().unwrap().datasets.is_empty());
    assert!(s.delete("ds-0000000000000000").is_err());
    assert!(s
        .delete("../agent")
        .unwrap_err()
        .contains("not a dataset id"));
}

#[test]
fn an_unreachable_keychain_keeps_datasets_closed() {
    let d = tmp("dead");
    let s = DatasetStore::new(d.join("datasets"), &DeadKeyring);
    let root = project("deadsrc");
    let e = s
        .create(&req("x"), ingest_folder(&root, 1).unwrap(), 1)
        .unwrap_err();
    assert!(e.contains("keychain"), "{e}");
    assert!(
        all_bytes(&d.join("datasets")).is_empty(),
        "nothing is written without the key"
    );
}

#[test]
fn datasets_without_their_key_are_never_reminted_over() {
    let d = tmp("nokey");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    make(&s, "nokeysrc");
    kr.delete(KEY_ACCOUNT).unwrap();
    let e = s.list().unwrap_err();
    assert!(e.contains("key is not in the keychain"), "{e}");
    let root = project("nokeysrc2");
    assert!(s
        .create(&req("y"), ingest_folder(&root, 1).unwrap(), 1)
        .is_err());
    assert!(
        !kr.has_key(),
        "no new key is minted over datasets it cannot open"
    );
}

#[test]
fn create_checks_its_inputs() {
    let d = tmp("inputs");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let root = project("inputssrc");
    let mut r = req("ok");
    r.data_class = "Has Spaces".into();
    assert!(s.create(&r, ingest_folder(&root, 1).unwrap(), 1).is_err());
    let mut r = req("ok");
    r.consent_statement = "  ".into();
    assert!(s.create(&r, ingest_folder(&root, 1).unwrap(), 1).is_err());
    let empty = tmp("inputsempty").join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(s
        .create(&req("e"), ingest_folder(&empty, 1).unwrap(), 1)
        .unwrap_err()
        .contains("no readable rows"));
    assert!(!kr.has_key(), "a refused create mints nothing");
    assert!(valid_data_class("support-tickets"));
    assert!(!valid_data_class("-x") && !valid_data_class("") && !valid_data_class(&"a".repeat(49)));
}

#[cfg(unix)]
#[test]
fn folder_ingest_never_follows_links() {
    let root = tmp("links").join("linked");
    std::fs::create_dir_all(&root).unwrap();
    let outside = tmp("links-outside");
    std::fs::write(outside.join("secret.txt"), SECRET).unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("in.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("dir")).unwrap();
    std::fs::write(root.join("real.txt"), "real").unwrap();
    let ing = ingest_folder(&root, 1).unwrap();
    assert_eq!(ing.rows.len(), 1);
    assert!(
        ing.skipped.iter().filter(|s| s.contains("a link")).count() == 2,
        "{:?}",
        ing.skipped
    );
    assert!(!ing.rows.iter().any(|r| r.data.to_string().contains("plum")));
}

#[test]
fn an_unknown_license_is_recorded_as_unknown() {
    let root = tmp("nolicense").join("plain");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "hello").unwrap();
    let ing = ingest_folder(&root, 1).unwrap();
    assert_eq!(ing.sources[0].license, LicenseRecord::unknown());
    let d = tmp("nolicense-store");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let out = s.create(&req("p"), ing, 1).unwrap();
    assert_eq!(out.version.unknown_license_sources, 1);
}

#[test]
fn the_registry_license_rule_matches_the_contract() {
    for ok in [
        "MIT",
        "Apache-2.0",
        "CC-BY-4.0 OR MIT",
        "LicenseRef-orchard-1",
    ] {
        assert!(registry_license_ok(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        " MIT",
        "MIT ",
        "unknown",
        "UNKNOWN",
        "NoAssertion",
        "NOASSERTION",
        "MIT\n",
    ] {
        assert!(registry_license_ok(bad).is_err(), "{bad:?}");
    }
    assert!(registry_license_ok(&"a".repeat(257)).is_err());
    assert_eq!(
        spdx_tag("// SPDX-License-Identifier: Apache-2.0\nfn main(){}").as_deref(),
        Some("Apache-2.0")
    );
    assert_eq!(
        spdx_tag("<!-- SPDX-License-Identifier: MIT -->").as_deref(),
        Some("MIT")
    );
    assert_eq!(spdx_tag("SPDX-License-Identifier: NOASSERTION"), None);
}

#[test]
fn the_dataset_store_is_inside_the_agents_default_deny_list() {
    // Production roots: <app data>/datasets with the Tauri identifier ai.citrate.core.
    #[allow(unused_mut)]
    let mut roots = vec![
        "/Users/m/Library/Application Support/ai.citrate.core/datasets",
        "/home/m/.local/share/ai.citrate.core/datasets",
    ];
    #[cfg(windows)]
    roots.push("C:\\Users\\m\\AppData\\Roaming\\ai.citrate.core\\datasets");
    for p in roots {
        assert!(dataset_root_is_agent_denied(Path::new(p)), "{p}");
    }
    assert!(!dataset_root_is_agent_denied(Path::new(
        "/home/m/projects/datasets"
    )));
    // The identifier the app ships with is the one the agent's PREFIX_RULES deny.
    let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(conf["identifier"], "ai.citrate.core");
    // And core refuses to grant any folder on or under its own data folder, so no grant can
    // reach the store.
    let data = tmp("deny-appdata").join("ai.citrate.core");
    std::fs::create_dir_all(data.join(DATASETS_DIR)).unwrap();
    let home = data.parent().unwrap().to_path_buf();
    let grants =
        crate::agent_grants::GrantStore::new(data.join("agent"), home).protecting(data.clone());
    assert!(grants
        .add_folder(&data.join(DATASETS_DIR), true, true, 1)
        .is_err());
}

#[test]
fn only_an_active_folder_read_grant_lets_a_folder_in() {
    use crate::agent_grants::{Access, Grant, GrantKind, GrantState};
    let g = |id: &str,
             kind: GrantKind,
             access: Access,
             root: &str,
             expires: Option<u64>,
             revoked: Option<u64>| Grant {
        id: id.into(),
        kind,
        root: root.into(),
        access,
        scope: "subtree".into(),
        granted_at: 10,
        expires_at: expires,
        granted_by: "member".into(),
        reason: "test".into(),
        revoked_at: revoked,
    };
    let st = |grants: Vec<Grant>| GrantState {
        version: 1,
        next_id: 9,
        grants,
    };
    let p = Path::new("/home/m/notes/orchard");
    assert!(folder_read_granted(
        &st(vec![g(
            "g-1",
            GrantKind::Folder,
            Access::Read,
            "/home/m/notes",
            None,
            None
        )]),
        p,
        20
    ));
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::Folder,
                Access::Write,
                "/home/m/notes",
                None,
                None
            )]),
            p,
            20
        ),
        "write is not read"
    );
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::Folder,
                Access::Read,
                "/home/m/notes",
                None,
                Some(15)
            )]),
            p,
            20
        ),
        "revoked"
    );
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::Folder,
                Access::Read,
                "/home/m/notes",
                Some(20),
                None
            )]),
            p,
            20
        ),
        "expired"
    );
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::Folder,
                Access::Read,
                "/home/m/notes",
                None,
                None
            )]),
            p,
            5
        ),
        "not yet active"
    );
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::FullAccess,
                Access::Read,
                "/home/m",
                Some(100),
                None
            )]),
            p,
            20
        ),
        "full access is not a folder grant"
    );
    assert!(!folder_read_granted(
        &st(vec![g(
            "g-1",
            GrantKind::Folder,
            Access::Read,
            "/home/m/other",
            None,
            None
        )]),
        p,
        20
    ));
    assert!(
        !folder_read_granted(
            &st(vec![g(
                "g-1",
                GrantKind::Folder,
                Access::Read,
                "/home/m/not",
                None,
                None
            )]),
            p,
            20
        ),
        "a prefix of a name is not a parent"
    );
}

#[test]
fn conversations_need_the_training_switch() {
    let hermes = tmp("conv");
    let e = ingest_conversations(&hermes, 10, 1).unwrap_err();
    assert!(e.contains("off"), "{e}");
}

#[test]
fn conversations_become_rows_when_the_switch_is_on() {
    let hermes = tmp("conv-on");
    crate::fl_trajectories::save(
        &hermes,
        &crate::fl_trajectories::TrajectorySettings {
            enabled: true,
            changed_at_ms: Some(1),
        },
    )
    .unwrap();
    let dir = hermes.join(crate::fl_trajectories::TRAJECTORIES_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    let line = serde_json::json!({
        "messages": [{"role": "user", "content": "prune when"}, {"role": "assistant", "content": "march"}],
        "metadata": {"verifiers": ["tool_succeeded x"]}
    })
    .to_string();
    std::fs::write(dir.join("s1-100.jsonl"), format!("{line}\nnot json\n")).unwrap();
    let ing = ingest_conversations(&hermes, 10, 7).unwrap();
    assert_eq!(ing.rows.len(), 1);
    assert_eq!(ing.sources[0].kind, SourceKind::Conversations);
    assert_eq!(ing.sources[0].locator, "conversations");
    assert!(ing.skipped.iter().any(|s| s.contains("not verified")));
}

#[test]
fn version_files_are_never_overwritten() {
    let d = tmp("noover");
    let kr = MemKeyring::default();
    let s = DatasetStore::new(d.join("datasets"), &kr);
    let (id, _) = make(&s, "nooversrc");
    let keys = s.need_keys().unwrap();
    let e = s.write_file(&keys, &id, "v1.rows", b"x", true).unwrap_err();
    assert!(e.contains("never overwritten"), "{e}");
}
