//! L0-WP09 (planset v060, L0_INTEGRATION_SEAMS 3.7 and 5): the feature-flag table,
//! `release/flags.json`, in the 3.7 shape:
//! `{ "version": 1, "flags": [ { "id", "lane", "where", "default", "signoff": "required" | "none", "record"? } ] }`.
//!
//! - `id`: the flag's SCREAMING_CASE name.
//! - `lane`: its owning lane, `L0` to `L9` (L0 section 2.2: the lane edits its rows; L0 reviews the
//!   schema; the owner approves every change, CODEOWNERS).
//! - `where`: the repo-relative file that holds its value. A `.rs` file under `src-tauri/src/`
//!   declares it as a `bool` const with a `// @flag <ID>` line right above it, and the row's
//!   `default` is that const's value. `where` equal to `release/flags.json` itself is a table-only
//!   flag: no Rust const reads it; scripts read the JSON and the webview reads `flag(id)` from
//!   `src/shell/flags.ts`. (3.7 lists `PQC_CLAIMS_ALLOWED` with no `where`, and L0-WP12's
//!   `cx-copy-lint` reads it from this file.) The 3.7 examples' `:line` suffixes are left out:
//!   the marker finds the const, and line numbers drift.
//! - `signoff`: `required` means a `true` value needs a sign-off record that verifies
//!   ([`crate::signoff::gate`]); `none` means the flag's own tests gate it.
//! - `record`: the record a `required` flag needs, `release/signoffs/<id>.json`; when left out it is
//!   `release/signoffs/<ID>.json`. The record's file stem is the sign-off id.
//!
//! `flags_tests.rs` ties the table to the code both ways and runs the sign-off gate; the webview
//! reads the same table through the generated `src/shell/flags.gen.ts` (`scripts/gen-flags.mjs`).
//! It is a build gate: only tests read it. A flag never defaults on a feature whose data source
//! does not exist (Rule 1).

use serde::Deserialize;

/// The table, repo-relative. Also the `where` of a table-only flag.
pub(crate) const TABLE_PATH: &str = "release/flags.json";
/// The marker that sits right above a flag's `const`, followed by one space and the flag's id.
pub(crate) const MARKER: &str = "// @flag";
/// Where sign-off records live, repo-relative.
const RECORD_DIR: &str = "release/signoffs/";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Table {
    pub version: u32,
    pub flags: Vec<Row>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    pub id: String,
    pub lane: String,
    #[serde(rename = "where")]
    pub where_: String,
    /// The flag's value in this build.
    pub default: bool,
    pub signoff: Signoff,
    #[serde(default)]
    pub record: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum Signoff {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "none")]
    NotNeeded,
}

impl Row {
    /// The record a `true` value needs, repo-relative; `None` when the flag needs no sign-off.
    pub(crate) fn record_path(&self) -> Option<String> {
        (self.signoff == Signoff::Required).then(|| {
            self.record
                .clone()
                .unwrap_or_else(|| format!("{RECORD_DIR}{}.json", self.id))
        })
    }

    /// The sign-off id: the record's file stem (`CL-S4` for `release/signoffs/CL-S4.json`).
    pub(crate) fn signoff_id(&self) -> Option<String> {
        // A path of another shape keeps its full text, which `parse` then refuses.
        self.record_path().map(|p| {
            p.strip_prefix(RECORD_DIR)
                .and_then(|f| f.strip_suffix(".json"))
                .map_or_else(|| p.clone(), str::to_string)
        })
    }

