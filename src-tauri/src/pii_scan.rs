//! The deterministic on-device private-information scanner (WP-H7; owner decision 2026-11-16,
//! collection policy §3.1 rule 6, and V6 for @Hermes group answers).
//!
//! One scanner, used wherever core must decide on text before it leaves the member's device:
//! a dataset version before it can be published, and a Hermes answer before it is posted to a
//! group. The model may propose redactions; this scanner decides. It is pure and offline: byte
//! scanners, the `bip39` English wordlist and a compiled-in list of common given names. Nothing is
//! sent anywhere and no model is asked.
//!
//! | class | found as |
//! |---|---|
//! | `email` | `local@domain.tld` ([`crate::secret_shapes`]) |
//! | `phone` | 10 to 15 digits written as a phone number: `+` and digits, or two or more groups split by single spaces, `-`, `.` or an area code in parentheses (a plain run of digits without `+` is not a phone number: block heights and timestamps look the same) |
//! | `card` | 13 to 19 digits, plain or in groups split by single spaces or `-`, that pass the Luhn check |
//! | `government_id` | a US Social Security number `AAA-GG-SSSS` with a valid area, group and serial |
//! | `iban` | an IBAN (country, check digits, 11 to 30 characters, plain or in groups of four) that passes mod 97 |
//! | `ip_address` | a public IPv4 address (private, loopback and link-local ranges are not personal) |
//! | `street_address` | a house number, one to four capitalised words and a street suffix (`12 Elm Street`, `400 N Main St`), or `PO Box` and a number |
//! | `name` | a title and a capitalised name (`Dr. Rivera`), a labelled name (`Name: Ana Lopez`, `my name is Ana`), or a common given name followed by a capitalised surname (`Maria Gonzalez`) |
//! | `secret` | every other shape of [`crate::secret_shapes`]: keys, tokens, JWTs, seed phrases, cookies, passwords after a key name |
//!
//! What it does not catch, said wherever a result is shown ([`COVERAGE`]): names that are not in
//! the given-name list and have no title or label, addresses outside the house-number form, ids of
//! other countries, and anything in images or binary files. A pass means none of the classes above
//! was found, not that the text holds nothing personal.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The scanner's name and version, recorded with every result (a dataset manifest names it).
pub const SCANNER: &str = "citrate-pii-scan/1";

/// What a pass does and does not mean, shown with every result.
pub const COVERAGE: &str = "Checked for email addresses, phone numbers, card numbers, US Social Security numbers, IBANs, public IP addresses, street addresses, names with a title or label or a common given name, and keys, tokens and seed phrases. Not checked: other names, other address forms, other countries' ids, images and binary files.";

/// A class of private information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PiiClass {
    Email,
    Phone,
    Card,
    GovernmentId,
    Iban,
    IpAddress,
    StreetAddress,
    Name,
    Secret,
}

impl PiiClass {
    /// The stable key a dataset manifest's scan record counts under (`email`, `government-id`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn key(self) -> &'static str {
        match self {
            PiiClass::Email => "email",
            PiiClass::Phone => "phone",
            PiiClass::Card => "card",
            PiiClass::GovernmentId => "government-id",
            PiiClass::Iban => "iban",
            PiiClass::IpAddress => "ip-address",
            PiiClass::StreetAddress => "street-address",
            PiiClass::Name => "name",
            PiiClass::Secret => "secret",
        }
    }

    /// The plain words for the class.
    pub fn label(self) -> &'static str {
        match self {
            PiiClass::Email => "email address",
            PiiClass::Phone => "phone number",
            PiiClass::Card => "card number",
            PiiClass::GovernmentId => "government id",
            PiiClass::Iban => "bank account (IBAN)",
            PiiClass::IpAddress => "IP address",
            PiiClass::StreetAddress => "street address",
            PiiClass::Name => "person's name",
            PiiClass::Secret => "key, token or seed phrase",
        }
    }

    /// The text that replaces a finding of this class in [`redact`].
    pub fn placeholder(self) -> &'static str {
        match self {
            PiiClass::Email => "<email>",
            PiiClass::Phone => "<phone>",
            PiiClass::Card => "<card>",
            PiiClass::GovernmentId => "<government-id>",
            PiiClass::Iban => "<iban>",
            PiiClass::IpAddress => "<ip-address>",
            PiiClass::StreetAddress => "<street-address>",
            PiiClass::Name => "<name>",
            PiiClass::Secret => "<secret>",
        }
    }
}

