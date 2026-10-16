//! HUP-S2.3 — hardened Sign-In with Ethereum (EIP-4361) for the B-1 web-signing budget.
//!
//! ADR-2026-09-30-rule3-budgetable-signatures, D2 checks 6 to 18. This module is PURE: it parses
//! and checks message text and never touches a key. A message that fails any check here is not
//! rejected outright by the budgeted path: it falls through to an ordinary HIC-1 ceremony, so a
//! legitimate but unusual sign-in still works with one click (see `ceremony::request_siwe_budgeted`).
//!
//! Strictness, in one place:
//! - The text must match the EIP-4361 ABNF layout exactly: `\n` line endings only, no leading or
//!   trailing bytes, the fields in ABNF order, no unknown or duplicate field, at most
//!   [`MAX_MESSAGE_BYTES`].
//! - [`SiweFields::to_message`] re-serializes the parse, and [`parse_strict`] requires byte
//!   equality with the input, so the parser and the signer agree on what is signed.
//! - The provenance checks (D2 1 to 5) are NOT here: the attested top-frame origin comes from
//!   core, never from the page, the model or the sidecar. This module only compares against it.

use sha3::{Digest, Keccak256};

/// D2 #6: the largest message the budget path will consider.
pub const MAX_MESSAGE_BYTES: usize = 2048;
/// D2 #11: the chain allowlist is exactly {40204} under the accepted ADR.
pub const CHAIN_ALLOWLIST: [u64; 1] = [40204];
/// D2 #12: the stricter nonce floor (EIP-4361 allows 8).
pub const MIN_NONCE_LEN: usize = 16;
/// D2 #13: how old `Issued At` may be.
pub const ISSUED_AT_MAX_AGE_MS: u64 = 5 * 60 * 1000;
/// D2 #13 and #15: the clock-skew tolerance for times in the future.
pub const CLOCK_SKEW_MS: u64 = 60 * 1000;
/// D2 #14: the longest life a budgeted message may claim.
pub const MAX_MESSAGE_LIFETIME_MS: u64 = 24 * 60 * 60 * 1000;
/// D2 #16: the longest statement.
pub const MAX_STATEMENT_CHARS: usize = 280;
/// D2 #18: the longest request id.
pub const MAX_REQUEST_ID_CHARS: usize = 128;

const HEADER_SUFFIX: &str = " wants you to sign in with your Ethereum account:";

/// The fields of an EIP-4361 message, kept as the exact strings that appeared in the text so a
/// re-serialization reproduces it byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiweFields {
    /// Optional `scheme://` prefix on the header line.
    pub scheme: Option<String>,
    /// The `domain` (RFC 3986 authority) the site claims.
    pub domain: String,
    /// The signing address as written.
    pub address: String,
    /// The optional human-readable statement.
    pub statement: Option<String>,
    pub uri: String,
    pub version: String,
    /// `Chain ID`, as written (decimal digits, no leading zero).
    pub chain_id: String,
    pub nonce: String,
    pub issued_at: String,
    pub expiration_time: Option<String>,
    pub not_before: Option<String>,
    pub request_id: Option<String>,
    /// `Resources` entries. Any entry makes the message ineligible for a budget (D2 #17).
    pub resources: Vec<String>,
}

impl SiweFields {
    /// Serialize per the EIP-4361 ABNF. With no statement the layout keeps the blank line, so
    /// the address is followed by three line feeds before `URI: `.
    pub fn to_message(&self) -> String {
        let mut out = String::new();
        if let Some(s) = &self.scheme {
            out.push_str(s);
            out.push_str("://");
        }
        out.push_str(&self.domain);
        out.push_str(HEADER_SUFFIX);
        out.push('\n');
        out.push_str(&self.address);
        out.push_str("\n\n");
        if let Some(st) = &self.statement {
            out.push_str(st);
            out.push('\n');
        }
        out.push('\n');
        out.push_str("URI: ");
        out.push_str(&self.uri);
        out.push_str("\nVersion: ");
        out.push_str(&self.version);
        out.push_str("\nChain ID: ");
        out.push_str(&self.chain_id);
        out.push_str("\nNonce: ");
        out.push_str(&self.nonce);
        out.push_str("\nIssued At: ");
        out.push_str(&self.issued_at);
        if let Some(v) = &self.expiration_time {
            out.push_str("\nExpiration Time: ");
            out.push_str(v);
        }
        if let Some(v) = &self.not_before {
            out.push_str("\nNot Before: ");
            out.push_str(v);
        }
        if let Some(v) = &self.request_id {
            out.push_str("\nRequest ID: ");
            out.push_str(v);
        }
        if !self.resources.is_empty() {
            out.push_str("\nResources:");
            for r in &self.resources {
                out.push_str("\n- ");
                out.push_str(r);
            }
        }
        out
    }
}

