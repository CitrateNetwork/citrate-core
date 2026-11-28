//! L0-WP09 (planset v060, L0_INTEGRATION_SEAMS 5): the feature-flag table, `release/flags.json`.
//!
//! Every build-time feature flag is a `bool` const marked with a `// @flag` line right above it,
//! and has exactly one row in the table naming its file, its owning lane, its value in the code,
//! and what gates a change to it: a sign-off record (checked by [`crate::signoff::gate`]) or a
//! named test in the flag's file. `flags_tests.rs` ties the table to the code both ways, and the
//! webview reads the same table through the generated `src/shell/flags.gen.ts`
//! (`scripts/gen-flags.mjs`). It is a build gate: only tests read it.

use serde::Deserialize;

/// The table, repo-relative.
pub(crate) const TABLE_PATH: &str = "release/flags.json";
/// The marker line that sits right above a flag's `const`.
pub(crate) const MARKER: &str = "// @flag";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Table {
    #[serde(rename = "$comment")]
    pub comment: Vec<String>,
    pub flags: Vec<Row>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub name: String,
    /// Repo-relative path of the Rust file that declares the const.
    pub file: String,
    pub lane: String,
    /// The const's value in the code.
    pub default: bool,
    /// The sign-off a `true` value needs, if any.
    pub signoff: Option<SignoffRef>,
    /// The test in `file` that gates a change, if any.
    pub gate: Option<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignoffRef {
    pub id: String,
    pub packet_repo: String,
}

fn is_ident(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A sign-off id names files under `release/signoffs/`, so it is one plain path segment.
fn is_signoff_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

/// Parse and check the table: unique names and sign-off ids, a Rust file under `src-tauri/src/`,
/// a known lane, and at least one gate (a sign-off or a test) on every flag.
pub(crate) fn parse(text: &str) -> Result<Table, String> {
    let t: Table = serde_json::from_str(text).map_err(|e| format!("{TABLE_PATH}: {e}"))?;
    if t.comment.iter().all(|c| c.trim().is_empty()) {
        return Err(format!("{TABLE_PATH}: $comment says what the table is"));
    }
    let mut names: Vec<&str> = Vec::new();
    let mut ids: Vec<&str> = Vec::new();
    for r in &t.flags {
        let at = format!("{TABLE_PATH} row {}", r.name);
        if !is_ident(&r.name) || names.contains(&r.name.as_str()) {
            return Err(format!(
                "{at}: the name must be a unique SCREAMING_CASE const"
            ));
        }
        names.push(&r.name);
        let segments_ok = r
            .file
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..");
        if !r.file.starts_with("src-tauri/src/") || !r.file.ends_with(".rs") || !segments_ok {
            return Err(format!(
                "{at}: file must be a .rs path under src-tauri/src/"
            ));
        }
        let lane_ok = r.lane.len() == 2
            && r.lane.starts_with('l')
            && r.lane[1..].chars().all(|c| c.is_ascii_digit());
        if !lane_ok {
            return Err(format!("{at}: lane must be l0..l9"));
        }
        if r.summary.trim().is_empty() {
            return Err(format!("{at}: a summary says what the flag turns on"));
        }
        match (&r.signoff, &r.gate) {
            (None, None) => return Err(format!("{at}: name a signoff or a gate test")),
            (_, Some(g))
                if !g.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || g.is_empty() =>
            {
                return Err(format!("{at}: gate must be a test fn name"));
            }
            _ => {}
        }
        if let Some(s) = &r.signoff {
            if !is_signoff_id(&s.id) || ids.contains(&s.id.as_str()) {
                return Err(format!(
                    "{at}: the sign-off id must be a unique plain segment"
                ));
            }
            ids.push(&s.id);
            let mut repo = s.packet_repo.split('/');
            let repo_ok = repo.next() == Some("CitrateNetwork")
                && repo.next().is_some_and(|n| !n.is_empty())
                && repo.next().is_none();
            if !repo_ok {
                return Err(format!("{at}: packet_repo must be a CitrateNetwork repo"));
            }
        }
    }
    Ok(t)
}

/// The committed table.
pub(crate) fn load() -> Result<Table, String> {
    parse(include_str!("../../release/flags.json"))
}

/// A const marked as a flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    pub file: String,
    pub name: String,
    pub value: bool,
}

/// `pub(crate) const NAME: bool = true;` (any visibility) as `(NAME, value)`.
fn bool_const(line: &str) -> Option<(&str, bool)> {
    let l = line.trim();
    let l = l
        .strip_prefix("pub(crate) ")
        .or_else(|| l.strip_prefix("pub "))
        .unwrap_or(l);
    let (name, rest) = l.strip_prefix("const ")?.split_once(": bool = ")?;
    let value = match rest {
        "true;" => true,
        "false;" => false,
        _ => return None,
    };
    is_ident(name).then_some((name, value))
}

/// The flags marked in one file. A marker must sit right above a `const NAME: bool = <literal>;`,
/// and a marked name may be declared only once in the file (no `cfg` twin the table cannot see).
pub(crate) fn marked_consts(file: &str, src: &str) -> Result<Vec<Marked>, String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<Marked> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != MARKER {
            continue;
        }
        let Some((name, value)) = lines.get(i + 1).and_then(|l| bool_const(l)) else {
            return Err(format!(
                "{file}:{}: {MARKER} must sit right above `const NAME: bool = true|false;`",
                i + 1
            ));
        };
        let decl = format!("const {name}:");
        let count = lines.iter().filter(|l| l.contains(&decl)).count();
        if count != 1 {
            return Err(format!("{file}: {name} is declared {count} times"));
        }
        out.push(Marked {
            file: file.to_string(),
            name: name.to_string(),
            value,
        });
    }
    Ok(out)
}

/// Every disagreement between the table and the marked consts of `sources` (file, text): a marked
/// const with no row, a row with no marked const, a default that is not the code's value, and a
/// gate test that is not in the flag's file.
pub(crate) fn mismatches(table: &Table, sources: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut marked: Vec<Marked> = Vec::new();
    for (file, src) in sources {
        match marked_consts(file, src) {
            Ok(m) => marked.extend(m),
            Err(e) => out.push(e),
        }
    }
    for m in &marked {
        if !table
            .flags
            .iter()
            .any(|r| r.name == m.name && r.file == m.file)
        {
            out.push(format!(
                "{}: {} is marked {MARKER} but has no row in {TABLE_PATH}",
                m.file, m.name
            ));
        }
    }
    for r in &table.flags {
        let Some(m) = marked.iter().find(|m| m.name == r.name && m.file == r.file) else {
            out.push(format!(
                "{TABLE_PATH} row {} points at no {MARKER} const in {}",
                r.name, r.file
            ));
            continue;
        };
        if m.value != r.default {
            out.push(format!(
                "{TABLE_PATH} row {} says {} but {} has {}",
                r.name, r.default, r.file, m.value
            ));
        }
        if let Some(g) = &r.gate {
            let has_fn = sources
                .iter()
                .any(|(f, s)| f == &r.file && s.contains(&format!("fn {g}(")));
            if !has_fn {
                out.push(format!(
                    "{TABLE_PATH} row {}: gate test {g} is not in {}",
                    r.name, r.file
                ));
            }
        }
    }
    out
}