    /// The flag lives only in the table (no Rust const).
    pub(crate) fn table_only(&self) -> bool {
        self.where_ == TABLE_PATH
    }
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

/// A `.rs` path under `src-tauri/src/` with no empty, `.` or `..` segment.
fn is_rust_source(p: &str) -> bool {
    p.starts_with("src-tauri/src/")
        && p.ends_with(".rs")
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Parse and check the table: version 1, unique ids and sign-off ids, a known lane, a `where`
/// that is a Rust source or the table itself, and a `record` only on a `required` row, naming one
/// file directly in `release/signoffs/`.
pub(crate) fn parse(text: &str) -> Result<Table, String> {
    let t: Table = serde_json::from_str(text).map_err(|e| format!("{TABLE_PATH}: {e}"))?;
    if t.version != 1 {
        return Err(format!("{TABLE_PATH}: version must be 1"));
    }
    let mut ids: Vec<&str> = Vec::new();
    let mut signoffs: Vec<String> = Vec::new();
    for r in &t.flags {
        let at = format!("{TABLE_PATH} row {}", r.id);
        if !is_ident(&r.id) || ids.contains(&r.id.as_str()) {
            return Err(format!("{at}: id must be a unique SCREAMING_CASE name"));
        }
        ids.push(&r.id);
        let lane_ok = r.lane.len() == 2
            && r.lane.starts_with('L')
            && r.lane[1..].chars().all(|c| c.is_ascii_digit());
        if !lane_ok {
            return Err(format!("{at}: lane must be L0..L9"));
        }
        if !r.table_only() && !is_rust_source(&r.where_) {
            return Err(format!(
                "{at}: where must be a .rs path under src-tauri/src/ or {TABLE_PATH}"
            ));
        }
        if r.record.is_some() && r.signoff != Signoff::Required {
            return Err(format!(
                "{at}: only a signoff \"required\" row names a record"
            ));
        }
        if let (Some(path), Some(id)) = (r.record_path(), r.signoff_id()) {
            let shape_ok = path == format!("{RECORD_DIR}{id}.json") && is_signoff_id(&id);
            if !shape_ok || signoffs.contains(&id) {
                return Err(format!(
                    "{at}: record must be a unique release/signoffs/<id>.json, id one plain segment"
                ));
            }
            signoffs.push(id);
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
    pub id: String,
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

/// A line that is meant as a flag marker: a `//` comment whose text starts with `@flag`, in any
/// spacing (`// @flag`, `//@flag`, `//  @flag X`). Doc comments (`///`, `//!`) are not markers.
fn looks_like_marker(line: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix("//").is_some_and(|rest| {
        !rest.starts_with('/') && !rest.starts_with('!') && rest.trim_start().starts_with("@flag")
    })
}

/// The flags marked in one file. A marker is exactly `// @flag <ID>`, sits right above
/// `const <ID>: bool = <literal>;` with the same id, and that id is declared only once in the file
/// (no `cfg` twin the table cannot see). A malformed marker is refused, never skipped.
pub(crate) fn marked_consts(file: &str, src: &str) -> Result<Vec<Marked>, String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out: Vec<Marked> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !looks_like_marker(line) {
            continue;
        }
        let at = format!("{file}:{}", i + 1);
        let id = line
            .trim()
            .strip_prefix(MARKER)
            .and_then(|r| r.strip_prefix(' '))
            .filter(|id| is_ident(id))
            .ok_or_else(|| format!("{at}: a marker is exactly `{MARKER} <ID>`"))?;
        let Some((name, value)) = lines.get(i + 1).and_then(|l| bool_const(l)) else {
            return Err(format!(
                "{at}: `{MARKER} {id}` must sit right above `const {id}: bool = true|false;`"
            ));
        };
        if name != id {
            return Err(format!("{at}: `{MARKER} {id}` marks const {name}"));
        }
        // Count declarations in code lines only. A comment that quotes `const NAME:` is not a
        // twin; a cfg twin is always code. (Counting comments too would fail closed on prose.)
        let decl = format!("const {name}:");
        let count = lines
            .iter()
            .filter(|l| !l.trim_start().starts_with("//") && l.contains(&decl))
            .count();
        if count != 1 {
            return Err(format!("{file}: {name} is declared {count} times"));
        }
        out.push(Marked {
            file: file.to_string(),
            id: name.to_string(),
            value,
        });
    }
    Ok(out)
}

/// Every disagreement between the table and the marked consts of `sources` (file, text): a
/// malformed marker, a marked const with no row, a Rust row with no marked const, and a default
/// that is not the code's value.
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
            .any(|r| r.id == m.id && r.where_ == m.file)
        {
            out.push(format!(
                "{}: {} is marked {MARKER} but has no row in {TABLE_PATH}",
                m.file, m.id
            ));
        }
    }
    for r in table.flags.iter().filter(|r| !r.table_only()) {
        let Some(m) = marked.iter().find(|m| m.id == r.id && m.file == r.where_) else {
            out.push(format!(
                "{TABLE_PATH} row {} points at no `{MARKER} {}` const in {}",
                r.id, r.id, r.where_
            ));
            continue;
        };
        if m.value != r.default {
            out.push(format!(
                "{TABLE_PATH} row {} says {} but {} has {}",
                r.id, r.default, r.where_, m.value
            ));
        }
    }
    out
}

/// The value a `required` row's sign-off gate checks: on when the table's `default` is on or, for
/// a Rust row, when its marked const in `where` is on. Gating on the code as well as the table
/// means a const flipped without a table edit still needs its record. A Rust row whose marked
/// const is not in `sources` is refused, never read as off.
pub(crate) fn gated_value(row: &Row, sources: &[(String, String)]) -> Result<bool, String> {
    if row.table_only() {
        return Ok(row.default);
    }
    let missing = || {
        format!(
            "{TABLE_PATH} row {} points at no `{MARKER} {}` const in {}",
            row.id, row.id, row.where_
        )
    };
    let (file, src) = sources
        .iter()
        .find(|(f, _)| *f == row.where_)
        .ok_or_else(missing)?;
    let marked = marked_consts(file, src)?;
    let m = marked.iter().find(|m| m.id == row.id).ok_or_else(missing)?;
    Ok(row.default || m.value)
}
