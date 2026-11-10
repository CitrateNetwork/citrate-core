// Tripwire, owner decision D-9 (2026-10-26): the liquid staking pool is retired at the r1006 reroll
// and is not in the genesis contract set. The core crate, its address book and the kit make no pool
// call: no pool address, selector, event, command or self-stake read may come back.
//
// The banned strings live in fixtures/retired/liquid-staking-pool.json, which the webview tripwire
// (src/wallet/lspRetired.tripwire.test.ts) reads too, so the two lists cannot drift. This file
// names none of them itself.

use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
struct Retired {
    scan: Vec<String>,
    addresses: Vec<String>,
    selectors: Vec<String>,
    strings: Vec<String>,
    allow: std::collections::HashMap<String, Vec<String>>,
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("src-tauri has a parent"))
}

fn retired() -> Retired {
    let p = repo().join("fixtures/retired/liquid-staking-pool.json");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// `a1b2c3d4` as a pinned selector const writes it: `0xa1, 0xb2, 0xc3, 0xd4`.
fn byte_form(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|c| format!("0x{}", String::from_utf8_lossy(c)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every banned string in `text`, except those `allowed` for its file.
fn hits(r: &Retired, text: &str, allowed: &[String]) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    for a in &r.addresses {
        if lower.contains(&a.to_ascii_lowercase()) {
            out.push(a.clone());
        }
    }
    for s in &r.selectors {
        if lower.contains(s.as_str()) || lower.contains(&byte_form(s)) {
            out.push(s.clone());
        }
    }
    for s in &r.strings {
        if !allowed.contains(s) && text.contains(s.as_str()) {
            out.push(s.clone());
        }
    }
    out
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        panic!("{} is not readable", dir.display())
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("rs" | "json" | "toml" | "ts" | "tsx" | "md" | "txt" | "sh")
        ) {
            out.push(p);
        }
    }
}

/// Every banned string in `text` (no allowance): for fixtures and data outside the scanned trees.
pub(crate) fn retired_hits(text: &str) -> Vec<String> {
    hits(&retired(), text, &[])
}

#[test]
fn the_scanner_finds_each_kind_of_pool_reference() {
    let r = retired();
    let addr = r.addresses[0].to_ascii_uppercase().replacen("0X", "0x", 1);
    assert_eq!(hits(&r, &format!("to: {addr}"), &[]).len(), 1);
    let sel = &r.selectors[0];
    assert_eq!(
        hits(
            &r,
            &format!("const S: [u8; 4] = [{}];", byte_form(sel)),
            &[]
        ),
        vec![sel.clone()]
    );
    for s in &r.strings {
        assert!(hits(&r, &format!("x {s} y"), &[]).contains(s), "{s}");
    }
    assert!(hits(&r, &format!("x {} y", r.strings[0]), &r.strings[..1]).is_empty());
}

#[test]
fn no_pool_reference_in_the_core_crate_its_address_book_or_the_kit() {
    let r = retired();
    let root = repo();
    let me = Path::new(file!())
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    let mut found = Vec::new();
    let mut scanned = 0usize;
    for dir in r.scan.iter().filter(|d| d.as_str() != "src") {
        let mut all = Vec::new();
        files(&root.join(dir), &mut all);
        for p in all {
            let rel = p
                .strip_prefix(&root)
                .map(|x| x.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if rel == format!("src-tauri/src/{me}") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            scanned += 1;
            let allowed = r.allow.get(&rel).cloned().unwrap_or_default();
            for h in hits(&r, &text, &allowed) {
                found.push(format!("{rel}: {h}"));
            }
        }
    }
    assert!(scanned > 200, "scanned only {scanned} files");
    assert!(
        found.is_empty(),
        "retired pool references (D-9):\n{}",
        found.join("\n")
    );
}
