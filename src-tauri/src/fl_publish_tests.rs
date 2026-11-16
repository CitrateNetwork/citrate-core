// 0.5.4 FL design board 07 — publishing a round's requirements file to the bundled IPFS node.
// Pure: a recording kubo fake; nothing reaches a real daemon.
use super::*;
use crate::storage::{AddOutcome, StorageError};
use std::sync::Mutex;

#[derive(Default)]
struct FakeKubo {
    added: Mutex<Vec<Vec<u8>>>,
    pinned: Mutex<Vec<String>>,
    fail_add: bool,
    fail_pin: bool,
}

impl KuboTransport for FakeKubo {
    fn add(&self, _f: &str, bytes: &[u8]) -> std::result::Result<AddOutcome, StorageError> {
        if self.fail_add {
            return Err(StorageError::Transport("connection refused".into()));
        }
        self.added.lock().unwrap().push(bytes.to_vec());
        Ok(AddOutcome {
            cid: "bafkreiflmtest".into(),
            size_bytes: bytes.len() as u64,
        })
    }
    fn pin_add(&self, cid: &str) -> std::result::Result<(), StorageError> {
        if self.fail_pin {
            return Err(StorageError::Transport("pin refused".into()));
        }
        self.pinned.lock().unwrap().push(cid.into());
        Ok(())
    }
    fn pin_rm(&self, _c: &str) -> std::result::Result<(), StorageError> {
        Ok(())
    }
    fn pin_ls(&self) -> std::result::Result<Vec<String>, StorageError> {
        Ok(vec![])
    }
    fn cat(&self, _c: &str) -> std::result::Result<Vec<u8>, StorageError> {
        Ok(vec![])
    }
}

fn req() -> Requirements {
    Requirements {
        v: 1,
        data_classes: vec!["support-tickets".into()],
        architectures: vec!["llama-8b-lora-r8".into()],
        min_vram_gb: 16,
        est_hours: 2,
    }
}

#[test]
fn publishes_the_exact_bytes_the_read_model_accepts_and_their_keccak() {
    let k = FakeKubo::default();
    let out = publish(&req(), &k).unwrap();
    let bytes = k.added.lock().unwrap()[0].clone();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"v":1,"dataClasses":["support-tickets"],"architectures":["llama-8b-lora-r8"],"minVramGb":16,"estHours":2}"#
    );
    assert_eq!(out.cid, "bafkreiflmtest");
    assert_eq!(
        out.requirements_hash,
        format!("0x{}", hex::encode(Keccak256::digest(&bytes)))
    );
    assert_eq!(out.bytes, bytes.len() as u64);
    assert_eq!(
        k.pinned.lock().unwrap().as_slice(),
        ["bafkreiflmtest".to_string()]
    );
}

#[test]
fn bad_requirements_are_refused_before_anything_is_added() {
    let k = FakeKubo::default();
    for (r, why) in [
        (Requirements { v: 2, ..req() }, "version 2"),
        (
            Requirements {
                data_classes: vec![],
                ..req()
            },
            "lowercase labels",
        ),
        (
            Requirements {
                data_classes: vec!["has space".into()],
                ..req()
            },
            "lowercase labels",
        ),
        (
            Requirements {
                min_vram_gb: 1025,
                ..req()
            },
            "out of range",
        ),
        (
            Requirements {
                est_hours: 0,
                ..req()
            },
            "out of range",
        ),
    ] {
        assert!(publish(&r, &k).unwrap_err().contains(why));
    }
    assert!(k.added.lock().unwrap().is_empty());
}

#[test]
fn an_unreachable_or_refusing_ipfs_node_is_said() {
    let k = FakeKubo {
        fail_add: true,
        ..Default::default()
    };
    assert!(publish(&req(), &k)
        .unwrap_err()
        .contains("IPFS node did not take"));
    let p = FakeKubo {
        fail_pin: true,
        ..Default::default()
    };
    assert!(publish(&req(), &p).unwrap_err().contains("not pinned"));
}

#[test]
fn unknown_fields_are_refused() {
    let r: std::result::Result<Requirements, _> = serde_json::from_value(serde_json::json!({
        "v": 1, "dataClasses": ["a"], "architectures": [], "minVramGb": 1, "estHours": 1, "extra": 1
    }));
    assert!(r.is_err());
}

// #724 review P1: what the publisher accepts, every reader accepts (one validator).
#[test]
fn every_published_file_is_one_the_offers_read_model_accepts() {
    let k = FakeKubo::default();
    let out = publish(&req(), &k).unwrap();
    let bytes = k.added.lock().unwrap()[0].clone();
    assert!(crate::fl_market::verify_requirements(&bytes, &out.requirements_hash).is_ok());
}

#[test]
fn the_publisher_refuses_what_the_reader_refuses() {
    let k = FakeKubo::default();
    for r in [
        Requirements {
            architectures: vec![],
            ..req()
        },
        Requirements {
            architectures: vec!["Llama-8B".into()],
            ..req()
        },
        Requirements {
            data_classes: vec!["Support-Tickets".into()],
            ..req()
        },
        Requirements {
            est_hours: 24 * 90 + 1,
            ..req()
        },
    ] {
        let err = publish(&r, &k).unwrap_err();
        let reader = crate::fl_market::requirements_problem(&r);
        assert_eq!(
            Some(err),
            reader,
            "publisher and reader must give the same refusal"
        );
    }
    assert!(k.added.lock().unwrap().is_empty());
}