/// One finding: its class and its byte span in the scanned text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub class: PiiClass,
    pub start: usize,
    pub end: usize,
}

// The dataset gate (WP-H7, `fl_dataset_gate.rs`) is the in-process caller.
#[cfg_attr(not(test), allow(dead_code))]
/// A finding located in one row of a multi-row input (a dataset version).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowFinding {
    pub row: usize,
    pub class: PiiClass,
    pub start: usize,
    pub end: usize,
}

// The dataset gate (WP-H7, `fl_dataset_gate.rs`) is the in-process caller.
#[cfg_attr(not(test), allow(dead_code))]
/// The most row references a report keeps (the counts are always complete).
pub const MAX_ROW_REFS: usize = 1000;

// The dataset gate (WP-H7, `fl_dataset_gate.rs`) is the in-process caller.
#[cfg_attr(not(test), allow(dead_code))]
/// The result of a scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scanner: String,
    /// True when nothing was found.
    pub passed: bool,
    pub rows_scanned: usize,
    /// Findings per class (complete).
    pub counts: BTreeMap<PiiClass, usize>,
    /// Where they are, first [`MAX_ROW_REFS`] only (never the matched text).
    pub findings: Vec<RowFinding>,
    pub truncated: bool,
    pub coverage: String,
}

/// Every finding in `s`, in order, with overlaps resolved (the earlier, then the longer, wins).
/// Spans start and end on ASCII bytes, so slicing `s` with them is always valid.
pub fn scan(s: &str) -> Vec<Finding> {
    let b = s.as_bytes();
    let mut all: Vec<Finding> = Vec::new();
    for (start, end, shape) in crate::secret_shapes::spans(s) {
        let class = if shape == "email" {
            PiiClass::Email
        } else {
            PiiClass::Secret
        };
        all.push(Finding { class, start, end });
    }
    card_hits(b, &mut all);
    ssn_hits(b, &mut all);
    iban_hits(b, &mut all);
    ip_hits(b, &mut all);
    phone_hits(b, &mut all);
    address_hits(b, &mut all);
    name_hits(b, &mut all);
    all.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end), f.class));
    let mut out: Vec<Finding> = Vec::new();
    for f in all {
        match out.last_mut() {
            Some(last) if f.start < last.end => {
                // Overlaps: keep the first; it grows to cover the later one.
                if f.end > last.end {
                    last.end = f.end;
                }
            }
            _ => out.push(f),
        }
    }
    out
}

// The dataset gate (WP-H7, `fl_dataset_gate.rs`) is the in-process caller.
#[cfg_attr(not(test), allow(dead_code))]
/// Scan many rows (the lines or records of a dataset version) into one report.
pub fn scan_rows<'a>(rows: impl IntoIterator<Item = &'a str>) -> ScanReport {
    let mut counts: BTreeMap<PiiClass, usize> = BTreeMap::new();
    let mut findings = Vec::new();
    let mut truncated = false;
    let mut n = 0usize;
    for (row, text) in rows.into_iter().enumerate() {
        n = row + 1;
        for f in scan(text) {
            *counts.entry(f.class).or_insert(0) += 1;
            if findings.len() < MAX_ROW_REFS {
                findings.push(RowFinding {
                    row,
                    class: f.class,
                    start: f.start,
                    end: f.end,
                });
            } else {
                truncated = true;
            }
        }
    }
    ScanReport {
        scanner: SCANNER.to_string(),
        passed: counts.is_empty(),
        rows_scanned: n,
        counts,
        findings,
        truncated,
        coverage: COVERAGE.to_string(),
    }
}