/// Why a message is not eligible for a budget. Every variant is a plain-language reason shown on
/// the HIC-1 card the request falls through to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiweReject {
    Malformed,
    TooLarge,
    DomainMismatch,
    UriMismatch,
    AddressMismatch,
    BadVersion,
    ChainNotAllowed,
    WeakNonce,
    IssuedAtOutOfRange,
    ExpirationMissing,
    ExpirationOutOfRange,
    NotBeforeOutOfRange,
    BadStatement,
    HasResources,
    BadRequestId,
}

impl std::fmt::Display for SiweReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SiweReject::Malformed => "the message is not a well-formed Sign-In with Ethereum message",
            SiweReject::TooLarge => "the sign-in message is longer than 2048 bytes",
            SiweReject::DomainMismatch => "the sign-in domain does not match the page you are on",
            SiweReject::UriMismatch => "the sign-in URI does not match the page you are on",
            SiweReject::AddressMismatch => {
                "the sign-in address is not your wallet address in checksummed form"
            }
            SiweReject::BadVersion => "the sign-in message version is not 1",
            SiweReject::ChainNotAllowed => "the sign-in message is not for Citrate (chain 40204)",
            SiweReject::WeakNonce => "the sign-in nonce is shorter than 16 letters and digits",
            SiweReject::IssuedAtOutOfRange => "the sign-in message is too old or dated in the future",
            SiweReject::ExpirationMissing => "the sign-in message has no expiration time",
            SiweReject::ExpirationOutOfRange => {
                "the sign-in message is expired or valid for more than 24 hours"
            }
            SiweReject::NotBeforeOutOfRange => "the sign-in message is not valid yet",
            SiweReject::BadStatement => {
                "the sign-in statement is too long or contains hidden control characters"
            }
            SiweReject::HasResources => {
                "the sign-in message asks for extra permissions (resources), which a budget never grants"
            }
            SiweReject::BadRequestId => "the sign-in request id is too long or not printable",
        };
        f.write_str(s)
    }
}

/// What the checks compare against. `attested_origin` must come from core's own attestation of
/// the top frame (D2 #1), never from the request.
pub struct SiweCheckContext<'a> {
    /// The attested top-frame origin, `https://host[:port]`.
    pub attested_origin: &'a str,
    /// The member's active wallet address (any case).
    pub wallet_address: &'a str,
    /// Wall clock, epoch ms.
    pub now_ms: u64,
}

/// A message that passed every D2 #6-18 check, with the fields the budget store and the decision
/// record need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetableSiwe {
    pub nonce: String,
    pub issued_at_ms: u64,
    pub expiration_ms: u64,
    pub statement: Option<String>,
    pub request_id: Option<String>,
}

