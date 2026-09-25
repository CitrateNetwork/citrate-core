// PBA-L4-008 class tripwire (I-2: no secret crosses `invoke`).
//
// `group_invites` returned `Vec<PendingInvite>`, a `Serialize` struct whose `priv_key` field (the
// invite's ECIES private key) serialized whenever it was non-empty — so the key crossed into the
// webview. This scan walks the REAL production sources of the app crate and the kit crate and
// fails if any `#[tauri::command]` return type names a `Serialize` struct that carries a
// secret-named field that is not `skip_serializing`/`skip`.
//
// XR-002 lesson: a source tripwire must never scan its own literals and must prove it can fail.
// Test files (`*_tests.rs`, this file) are excluded, and `negative_control_*` feeds the scanner a
// synthetic source containing the exact pre-fix shape and asserts it is flagged.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Field names that hold key material and must never be serialized across `invoke`.
const SECRET_FIELD_NAMES: &[&str] = &[
    "priv_key",
    "private_key",
    "secret_key",
    "secret",
    "seed",
    "mnemonic",
    "entropy",
    "api_key",
];

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Does `hay` contain `word` as a whole identifier?
fn contains_ident(hay: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(i) = hay[from..].find(word) {
        let at = from + i;
        let before = hay[..at].chars().next_back();
        let after = hay[at + word.len()..].chars().next();
        if !before.is_some_and(is_ident_char) && !after.is_some_and(is_ident_char) {
            return true;
        }
        from = at + word.len();
    }
    false
}

/// The body between the `{` at/after `open` and its matching `}`.
fn braced_body(src: &str, open: usize) -> Option<&str> {
    let start = open + src[open..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in src[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[start + 1..start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Names of `Serialize` structs in `src` that carry a serialized secret-named field.
fn secret_bearing_serialize_structs(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("struct ") {
        let at = from + i;
        from = at + 7;
        // The derive attribute(s) just above the struct.
        let mut head_start = at.saturating_sub(400);
        while !src.is_char_boundary(head_start) {
            head_start += 1;
        }
        let head = &src[head_start..at];
        let derive = head.rfind("#[derive(").map(|d| &head[d..]);
        if !derive.is_some_and(|d| d.contains("Serialize")) {
            continue;
        }
        let name: String = src[at + 7..]
            .chars()
            .take_while(|c| is_ident_char(*c))
            .collect();
        if name.is_empty() {
            continue;
        }
        // Tuple/unit structs have no named fields to leak by name.
        let rest = &src[at + 7 + name.len()..];
        if !rest.trim_start().starts_with('{') && !rest.trim_start().starts_with('<') {
            continue;
        }
        let Some(body) = braced_body(src, at) else {
            continue;
        };
        let mut pending_attrs = String::new();
        for line in body.lines() {
            let t = line.trim();
            if t.starts_with("#[") {
                pending_attrs.push_str(t);
                continue;
            }
            if t.starts_with("//") || t.is_empty() {
                continue;
            }
            let field = t
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ");
            let fname: String = field.chars().take_while(|c| is_ident_char(*c)).collect();
            let skipped = pending_attrs.contains("skip_serializing\"")
                || pending_attrs.contains("skip_serializing)")
                || pending_attrs.contains("skip_serializing,")
                || pending_attrs.contains("skip)")
                || pending_attrs.contains("skip,");
            if SECRET_FIELD_NAMES.contains(&fname.as_str()) && !skipped {
                out.insert(name.clone());
            }
            pending_attrs.clear();
        }
    }
    out
}

/// `(fn name, return type text)` for every `#[tauri::command]` in `src`.
fn command_return_types(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("#[tauri::command]") {
        let at = from + i;
        from = at + 1;
        let Some(fn_off) = src[at..].find("fn ") else {
            continue;
        };
        let sig_start = at + fn_off + 3;
        let name: String = src[sig_start..]
            .chars()
            .take_while(|c| is_ident_char(*c))
            .collect();
        let Some(brace) = src[sig_start..].find('{') else {
            continue;
        };
        let sig = &src[sig_start..sig_start + brace];
        let ret = sig
            .rsplit_once("->")
            .map(|(_, r)| r.trim().to_string())
            .unwrap_or_default();
        out.push((name, ret));
    }
    out
}

/// Violations: commands whose return type names a secret-bearing Serialize struct.
fn violations(sources: &[(String, String)]) -> Vec<String> {
    let mut secret_structs = BTreeSet::new();
    for (_, src) in sources {
        secret_structs.extend(secret_bearing_serialize_structs(src));
    }
    let mut v = Vec::new();
    for (file, src) in sources {
        for (cmd, ret) in command_return_types(src) {
            for s in &secret_structs {
                if contains_ident(&ret, s) {
                    v.push(format!("{file}: command `{cmd}` returns `{ret}` which serializes secret-bearing `{s}`"));
                }
            }
        }
    }
    v
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
            out.push((p.display().to_string(), src));
        }
    }
}

#[test]
fn pba_l4_008_no_invoke_command_returns_a_serialized_secret() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    production_sources(&manifest.join("src"), &mut sources);
    production_sources(&manifest.join("..").join("kit").join("src"), &mut sources);
    assert!(
        sources.len() > 20,
        "the scan must actually see the app + kit sources (saw {})",
        sources.len()
    );
    let cmds: usize = sources
        .iter()
        .map(|(_, s)| command_return_types(s).len())
        .sum();
    assert!(
        cmds > 50,
        "the scan must actually see the invoke commands (saw {cmds})"
    );
    let v = violations(&sources);
    assert!(
        v.is_empty(),
        "secret crosses invoke (I-2):\n{}",
        v.join("\n")
    );
}

#[test]
fn negative_control_the_pre_fix_group_invites_shape_is_flagged() {
    let pre_fix = r#"
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvite {
    pub group: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub priv_key: String,
}
#[tauri::command]
pub async fn group_invites(app: tauri::AppHandle, group: String) -> Result<Vec<PendingInvite>, String> {
    Ok(vec![])
}
"#;
    let v = violations(&[("synthetic.rs".into(), pre_fix.into())]);
    assert_eq!(v.len(), 1, "the pre-fix shape must be flagged: {v:?}");
    // And a skip_serializing field / a key-free view is NOT flagged.
    let fixed = r#"
#[derive(Serialize)]
pub struct View {
    pub group: String,
    #[serde(skip_serializing)]
    pub priv_key: String,
}
#[tauri::command]
pub fn list() -> Result<Vec<View>, String> { Ok(vec![]) }
"#;
    assert!(violations(&[("synthetic.rs".into(), fixed.into())]).is_empty());
}