/// `s` with every finding replaced by its class placeholder. Idempotent.
pub fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at = 0;
    for f in scan(s) {
        out.push_str(&s[at..f.start]);
        out.push_str(f.class.placeholder());
        at = f.end;
    }
    out.push_str(&s[at..]);
    out
}

/// One class found in a text, for the webview (no spans: the webview counts UTF-16, not bytes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassCount {
    pub class: PiiClass,
    pub label: &'static str,
    pub count: usize,
}

/// What [`pii_scan_text`] returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextScan {
    pub scanner: &'static str,
    pub passed: bool,
    pub found: Vec<ClassCount>,
    /// The text with every finding replaced by its class placeholder.
    pub redacted: String,
    pub coverage: &'static str,
}

/// The most bytes [`pii_scan_text`] scans in one call.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

/// Scan one text (a message about to leave the device).
pub fn scan_text(text: &str) -> TextScan {
    let mut counts: BTreeMap<PiiClass, usize> = BTreeMap::new();
    for f in scan(text) {
        *counts.entry(f.class).or_insert(0) += 1;
    }
    TextScan {
        scanner: SCANNER,
        passed: counts.is_empty(),
        found: counts
            .into_iter()
            .map(|(class, count)| ClassCount {
                class,
                label: class.label(),
                count,
            })
            .collect(),
        redacted: redact(text),
        coverage: COVERAGE,
    }
}

/// **pii_scan_text**: scan a text on this device before it is sent (V6: a Hermes answer before
/// it is posted to a group). Pure; reads and sends nothing. Refuses a text over
/// [`MAX_TEXT_BYTES`].
#[tauri::command]
pub async fn pii_scan_text(text: String) -> Result<TextScan, String> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "the text is over {} KiB, the most the scanner reads at once",
            MAX_TEXT_BYTES / 1024
        ));
    }
    crate::blocking::off_main(move || Ok(scan_text(&text))).await
}

// ---- detectors -----------------------------------------------------------------------------

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// No letter, digit or `_` right before `i` (and not part of a longer number with a `.`/`-`).
fn left_edge(b: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let p = b[i - 1];
    if is_word(p) {
        return false;
    }
    // `1.2025550123` or `x-2025550123`: part of something longer.
    !(matches!(p, b'.' | b'-' | b'/' | b'+') && i >= 2 && b[i - 2].is_ascii_alphanumeric())
}

fn right_edge(b: &[u8], e: usize) -> bool {
    if e >= b.len() {
        return true;
    }
    let n = b[e];
    if is_word(n) {
        return false;
    }
    !(matches!(n, b'.' | b'-' | b'/') && e + 1 < b.len() && b[e + 1].is_ascii_alphanumeric())
}

fn luhn(digits: &[u8]) -> bool {
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        let mut v = u32::from(d - b'0');
        if i % 2 == 1 {
            v *= 2;
            if v > 9 {
                v -= 9;
            }
        }
        sum += v;
    }
    sum.is_multiple_of(10)
}

/// A run of digit groups from `i`: groups of digits split by one `sep` byte from `seps`. Returns
/// the end, the digits and the group lengths.
fn digit_groups(b: &[u8], i: usize, seps: &[u8]) -> (usize, Vec<u8>, Vec<usize>) {
    let mut j = i;
    let mut digits = Vec::new();
    let mut groups = Vec::new();
    loop {
        let s = j;
        while j < b.len() && b[j].is_ascii_digit() {
            digits.push(b[j]);
            j += 1;
        }
        if j == s {
            break;
        }
        groups.push(j - s);
        if j + 1 < b.len() && seps.contains(&b[j]) && b[j + 1].is_ascii_digit() {
            j += 1;
        } else {
            break;
        }
    }
    (j, digits, groups)
}

fn card_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() && left_edge(b, i) {
            let (e, digits, groups) = digit_groups(b, i, b" -");
            let grouped_ok =
                groups.len() == 1 || groups[..groups.len() - 1].iter().all(|g| *g == 4);
            if (13..=19).contains(&digits.len()) && grouped_ok && right_edge(b, e) && luhn(&digits)
            {
                out.push(Finding {
                    class: PiiClass::Card,
                    start: i,
                    end: e,
                });
                i = e;
                continue;
            }
            i = e.max(i + 1);
        } else {
            i += 1;
        }
    }
}