/// Strict EIP-4361 parse (D2 #6). Returns the fields only if the input is exactly the
/// re-serialization of those fields.
pub fn parse_strict(text: &str) -> Result<SiweFields, SiweReject> {
    if text.len() > MAX_MESSAGE_BYTES {
        return Err(SiweReject::TooLarge);
    }
    if text.contains('\r') {
        return Err(SiweReject::Malformed);
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0usize;
    let next = |i: &mut usize| -> Result<&str, SiweReject> {
        let l = lines.get(*i).copied().ok_or(SiweReject::Malformed)?;
        *i += 1;
        Ok(l)
    };

    // Header: [scheme "://"] domain " wants you to sign in with your Ethereum account:"
    let header = next(&mut i)?;
    let authority = header
        .strip_suffix(HEADER_SUFFIX)
        .ok_or(SiweReject::Malformed)?;
    let (scheme, domain) = match authority.split_once("://") {
        Some((s, d)) => (Some(s.to_string()), d.to_string()),
        None => (None, authority.to_string()),
    };
    if let Some(s) = &scheme {
        let ok = !s.is_empty()
            && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
        if !ok {
            return Err(SiweReject::Malformed);
        }
    }
    if !is_token(&domain) {
        return Err(SiweReject::Malformed);
    }

    // Address line: 0x + 40 hex (case is checked against EIP-55 later, D2 #9).
    let address = next(&mut i)?.to_string();
    if !is_hex_address(&address) {
        return Err(SiweReject::Malformed);
    }
    if !next(&mut i)?.is_empty() {
        return Err(SiweReject::Malformed);
    }
    // [ statement LF ] LF
    let l = next(&mut i)?;
    let statement = if l.is_empty() {
        None
    } else {
        if !next(&mut i)?.is_empty() {
            return Err(SiweReject::Malformed);
        }
        Some(l.to_string())
    };

    let tagged = |i: &mut usize, tag: &str| -> Result<String, SiweReject> {
        let l = lines.get(*i).copied().ok_or(SiweReject::Malformed)?;
        let v = l.strip_prefix(tag).ok_or(SiweReject::Malformed)?;
        if !is_token(v) {
            return Err(SiweReject::Malformed);
        }
        *i += 1;
        Ok(v.to_string())
    };
    let optional = |i: &mut usize, tag: &str| -> Result<Option<String>, SiweReject> {
        match lines.get(*i) {
            Some(l) if l.starts_with(tag) => tagged(i, tag).map(Some),
            _ => Ok(None),
        }
    };

    let uri = tagged(&mut i, "URI: ")?;
    let version = tagged(&mut i, "Version: ")?;
    if !version.chars().all(|c| c.is_ascii_digit()) {
        return Err(SiweReject::Malformed);
    }
    let chain_id = tagged(&mut i, "Chain ID: ")?;
    if !chain_id.chars().all(|c| c.is_ascii_digit())
        || (chain_id.len() > 1 && chain_id.starts_with('0'))
        || chain_id.parse::<u64>().is_err()
    {
        return Err(SiweReject::Malformed);
    }
    let nonce = tagged(&mut i, "Nonce: ")?;
    let issued_at = tagged(&mut i, "Issued At: ")?;
    if parse_rfc3339_ms(&issued_at).is_none() {
        return Err(SiweReject::Malformed);
    }
    let expiration_time = optional(&mut i, "Expiration Time: ")?;
    let not_before = optional(&mut i, "Not Before: ")?;
    for t in [&expiration_time, &not_before].into_iter().flatten() {
        if parse_rfc3339_ms(t).is_none() {
            return Err(SiweReject::Malformed);
        }
    }
    // Request ID may contain spaces in some encoders; it is a pchar string in the ABNF, so no
    // whitespace is accepted here either.
    let request_id = match lines.get(i) {
        Some(l) if l.starts_with("Request ID: ") => {
            let v = &l["Request ID: ".len()..];
            if v.is_empty() || v.chars().any(|c| c.is_whitespace()) {
                return Err(SiweReject::Malformed);
            }
            i += 1;
            Some(v.to_string())
        }
        _ => None,
    };
    let mut resources = Vec::new();
    if lines.get(i) == Some(&"Resources:") {
        i += 1;
        while let Some(l) = lines.get(i) {
            let r = l.strip_prefix("- ").ok_or(SiweReject::Malformed)?;
            if !is_token(r) {
                return Err(SiweReject::Malformed);
            }
            resources.push(r.to_string());
            i += 1;
        }
        if resources.is_empty() {
            return Err(SiweReject::Malformed);
        }
    }
    if i != lines.len() {
        return Err(SiweReject::Malformed);
    }
    let fields = SiweFields {
        scheme,
        domain,
        address,
        statement,
        uri,
        version,
        chain_id,
        nonce,
        issued_at,
        expiration_time,
        not_before,
        request_id,
        resources,
    };
    // Parser and signer agree on the bytes (D2 #6).
    if fields.to_message() != text {
        return Err(SiweReject::Malformed);
    }
    Ok(fields)
}

/// Run every D2 #6-18 check against the attested origin, the member's wallet and the clock.
pub fn check_budgetable(
    text: &str,
    ctx: &SiweCheckContext<'_>,
) -> Result<BudgetableSiwe, SiweReject> {
    let f = parse_strict(text)?;
    let attested = url::Url::parse(ctx.attested_origin).map_err(|_| SiweReject::DomainMismatch)?;
    let attested_origin = attested.origin();

    // D2 #7 domain binding (IDNA and case normalized by the URL parser).
    if let Some(s) = &f.scheme {
        if s != "https" {
            return Err(SiweReject::DomainMismatch);
        }
    }
    let as_url = url::Url::parse(&format!("https://{}/", f.domain))
        .map_err(|_| SiweReject::DomainMismatch)?;
    if !as_url.username().is_empty()
        || as_url.password().is_some()
        || as_url.path() != "/"
        || as_url.query().is_some()
        || as_url.fragment().is_some()
        || f.domain.contains('/')
        || as_url.origin() != attested_origin
    {
        return Err(SiweReject::DomainMismatch);
    }

    // D2 #8 URI binding.
    let uri = url::Url::parse(&f.uri).map_err(|_| SiweReject::UriMismatch)?;
    if uri.scheme() != "https" || uri.origin() != attested_origin {
        return Err(SiweReject::UriMismatch);
    }

    // D2 #9 address.
    if !is_eip55(&f.address) || !f.address.eq_ignore_ascii_case(ctx.wallet_address) {
        return Err(SiweReject::AddressMismatch);
    }
    // D2 #10 version.
    if f.version != "1" {
        return Err(SiweReject::BadVersion);
    }
    // D2 #11 chain.
    let chain: u64 = f.chain_id.parse().map_err(|_| SiweReject::Malformed)?;
    if !CHAIN_ALLOWLIST.contains(&chain) {
        return Err(SiweReject::ChainNotAllowed);
    }
    // D2 #12 nonce floor (the ledger check is the budget store's).
    if f.nonce.chars().count() < MIN_NONCE_LEN
        || !f.nonce.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(SiweReject::WeakNonce);
    }
    // D2 #13 Issued At.
    let now = ctx.now_ms as i64;
    let iat = parse_rfc3339_ms(&f.issued_at).ok_or(SiweReject::Malformed)?;
    if iat < now - ISSUED_AT_MAX_AGE_MS as i64 || iat > now + CLOCK_SKEW_MS as i64 {
        return Err(SiweReject::IssuedAtOutOfRange);
    }
    // D2 #14 Expiration Time.
    let exp_s = f
        .expiration_time
        .as_deref()
        .ok_or(SiweReject::ExpirationMissing)?;
    let exp = parse_rfc3339_ms(exp_s).ok_or(SiweReject::Malformed)?;
    if exp <= now || exp > iat + MAX_MESSAGE_LIFETIME_MS as i64 {
        return Err(SiweReject::ExpirationOutOfRange);
    }
    // D2 #15 Not Before.
    if let Some(nb) = &f.not_before {
        let nbf = parse_rfc3339_ms(nb).ok_or(SiweReject::Malformed)?;
        if nbf > now + CLOCK_SKEW_MS as i64 {
            return Err(SiweReject::NotBeforeOutOfRange);
        }
    }
    // D2 #16 statement.
    if let Some(st) = &f.statement {
        if st.chars().count() > MAX_STATEMENT_CHARS || !st.chars().all(is_display_safe) {
            return Err(SiweReject::BadStatement);
        }
    }
    // D2 #17 no Resources, ever.
    if !f.resources.is_empty() {
        return Err(SiweReject::HasResources);
    }
    // D2 #18 Request ID.
    if let Some(r) = &f.request_id {
        if r.chars().count() > MAX_REQUEST_ID_CHARS || !r.chars().all(is_display_safe) {
            return Err(SiweReject::BadRequestId);
        }
    }
    Ok(BudgetableSiwe {
        nonce: f.nonce,
        issued_at_ms: iat.max(0) as u64,
        expiration_ms: exp.max(0) as u64,
        statement: f.statement,
        request_id: f.request_id,
    })
}

