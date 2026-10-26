// PBA-L4-008 class tripwire (I-2: no secret crosses `invoke`).
//
// `group_invites` returned `Vec<PendingInvite>`, a `Serialize` struct whose `priv_key` field (the
// invite's ECIES private key) serialized whenever it was non-empty — so the key crossed into the
// webview. This scan walks the REAL production sources of the app crate and the kit crate and
// fails if any `#[tauri::command]` return type names a `Serialize` struct or enum that serializes
// a secret-named field that is not `skip_serializing`/`skip`, directly or through the serialized
// fields of the types it contains, at any depth.
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

/// One serialized field of a `Serialize` type: its name (empty for a tuple field) and type text.
struct Field {
    name: String,
    ty: String,
}

/// The body between the `(` at/after `open` and its matching `)`.
fn paren_body(src: &str, open: usize) -> Option<&str> {
    let start = open + src[open..].find('(')?;
    let mut depth = 0usize;
    for (i, c) in src[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
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

/// Whether the collected `#[...]` attributes skip the field from serialization.
fn is_skipped(attrs: &str) -> bool {
    attrs.contains("skip_serializing\"")
        || attrs.contains("skip_serializing)")
        || attrs.contains("skip_serializing,")
        || attrs.contains("skip)")
        || attrs.contains("skip,")
}

/// The serialized fields in a struct body, an enum body, or a tuple-struct body. Enum variants
/// are flattened: a struct variant's named fields and a tuple variant's types all count.
fn serialized_fields(body: &str) -> Vec<Field> {
    let mut out = Vec::new();
    let mut pending_attrs = String::new();
    // Split at the item separators so `Variant { a: A, b: B }` and `Variant(A)` both yield
    // their fields; generic arguments may split too, which only adds harmless type pieces.
    let normalized: String = body
        .lines()
        .map(|l| {
            let t = l.trim();
            if t.starts_with("//") {
                ""
            } else {
                t
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut piece = String::new();
    let mut pieces = Vec::new();
    let mut attr_depth = 0usize;
    let mut chars = normalized.chars().peekable();
    while let Some(c) = chars.next() {
        if attr_depth == 0 && c == '#' && chars.peek() == Some(&'[') {
            if !piece.trim().is_empty() {
                pieces.push(std::mem::take(&mut piece));
            }
            piece.push(c);
            attr_depth = 1;
            chars.next();
            piece.push('[');
            continue;
        }
        if attr_depth > 0 {
            piece.push(c);
            match c {
                '[' => attr_depth += 1,
                ']' => {
                    attr_depth -= 1;
                    if attr_depth == 0 {
                        pieces.push(std::mem::take(&mut piece));
                    }
                }
                _ => {}
            }
            continue;
        }
        match c {
            '{' | '}' | '(' | ')' | ',' | '\n' => {
                if !piece.trim().is_empty() {
                    pieces.push(std::mem::take(&mut piece));
                }
                piece.clear();
            }
            _ => piece.push(c),
        }
    }
    if !piece.trim().is_empty() {
        pieces.push(piece);
    }
    for p in pieces {
        let t = p.trim();
        if t.starts_with("#[") {
            pending_attrs.push_str(t);
            continue;
        }
        let t = t
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub ");
        let (name, ty) = match t.split_once(':') {
            Some((n, ty)) if !n.contains('<') && !ty.starts_with(':') => {
                (n.trim().to_string(), ty.trim().to_string())
            }
            _ => (String::new(), t.to_string()),
        };
        if !is_skipped(&pending_attrs) {
            out.push(Field { name, ty });
        }
        pending_attrs.clear();
    }
    out
}

/// Every `Serialize` struct or enum in `src`, with its serialized fields.
fn serialize_types(src: &str) -> Vec<(String, Vec<Field>)> {
    let mut out = Vec::new();
    for kw in ["struct ", "enum "] {
        let mut from = 0;
        while let Some(i) = src[from..].find(kw) {
            let at = from + i;
            from = at + kw.len();
            if at > 0 && src[..at].chars().next_back().is_some_and(is_ident_char) {
                continue;
            }
            // The derive attribute(s) just above the item.
            let mut head_start = at.saturating_sub(400);
            while !src.is_char_boundary(head_start) {
                head_start += 1;
            }
            let head = &src[head_start..at];
            let derive = head.rfind("#[derive(").map(|d| &head[d..]);
            if !derive.is_some_and(|d| d.contains("Serialize")) {
                continue;
            }
            let name: String = src[at + kw.len()..]
                .chars()
                .take_while(|c| is_ident_char(*c))
                .collect();
            if name.is_empty() {
                continue;
            }
            let rest = src[at + kw.len() + name.len()..].trim_start();
            let body = if rest.starts_with('(') {
                paren_body(src, at + kw.len() + name.len())
            } else if rest.starts_with('{') || rest.starts_with('<') || rest.starts_with("where") {
                braced_body(src, at)
            } else {
                None
            };
            if let Some(body) = body {
                out.push((name, serialized_fields(body)));
            }
        }
    }
    out
}

/// Names of `Serialize` types in `sources` that serialize a secret: directly, through a
/// secret-named field, or through a serialized field of a type that does (at any depth).
fn secret_bearing_types(sources: &[(String, String)]) -> BTreeSet<String> {
    let types: Vec<(String, Vec<Field>)> = sources
        .iter()
        .flat_map(|(_, src)| serialize_types(src))
        .collect();
    let mut secret: BTreeSet<String> = types
        .iter()
        .filter(|(_, fields)| {
            fields
                .iter()
                .any(|f| SECRET_FIELD_NAMES.contains(&f.name.as_str()))
        })
        .map(|(n, _)| n.clone())
        .collect();
    loop {
        let before = secret.len();
        for (name, fields) in &types {
            if secret.contains(name) {
                continue;
            }
            if fields
                .iter()
                .any(|f| secret.iter().any(|s| contains_ident(&f.ty, s)))
            {
                secret.insert(name.clone());
            }
        }
        if secret.len() == before {
            return secret;
        }
    }
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
    let secret_structs = secret_bearing_types(sources);
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

#[test]
fn negative_control_a_secret_nested_below_the_return_type_is_flagged() {
    // The secret-bearing struct is reached through a field (directly, in a Vec, in an Option and
    // two levels down), and through an enum variant. Each command must be flagged.
    let nested = r#"
#[derive(Serialize)]
pub struct Inner {
    pub group: String,
    pub priv_key: String,
}
#[derive(Serialize)]
pub struct Middle {
    pub label: String,
    pub inner: Inner,
}
#[derive(Serialize)]
pub struct Outer {
    pub items: Vec<Middle>,
}
#[derive(Serialize)]
pub struct MaybeOuter {
    pub one: Option<Inner>,
}
#[derive(Serialize)]
pub enum Event {
    Plain(String),
    Leaky { inner: Inner },
}
#[tauri::command]
pub fn direct() -> Result<Middle, String> { todo() }
#[tauri::command]
pub fn deep() -> Result<Outer, String> { todo() }
#[tauri::command]
pub fn optional() -> Result<MaybeOuter, String> { todo() }
#[tauri::command]
pub fn event() -> Result<Vec<Event>, String> { todo() }
"#;
    let v = violations(&[("synthetic.rs".into(), nested.into())]);
    for cmd in ["direct", "deep", "optional", "event"] {
        assert!(
            v.iter().any(|m| m.contains(&format!("command `{cmd}`"))),
            "`{cmd}` reaches a secret through a nested type and must be flagged: {v:?}"
        );
    }
    // A field skipped from serialization does not carry the nested secret across.
    let skipped = r#"
#[derive(Serialize)]
pub struct Inner {
    pub priv_key: String,
}
#[derive(Serialize)]
pub struct View {
    pub label: String,
    #[serde(skip)]
    pub inner: Inner,
}
#[tauri::command]
pub fn view() -> Result<View, String> { todo() }
"#;
    assert!(violations(&[("synthetic.rs".into(), skipped.into())]).is_empty());
}