fn ssn_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i + 11 <= b.len() {
        let w = &b[i..i + 11];
        let shape = w.iter().enumerate().all(|(k, c)| {
            if k == 3 || k == 6 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
        if shape && left_edge(b, i) && right_edge(b, i + 11) {
            let area = &w[0..3];
            let ok = area != b"000"
                && area != b"666"
                && area[0] != b'9'
                && &w[4..6] != b"00"
                && &w[7..11] != b"0000";
            if ok {
                out.push(Finding {
                    class: PiiClass::GovernmentId,
                    start: i,
                    end: i + 11,
                });
                i += 11;
                continue;
            }
        }
        i += 1;
    }
}

fn iban_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i + 4 <= b.len() {
        let starts = b[i].is_ascii_uppercase()
            && b[i + 1].is_ascii_uppercase()
            && b[i + 2].is_ascii_digit()
            && b[i + 3].is_ascii_digit()
            && left_edge(b, i);
        if !starts {
            i += 1;
            continue;
        }
        // Collect upper-case alphanumerics, allowing single spaces between groups of four.
        let mut j = i;
        let mut chars: Vec<u8> = Vec::new();
        let mut since_space = 0usize;
        while j < b.len() {
            let c = b[j];
            if c.is_ascii_uppercase() || c.is_ascii_digit() {
                chars.push(c);
                since_space += 1;
                j += 1;
            } else if c == b' '
                && since_space == 4
                && j + 1 < b.len()
                && (b[j + 1].is_ascii_uppercase() || b[j + 1].is_ascii_digit())
            {
                since_space = 0;
                j += 1;
            } else {
                break;
            }
            if chars.len() > 34 {
                break;
            }
        }
        if (15..=34).contains(&chars.len()) && right_edge(b, j) && iban_mod97(&chars) {
            out.push(Finding {
                class: PiiClass::Iban,
                start: i,
                end: j,
            });
            i = j;
        } else {
            i += 1;
        }
    }
}

fn iban_mod97(chars: &[u8]) -> bool {
    let mut rem: u32 = 0;
    for c in chars[4..].iter().chain(chars[..4].iter()) {
        let v = if c.is_ascii_digit() {
            u32::from(c - b'0')
        } else {
            u32::from(c - b'A') + 10
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

fn ip_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_digit() && left_edge(b, i)) {
            i += 1;
            continue;
        }
        let (e, _, groups) = digit_groups(b, i, b".");
        if groups.len() == 4 && right_edge(b, e) && groups.iter().all(|g| *g <= 3) {
            let text = std::str::from_utf8(&b[i..e]).unwrap_or("");
            let octets: Vec<u32> = text.split('.').filter_map(|p| p.parse().ok()).collect();
            let version_like = i >= 1 && matches!(b[i - 1], b'v' | b'V');
            if octets.len() == 4
                && octets.iter().all(|o| *o <= 255)
                && !version_like
                && public_ipv4(&octets)
            {
                out.push(Finding {
                    class: PiiClass::IpAddress,
                    start: i,
                    end: e,
                });
            }
        }
        i = e.max(i + 1);
    }
}

fn public_ipv4(o: &[u32]) -> bool {
    !(o[0] == 0
        || o[0] == 10
        || o[0] == 127
        || o[0] >= 224
        || (o[0] == 169 && o[1] == 254)
        || (o[0] == 172 && (16..=31).contains(&o[1]))
        || (o[0] == 192 && o[1] == 168)
        || (o[0] == 100 && (64..=127).contains(&o[1])))
}

