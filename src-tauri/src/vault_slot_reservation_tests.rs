// Vault slot reservation tripwire.
//
// The webview can write vault slots through `custody_put`, which refuses every backend-owned
// namespace (`custody::BACKEND_SLOT_PREFIXES`). This scan lists every in-process vault write in
// the production sources of the app and kit crates, and each one must be in the allowlist below
// with a slot that the invoke path refuses. A new backend write site fails here until its slot
// namespace is reserved and the site is added to the allowlist.
//
// Test files (`*_tests.rs`, `tests.rs`) are excluded, and `#[cfg(test)]` modules are cut from
// each source before scanning; the negative control proves the scan sees a new write site.

use std::path::{Path, PathBuf};

use citrate_core_kit::custody::is_backend_reserved_slot;

/// `(file name, slot expression)` of every in-process vault write in production code.
const ALLOWED_WRITES: &[(&str, &str)] = &[
    // The OAuth token exchange seals the connection record.
    ("connections.rs", "&slot(service)"),
    // The OIDC refresh token.
    ("oidc.rs", "REFRESH_SLOT"),
    // The wallet keystore (create and import).
    ("wallet.rs", "WALLET_ENTROPY_SLOT"),
    // The invoke-path body itself, guarded by `is_backend_reserved_slot`.
    ("custody.rs", "slot"),
];

/// Cut every `#[cfg(test)]` item (a `mod … { … }` block or a single item) from `src`.
pub(crate) fn without_test_items(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        // The item ends at its matching `}` if it opens a block before a `;`, else at the `;`.
        let brace = after.find('{');
        let semi = after.find(';');
        let end = match (brace, semi) {
            (Some(b), Some(s)) if s < b => s + 1,
            (Some(b), _) => {
                let mut depth = 0usize;
                let mut end = after.len();
                for (j, c) in after[b..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = b + j + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            }
            (None, Some(s)) => s + 1,
            (None, None) => after.len(),
        };
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// `(file name, slot expression)` for every `<vault>.put(<slot>, …)` call in `src`.
fn vault_writes(file: &str, src: &str) -> Vec<(String, String)> {
    let src = without_test_items(src);
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("//") {
            continue;
        }
        let mut from = 0;
        while let Some(i) = t[from..].find(".put(") {
            let at = from + i;
            from = at + 5;
            let receiver: String = t[..at]
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            let is_vault = ["vault", "custody", "state.0", "custody.0"]
                .iter()
                .any(|r| receiver == *r || receiver.ends_with(&format!(".{r}")));
            if !is_vault {
                continue;
            }
            let args = &t[at + 5..];
            let slot = args.split(',').next().unwrap_or("").trim().to_string();
            out.push((file.to_string(), slot));
        }
    }
    out
}

fn production_sources(dir: &Path, out: &mut Vec<(String, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            production_sources(&p, out);
            continue;
        }
        let fname = p.file_name().and_then(|f| f.to_str()).unwrap_or("");
        if !fname.ends_with(".rs") || fname.ends_with("_tests.rs") || fname == "tests.rs" {
            continue;
        }
        if let Ok(src) = std::fs::read_to_string(&p) {
            out.push((fname.to_string(), src));
        }
    }
}

#[test]
fn every_in_process_vault_write_is_allowlisted() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    production_sources(&manifest.join("src"), &mut sources);
    production_sources(&manifest.join("..").join("kit").join("src"), &mut sources);
    assert!(sources.len() > 20, "the scan must see the sources");
    let mut found: Vec<(String, String)> = sources
        .iter()
        .flat_map(|(f, s)| vault_writes(f, s))
        .collect();
    found.sort();
    found.dedup();
    let mut allowed: Vec<(String, String)> = ALLOWED_WRITES
        .iter()
        .map(|(f, s)| (f.to_string(), s.to_string()))
        .collect();
    allowed.sort();
    assert_eq!(
        found, allowed,
        "the in-process vault write sites changed: reserve the new slot namespace in \
         custody::BACKEND_SLOT_PREFIXES, then add the site to ALLOWED_WRITES"
    );
}

#[test]
fn every_backend_slot_is_refused_on_the_invoke_path() {
    for svc in crate::connections::ALL_SERVICES {
        let slot = crate::connections::slot(svc);
        assert!(is_backend_reserved_slot(&slot), "{slot} must be reserved");
    }
    for network in ["x", "github", "discord", "telegram"] {
        let slot = crate::social::keyring_slot(network);
        assert!(is_backend_reserved_slot(&slot), "{slot} must be reserved");
    }
    assert!(is_backend_reserved_slot(
        citrate_core_kit::oidc::REFRESH_SLOT
    ));
    assert!(is_backend_reserved_slot(
        citrate_core_kit::wallet::WALLET_ENTROPY_SLOT
    ));
}

#[test]
fn negative_control_a_new_vault_write_site_is_seen() {
    let src = r#"
fn store(vault: &CustodyVault, mut b: Vec<u8>) {
    let _ = vault.put("plugin-token", &mut b);
}
#[cfg(test)]
mod tests {
    fn t(vault: &CustodyVault) { vault.put("ignored-in-tests", &mut vec![]).unwrap(); }
}
"#;
    assert_eq!(
        vault_writes("plugin.rs", src),
        vec![("plugin.rs".to_string(), "\"plugin-token\"".to_string())]
    );
}