/// D2 #4: normalize an origin the member wants to allowlist, or refuse it. Only an exact `https`
/// scheme + DNS host (+ optional port) qualifies. Loopback, `localhost`, IP literals, single-label
/// hosts, `http`, `file:`, custom schemes, userinfo, paths, queries and fragments are refused.
pub fn normalize_allowlist_origin(input: &str) -> Result<String, &'static str> {
    let u = url::Url::parse(input.trim()).map_err(|_| "not a valid URL")?;
    if u.scheme() != "https" {
        return Err("only https origins can have a sign-in budget");
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("an origin cannot carry a user name or password");
    }
    if u.path() != "/" && !u.path().is_empty() {
        return Err("enter the site origin only, without a path");
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err("enter the site origin only, without a query or fragment");
    }
    let host = match u.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(_) => return Err("IP addresses cannot have a sign-in budget"),
        None => return Err("the origin has no host"),
    };
    if host == "localhost" || host.ends_with(".localhost") {
        return Err("local addresses cannot have a sign-in budget");
    }
    if !host.contains('.') || host.ends_with('.') {
        return Err("the host must be a full domain name");
    }
    Ok(match u.port() {
        Some(p) => format!("https://{host}:{p}"),
        None => format!("https://{host}"),
    })
}

/// EIP-55 mixed-case checksum, validated exactly (all-lower and all-upper forms fail unless the
/// checksum happens to produce them).
pub fn is_eip55(addr: &str) -> bool {
    if !is_hex_address(addr) {
        return false;
    }
    let body = &addr[2..];
    let lower = body.to_ascii_lowercase();
    let hash = Keccak256::digest(lower.as_bytes());
    body.chars().enumerate().all(|(i, c)| {
        if !c.is_ascii_alphabetic() {
            return true;
        }
        let byte = hash[i / 2];
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0f };
        if nibble >= 8 {
            c.is_ascii_uppercase()
        } else {
            c.is_ascii_lowercase()
        }
    })
}