fn phone_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i < b.len() {
        let plus = b[i] == b'+' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        let paren = b[i] == b'(' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        if !(plus || paren || b[i].is_ascii_digit()) || !left_edge(b, i) {
            i += 1;
            continue;
        }
        let mut j = if plus || paren { i + 1 } else { i };
        let mut digits = 0usize;
        let mut groups = 0usize;
        let mut longest = 0usize;
        let mut seps_seen: Vec<u8> = Vec::new();
        loop {
            let s = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let g = j - s;
            if g == 0 {
                break;
            }
            digits += g;
            groups += 1;
            longest = longest.max(g);
            // `(555) 123-4567`: the closing parenthesis of the area code, then a separator.
            if paren && groups == 1 && j < b.len() && b[j] == b')' {
                j += 1;
                if j < b.len() && b[j] == b' ' {
                    j += 1;
                }
                continue;
            }
            if j + 1 < b.len() && matches!(b[j], b' ' | b'-' | b'.') && b[j + 1].is_ascii_digit() {
                seps_seen.push(b[j]);
                j += 1;
            } else {
                break;
            }
        }
        // Mixed separators (`1.2-3 4`) are not how a phone number is written.
        let one_style = seps_seen.windows(2).all(|w| w[0] == w[1]);
        let shaped = if plus {
            groups >= 1
        } else {
            groups >= 2 || paren
        };
        let ok = (10..=15).contains(&digits)
            && shaped
            && one_style
            && (plus || longest <= 4)
            && right_edge(b, j);
        if ok {
            out.push(Finding {
                class: PiiClass::Phone,
                start: i,
                end: j,
            });
            i = j;
        } else {
            i = j.max(i + 1);
        }
    }
}

const STREET_SUFFIXES: &[&str] = &[
    "street",
    "st",
    "avenue",
    "ave",
    "road",
    "rd",
    "boulevard",
    "blvd",
    "lane",
    "ln",
    "drive",
    "dr",
    "court",
    "ct",
    "way",
    "place",
    "pl",
    "terrace",
    "ter",
    "circle",
    "cir",
    "highway",
    "hwy",
    "parkway",
    "pkwy",
    "square",
    "sq",
    "trail",
    "trl",
];

const STREET_ABBREVS: &[&str] = &[
    "st", "ave", "rd", "blvd", "ln", "dr", "ct", "pl", "ter", "cir", "hwy", "pkwy", "sq", "trl",
];

fn word_at(b: &[u8], i: usize) -> usize {
    let mut j = i;
    while j < b.len() && (b[j].is_ascii_alphabetic() || b[j] == b'\'') {
        j += 1;
    }
    j
}

fn address_hits(b: &[u8], out: &mut Vec<Finding>) {
    let mut i = 0;
    while i < b.len() {
        // `PO Box 123` / `P.O. Box 123`.
        for lead in [&b"PO Box "[..], &b"P.O. Box "[..], &b"po box "[..]] {
            if b[i..].starts_with(lead) && left_edge(b, i) {
                let s = i + lead.len();
                let e = s + b[s..].iter().take_while(|c| c.is_ascii_digit()).count();
                if e > s && right_edge(b, e) {
                    out.push(Finding {
                        class: PiiClass::StreetAddress,
                        start: i,
                        end: e,
                    });
                }
            }
        }
        if !(b[i].is_ascii_digit() && left_edge(b, i)) {
            i += 1;
            continue;
        }
        let num_end = i + b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if num_end - i > 6 || num_end >= b.len() || b[num_end] != b' ' {
            i = num_end;
            continue;
        }
        // One to four capitalised words (or a direction letter), then a suffix.
        let mut j = num_end + 1;
        let mut words = 0usize;
        let mut found = None;
        while words <= 4 && j < b.len() && b[j].is_ascii_uppercase() {
            let e = word_at(b, j);
            let w = std::str::from_utf8(&b[j..e])
                .unwrap_or("")
                .to_ascii_lowercase();
            if words >= 1 && STREET_SUFFIXES.contains(&w.as_str()) {
                // An abbreviated suffix keeps its period (`Rd.`); a sentence's full stop is not
                // part of the address.
                let abbrev = STREET_ABBREVS.contains(&w.as_str());
                let end = if abbrev && e < b.len() && b[e] == b'.' {
                    e + 1
                } else {
                    e
                };
                if e >= b.len() || !b[e].is_ascii_alphanumeric() {
                    found = Some(end);
                }
                break;
            }
            words += 1;
            if e < b.len() && b[e] == b' ' {
                j = e + 1;
            } else {
                break;
            }
        }
        if let Some(end) = found {
            out.push(Finding {
                class: PiiClass::StreetAddress,
                start: i,
                end,
            });
            i = end;
        } else {
            i = num_end;
        }
    }
}

