// 0.5.4 FL design board 11 — the sealed worker terms and HIC-2 envelope (ML-KEM-768 + HKDF-SHA256
// + AES-256-GCM, key in the OS keyring). Pure: temp dirs and an in-memory keyring.
use super::*;
use crate::custody::CustodyError;
use std::collections::HashMap;
use std::sync::Mutex as StdMutex;

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-flwork-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&p);
        TestDir(p)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct MemKeyring {
    store: StdMutex<HashMap<String, Vec<u8>>>,
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

fn terms() -> WorkerTerms {
    WorkerTerms {
        floor_salt_per_unit: "1.20".into(),
        hours: Hours::Window {
            start: "23:00".into(),
            end: "07:00".into(),
        },
        weekdays_only: true,
        mains_only: true,
        data_classes: vec!["support-tickets".into()],
    }
}

fn input() -> EnvelopeInput {
    EnvelopeInput {
        name: "Weeknight rounds".into(),
        joins_per_week: 3,
        max_hours_per_round: 6,
    }
}

#[test]
fn nothing_saved_reads_none_and_mints_no_key() {
    let d = TestDir::new("empty");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    assert_eq!(s.terms().unwrap(), None);
    assert_eq!(s.envelope(0).unwrap(), None);
    assert!(k.get(KEY_ACCOUNT).unwrap().is_none(), "a read never mints a key");
}

#[test]
fn terms_round_trip_sealed_with_a_receipt() {
    let d = TestDir::new("terms");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    let r = s.save_terms(terms(), 1_000).unwrap();
    assert_eq!(r.saved_at, 1_000);
    assert!(r.sealed);
    assert_eq!(r.terms_hash.len(), 64);
    assert_eq!(s.terms().unwrap(), Some(terms()));
    let raw = std::fs::read(d.0.join("terms.sealed")).unwrap();
    assert_eq!(&raw[..8], MAGIC);
    let text = String::from_utf8_lossy(&raw);
    assert!(!text.contains("support-tickets"), "the file is not plaintext");
    assert!(!text.contains("1.20"));
}

#[test]
fn any_hours_round_trip_and_serde_matches_the_webview() {
    let t = WorkerTerms {
        hours: Hours::Any("any".into()),
        ..terms()
    };
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["hours"], "any");
    assert_eq!(json["floorSaltPerUnit"], "1.20");
    let w = serde_json::to_value(terms()).unwrap();
    assert_eq!(w["hours"]["start"], "23:00");
    let back: WorkerTerms = serde_json::from_value(json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn malformed_terms_are_refused_and_nothing_is_written() {
    let d = TestDir::new("bad");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    for (t, why) in [
        (
            WorkerTerms {
                floor_salt_per_unit: "-1".into(),
                ..terms()
            },
            "floor",
        ),
        (
            WorkerTerms {
                floor_salt_per_unit: "1.".into(),
                ..terms()
            },
            "floor",
        ),
        (
            WorkerTerms {
                hours: Hours::Window {
                    start: "25:00".into(),
                    end: "07:00".into(),
                },
                ..terms()
            },
            "Hours",
        ),
        (
            WorkerTerms {
                hours: Hours::Any("sometimes".into()),
                ..terms()
            },
            "Hours",
        ),
        (
            WorkerTerms {
                data_classes: vec!["".into()],
                ..terms()
            },
            "data class",
        ),
    ] {
        let e = s.save_terms(t, 1).unwrap_err();
        assert!(e.contains(why), "{e}");
    }
    assert!(!d.0.join("terms.sealed").exists());
}

#[test]
fn a_changed_byte_or_a_renamed_file_does_not_open() {
    let d = TestDir::new("tamper");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    s.save_terms(terms(), 1).unwrap();
    let path = d.0.join("terms.sealed");
    let mut raw = std::fs::read(&path).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 1;
    std::fs::write(&path, &raw).unwrap();
    assert!(s.terms().unwrap_err().contains("does not open"));

    let d2 = TestDir::new("rename");
    let s2 = WorkStore::new(&d2.0, &k);
    s2.save_terms(terms(), 1).unwrap();
    std::fs::rename(d2.0.join("terms.sealed"), d2.0.join("envelope.sealed")).unwrap();
    assert!(s2.envelope(1).unwrap_err().contains("does not open"));
}

#[test]
fn files_without_their_key_are_refused_and_no_key_is_minted_over_them() {
    let d = TestDir::new("nokey");
    let k = MemKeyring::default();
    WorkStore::new(&d.0, &k).save_terms(terms(), 1).unwrap();
    let fresh = MemKeyring::default();
    let s = WorkStore::new(&d.0, &fresh);
    assert!(s.terms().unwrap_err().contains("key is not in the keychain"));
    assert!(s.save_terms(terms(), 2).unwrap_err().contains("key is not in the keychain"));
    assert!(fresh.get(KEY_ACCOUNT).unwrap().is_none());
}

#[test]
fn an_unreachable_keyring_refuses_reads_and_writes() {
    let d = TestDir::new("dead");
    let s = WorkStore::new(&d.0, &DeadKeyring);
    assert!(s.save_terms(terms(), 1).unwrap_err().contains("keychain is not reachable"));
    assert!(s.terms().unwrap_err().contains("keychain is not reachable"));
}

#[test]
fn envelope_approve_off_and_the_week_of_use() {
    let d = TestDir::new("env");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    assert_eq!(s.envelope_off(5).unwrap(), None, "nothing to turn off");
    let e = s.approve_envelope(input(), 1_000).unwrap();
    assert!(e.on);
    assert_eq!((e.joins_per_week, e.max_hours_per_round, e.joins_this_week), (3, 6, 0));
    assert_eq!(s.envelope(2_000).unwrap(), Some(e.clone()));
    let off = s.envelope_off(3_000).unwrap().unwrap();
    assert!(!off.on);
    assert_eq!(off.off_at, Some(3_000));
    assert_eq!(s.envelope(3_001).unwrap().unwrap().on, false);

    let used = Envelope {
        joins_this_week: 2,
        ..e
    };
    let rolled = roll_week(used.clone(), 1_000 + MS_PER_WEEK * 2 + 5);
    assert_eq!(rolled.joins_this_week, 0);
    assert_eq!(rolled.week_start, 1_000 + MS_PER_WEEK * 2);
    assert_eq!(roll_week(used.clone(), 1_000 + MS_PER_WEEK - 1).joins_this_week, 2);
}

#[test]
fn envelope_caps_are_bounded() {
    let d = TestDir::new("caps");
    let k = MemKeyring::default();
    let s = WorkStore::new(&d.0, &k);
    for (i, why) in [
        (
            EnvelopeInput {
                joins_per_week: 0,
                ..input()
            },
            "Joins per week",
        ),
        (
            EnvelopeInput {
                joins_per_week: MAX_JOINS_PER_WEEK + 1,
                ..input()
            },
            "Joins per week",
        ),
        (
            EnvelopeInput {
                max_hours_per_round: 0,
                ..input()
            },
            "Machine hours",
        ),
        (
            EnvelopeInput {
                name: "  ".into(),
                ..input()
            },
            "Name the envelope",
        ),
    ] {
        assert!(s.approve_envelope(i, 1).unwrap_err().contains(why));
    }
    assert!(!d.0.join("envelope.sealed").exists());
}

#[test]
fn unknown_fields_from_the_webview_are_refused() {
    let r: Result<WorkerTerms, _> = serde_json::from_value(serde_json::json!({
        "floorSaltPerUnit": "1.20", "hours": "any", "weekdaysOnly": true, "mainsOnly": true,
        "dataClasses": [], "autoJoin": true
    }));
    assert!(r.is_err());
    let e: Result<EnvelopeInput, _> = serde_json::from_value(serde_json::json!({
        "name": "x", "joinsPerWeek": 1, "maxHoursPerRound": 1, "on": true
    }));
    assert!(e.is_err());
}

#[cfg(unix)]
#[test]
fn files_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = TestDir::new("perm");
    let k = MemKeyring::default();
    WorkStore::new(&d.0, &k).save_terms(terms(), 1).unwrap();
    let m = std::fs::metadata(d.0.join("terms.sealed")).unwrap();
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
    let dm = std::fs::metadata(&d.0).unwrap();
    assert_eq!(dm.permissions().mode() & 0o777, 0o700);
}