/// The EIP-55 checksummed form of a hex address in any case; `None` if it is not `0x` + 40 hex.
pub fn to_eip55(addr: &str) -> Option<String> {
    if !is_hex_address(addr) {
        return None;
    }
    let lower = addr[2..].to_ascii_lowercase();
    let hash = Keccak256::digest(lower.as_bytes());
    let body: String = lower
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let byte = hash[i / 2];
            let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0f };
            if c.is_ascii_alphabetic() && nibble >= 8 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    Some(format!("0x{body}"))
}

fn is_hex_address(a: &str) -> bool {
    a.len() == 42 && a.starts_with("0x") && a[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// A non-empty value with no whitespace or control characters.
fn is_token(v: &str) -> bool {
    !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Printable and free of the bidirectional-override and invisible-direction code points a
/// statement could use to make the card read differently from the bytes.
fn is_display_safe(c: char) -> bool {
    !c.is_control()
        && !matches!(
            c,
            '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// Strict RFC 3339 date-time to epoch milliseconds: `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`.
/// A `T` separator and an explicit offset are required. Fractions are truncated to ms.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = s.get(r)?;
        if !part.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    if b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b't')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d, h, mi, se) = (
        num(0..4)?,
        num(5..7)?,
        num(8..10)?,
        num(11..13)?,
        num(14..16)?,
        num(17..19)?,
    );
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || se > 59
    {
        return None;
    }
    let mut idx = 19;
    let mut ms = 0i64;
    if b.get(idx) == Some(&b'.') {
        idx += 1;
        let start = idx;
        while idx < b.len() && b[idx].is_ascii_digit() {
            idx += 1;
        }
        let digits = idx - start;
        if digits == 0 || digits > 9 {
            return None;
        }
        let frac = &s[start..idx];
        let first3: String = frac.chars().chain("000".chars()).take(3).collect();
        ms = first3.parse().ok()?;
    }
    let offset_min = match b.get(idx) {
        Some(b'Z') | Some(b'z') if idx + 1 == b.len() => 0,
        Some(&sign @ (b'+' | b'-')) if idx + 6 == b.len() && b[idx + 3] == b':' => {
            let oh = num(idx + 1..idx + 3)?;
            let om = num(idx + 4..idx + 6)?;
            if oh > 23 || om > 59 {
                return None;
            }
            let m = oh * 60 + om;
            if sign == b'+' {
                m
            } else {
                -m
            }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    let secs = days * 86_400 + h * 3600 + mi * 60 + se - offset_min * 60;
    Some(secs * 1000 + ms)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    include!("siwe_tests.rs");
}