const TITLES: &[&str] = &["Mr", "Mrs", "Ms", "Miss", "Mx", "Dr", "Prof"];
const NAME_LABELS: &[&str] = &[
    "name:",
    "full name:",
    "first name:",
    "last name:",
    "my name is ",
    "signed by ",
    "patient:",
    "contact:",
];

/// A capitalised word (`Rivera`, `O'Neil`) at `i`: its end, or `None`.
fn cap_word(b: &[u8], i: usize) -> Option<usize> {
    if i >= b.len() || !b[i].is_ascii_uppercase() {
        return None;
    }
    let e = word_at(b, i);
    (e - i >= 2 && b[i + 1..e].iter().any(|c| c.is_ascii_lowercase())).then_some(e)
}

/// Up to two more capitalised words after the one ending at `e`.
fn more_cap_words(b: &[u8], mut e: usize) -> usize {
    for _ in 0..2 {
        if e + 1 < b.len() && b[e] == b' ' {
            if let Some(n) = cap_word(b, e + 1) {
                e = n;
                continue;
            }
        }
        break;
    }
    e
}

fn name_hits(b: &[u8], out: &mut Vec<Finding>) {
    let lower = b.to_ascii_lowercase();
    // A label, then a capitalised name.
    for label in NAME_LABELS {
        let l = label.as_bytes();
        let mut from = 0;
        while let Some(p) = find(&lower[from..], l) {
            let at = from + p;
            from = at + l.len();
            if !left_edge(b, at) {
                continue;
            }
            let mut s = at + l.len();
            while s < b.len() && b[s] == b' ' {
                s += 1;
            }
            if let Some(e) = cap_word(b, s) {
                out.push(Finding {
                    class: PiiClass::Name,
                    start: s,
                    end: more_cap_words(b, e),
                });
            }
        }
    }
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_uppercase() && left_edge(b, i)) {
            i += 1;
            continue;
        }
        let e = word_at(b, i);
        let w = std::str::from_utf8(&b[i..e]).unwrap_or("");
        // A title, then a capitalised name.
        if TITLES.contains(&w) {
            let mut s = e;
            if s < b.len() && b[s] == b'.' {
                s += 1;
            }
            if s < b.len() && b[s] == b' ' {
                if let Some(n) = cap_word(b, s + 1) {
                    out.push(Finding {
                        class: PiiClass::Name,
                        start: i,
                        end: more_cap_words(b, n),
                    });
                    i = n;
                    continue;
                }
            }
        }
        // A common given name, then a capitalised surname.
        if GIVEN_NAMES.contains(&w) && e + 1 < b.len() && b[e] == b' ' {
            if let Some(n) = cap_word(b, e + 1) {
                out.push(Finding {
                    class: PiiClass::Name,
                    start: i,
                    end: n,
                });
                i = n;
                continue;
            }
        }
        i = e.max(i + 1);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Common given names. A given name counts only
/// when a capitalised surname follows it, so a word like "Will" or "May" on its own is left alone.
pub(crate) const GIVEN_NAMES: &[&str] = &[
    "Aaron",
    "Abigail",
    "Adam",
    "Adrian",
    "Ahmed",
    "Aisha",
    "Alan",
    "Albert",
    "Alejandro",
    "Alex",
    "Alexander",
    "Alexandra",
    "Ali",
    "Alice",
    "Alicia",
    "Alyssa",
    "Amanda",
    "Amber",
    "Amelia",
    "Amy",
    "Ana",
    "Andrea",
    "Andrew",
    "Angela",
    "Anna",
    "Anne",
    "Anthony",
    "Antonio",
    "Arjun",
    "Ashley",
    "Austin",
    "Ava",
    "Barbara",
    "Benjamin",
    "Beth",
    "Betty",
    "Brandon",
    "Brenda",
    "Brian",
    "Brittany",
    "Bruce",
    "Bryan",
    "Camila",
    "Carl",
    "Carlos",
    "Carmen",
    "Carol",
    "Caroline",
    "Catherine",
    "Charles",
    "Charlotte",
    "Chen",
    "Cheryl",
    "Chloe",
    "Christian",
    "Christina",
    "Christine",
    "Christopher",
    "Cynthia",
    "Daniel",
    "Daniela",
    "David",
    "Deborah",
    "Debra",
    "Denise",
    "Dennis",
    "Diana",
    "Diego",
    "Donald",
    "Donna",
    "Dorothy",
    "Douglas",
    "Dylan",
    "Edward",
    "Elena",
    "Elijah",
    "Elizabeth",
    "Ella",
    "Emily",
    "Emma",
    "Eric",
    "Ethan",
    "Eugene",
    "Evelyn",
    "Fatima",
    "Fernando",
    "Frances",
    "Francisco",
    "Frank",
    "Gabriel",
    "Gary",
    "George",
    "Gerald",
    "Gloria",
    "Grace",
    "Gregory",
    "Hannah",
    "Harold",
    "Harper",
    "Heather",
    "Helen",
    "Henry",
    "Hiroshi",
    "Isabel",
    "Isabella",
    "Jack",
    "Jacob",
    "Jacqueline",
    "James",
    "Janet",
    "Janice",
    "Jason",
    "Javier",
    "Jean",
    "Jeffrey",
    "Jennifer",
    "Jeremy",
    "Jerry",
    "Jessica",
    "Joan",
    "John",
    "Jonathan",
    "Jordan",
    "Jorge",
    "Jose",
    "Joseph",
    "Joshua",
    "Joyce",
    "Juan",
    "Judith",
    "Julia",
    "Julie",
    "Justin",
    "Karen",
    "Katherine",
    "Kathleen",
    "Kathryn",
    "Kayla",
    "Keith",
    "Kelly",
    "Kenneth",
    "Kevin",
    "Kimberly",
    "Kyle",
    "Larry",
    "Laura",
    "Lauren",
    "Lawrence",
    "Li",
    "Liam",
    "Linda",
    "Lisa",
    "Logan",
    "Lucas",
    "Lucia",
    "Luis",
    "Madison",
    "Manuel",
    "Margaret",
    "Maria",
    "Marie",
    "Marilyn",
    "Mark",
    "Martha",
    "Mary",
    "Mason",
    "Matthew",
    "Megan",
    "Melissa",
    "Mia",
    "Michael",
    "Michelle",
    "Miguel",
    "Mohammed",
    "Muhammad",
    "Nancy",
    "Natalie",
    "Nathan",
    "Nicholas",
    "Nicole",
    "Noah",
    "Oliver",
    "Olivia",
    "Omar",
    "Pablo",
    "Pamela",
    "Patricia",
    "Patrick",
    "Paul",
    "Pedro",
    "Peter",
    "Philip",
    "Priya",
    "Rachel",
    "Rahul",
    "Ralph",
    "Raymond",
    "Rebecca",
    "Richard",
    "Robert",
    "Roger",
    "Ronald",
    "Rose",
    "Russell",
    "Ruth",
    "Ryan",
    "Samantha",
    "Samuel",
    "Sandra",
    "Sara",
    "Sarah",
    "Scott",
    "Sean",
    "Sharon",
    "Shirley",
    "Sofia",
    "Sophia",
    "Stephanie",
    "Stephen",
    "Steven",
    "Susan",
    "Teresa",
    "Terry",
    "Thomas",
    "Timothy",
    "Tyler",
    "Valentina",
    "Victoria",
    "Vincent",
    "Virginia",
    "Walter",
    "Wayne",
    "Wei",
    "William",
    "Yuki",
    "Zachary",
    "Zoe",
];

#[cfg(test)]
mod tests {
    include!("pii_scan_tests.rs");
}
