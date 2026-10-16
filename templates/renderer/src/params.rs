//! Strict validators for the five template parameters.
//!
//! Every value that reaches a template file comes out of one of these functions.
//! Each one accepts only a small ASCII alphabet that cannot terminate or escape a
//! string literal in Solidity, TypeScript, JSON or HTML (no quotes, backslashes,
//! backticks, `$`, `<`, `>`, `{`, `}`, control characters or non-ASCII), so the
//! renderer never needs context-specific escaping. Values are normalized (the
//! owner is EIP-55 checksummed) before substitution.

use sha3::{Digest, Keccak256};

/// Longest accepted collection name, in characters.
pub const NAME_MAX: usize = 48;
/// Longest accepted ticker symbol.
pub const SYMBOL_MAX: usize = 11;
/// Highest accepted price, in wei: 10^30 (one trillion SALT).
pub const PRICE_MAX_DIGITS: usize = 31;

/// Identifiers the templates import or declare. A collection name whose derived
/// contract identifier equals one of these would shadow it, so it is refused.
/// `tests/render.rs` checks that every `import {..}` symbol in every template is
/// listed here.
pub const RESERVED_IDENTIFIERS: &[&str] = &[
    // OpenZeppelin
    "ERC20",
    "ERC20Permit",
    "ERC20Votes",
    "ERC721",
    "ERC1155",
    "ERC1155Supply",
    "Ownable",
    "ReentrancyGuard",
    "Strings",
    "Governor",
    "GovernorSettings",
    "GovernorCountingSimple",
    "GovernorVotes",
    "GovernorVotesQuorumFraction",
    "IVotes",
    "IGovernor",
    "Nonces",
    "Votes",
    "IERC721Receiver",
    "IERC1155Receiver",
    "IERC20",
    "IERC20Errors",
    // Solady
    "LibString",
    "SafeTransferLib",
    // forge-std and the shared cheatcode interface
    "Test",
    "Vm",
    "StdInvariant",
    "StdStorage",
    "stdStorage",
    "FuzzSelector",
    "IHevm",
    "HEVM",
    // Fixed helper declarations inside the templates
    "ReentrantMinter",
    "ReentrantBuyer",
];

/// A validation failure, naming the parameter and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamError {
    pub param: &'static str,
    pub reason: String,
}

impl std::fmt::Display for ParamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.param, self.reason)
    }
}

impl std::error::Error for ParamError {}

fn err(param: &'static str, reason: impl Into<String>) -> ParamError {
    ParamError {
        param,
        reason: reason.into(),
    }
}

/// A collection name: ASCII letters and digits in words joined by a single space
/// or hyphen, starting with a letter, at most [`NAME_MAX`] characters.
pub fn validate_name(raw: &str) -> Result<String, ParamError> {
    if raw.is_empty() {
        return Err(err("name", "is empty"));
    }
    if raw.len() > NAME_MAX {
        return Err(err("name", format!("is longer than {NAME_MAX} characters")));
    }
    if !raw
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-')
    {
        return Err(err(
            "name",
            "may only use letters A-Z, digits, single spaces and hyphens",
        ));
    }
    if !raw.as_bytes()[0].is_ascii_alphabetic() {
        return Err(err("name", "must start with a letter"));
    }
    let words: Vec<&str> = raw.split([' ', '-']).collect();
    if words.iter().any(|w| w.is_empty()) {
        return Err(err(
            "name",
            "has a leading, trailing or doubled space or hyphen",
        ));
    }
    Ok(raw.to_string())
}

/// The Solidity contract identifier for a collection name: each word's first
/// letter upper-cased, separators dropped (`"lemon-drops 2"` -> `LemonDrops2`).
pub fn contract_identifier(name: &str) -> Result<String, ParamError> {
    let name = validate_name(name)?;
    let mut ident = String::with_capacity(name.len());
    for word in name.split([' ', '-']) {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            ident.push(first.to_ascii_uppercase());
            ident.extend(chars);
        }
    }
    if RESERVED_IDENTIFIERS.contains(&ident.as_str()) {
        return Err(err(
            "name",
            format!("becomes the contract name `{ident}`, which a template already uses"),
        ));
    }
    Ok(ident)
}

/// A ticker symbol: an upper-case letter then up to ten upper-case letters or digits.
pub fn validate_symbol(raw: &str) -> Result<String, ParamError> {
    let ok = !raw.is_empty()
        && raw.len() <= SYMBOL_MAX
        && raw.as_bytes()[0].is_ascii_uppercase()
        && raw
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if ok {
        Ok(raw.to_string())
    } else {
        Err(err(
            "symbol",
            format!("must be 1-{SYMBOL_MAX} characters A-Z or 0-9, starting with a letter"),
        ))
    }
}

/// True for a canonical unsigned decimal: digits only, no leading zero unless "0".
fn canonical_decimal(raw: &str) -> bool {
    !raw.is_empty()
        && raw.bytes().all(|b| b.is_ascii_digit())
        && (raw == "0" || !raw.starts_with('0'))
}

/// A supply: a canonical decimal integer in `[min, max]`.
pub fn validate_supply(raw: &str, min: u64, max: u64) -> Result<u64, ParamError> {
    if !canonical_decimal(raw) {
        return Err(err(
            "supply",
            "must be a whole number written with digits only",
        ));
    }
    let n: u64 = raw.parse().map_err(|_| err("supply", "is too large"))?;
    if n < min || n > max {
        return Err(err("supply", format!("must be between {min} and {max}")));
    }
    Ok(n)
}

/// A price in wei: a canonical decimal integer no larger than 10^30.
pub fn validate_price(raw: &str) -> Result<String, ParamError> {
    if !canonical_decimal(raw) {
        return Err(err(
            "price",
            "must be a whole number of wei written with digits only",
        ));
    }
    let ceiling = format!("1{}", "0".repeat(PRICE_MAX_DIGITS - 1));
    let over =
        raw.len() > PRICE_MAX_DIGITS || (raw.len() == PRICE_MAX_DIGITS && raw > ceiling.as_str());
    if over {
        return Err(err("price", "is above 10^30 wei"));
    }
    Ok(raw.to_string())
}

/// EIP-55 checksum of 40 lower-case hex digits (no `0x`). The input must already
/// be 40 lower-case hex characters; [`validate_owner`] guarantees that.
pub fn checksum_address(lower_hex: &str) -> String {
    let hash = Keccak256::digest(lower_hex.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, c) in lower_hex.chars().enumerate() {
        let byte = hash[i / 2];
        let nibble = if i % 2 == 0 { byte >> 4 } else { byte & 0x0f };
        if c.is_ascii_alphabetic() && nibble >= 8 {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// An owner address: `0x` + 40 hex digits, not the zero address. All-lower or
/// all-upper input is accepted; mixed case must be a valid EIP-55 checksum. The
/// result is always the EIP-55 form (solc rejects non-checksummed literals).
pub fn validate_owner(raw: &str) -> Result<String, ParamError> {
    let Some(hex) = raw.strip_prefix("0x") else {
        return Err(err("owner", "must start with 0x"));
    };
    if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err("owner", "must be 0x followed by 40 hex digits"));
    }
    let lower = hex.to_ascii_lowercase();
    if lower.bytes().all(|b| b == b'0') {
        return Err(err("owner", "must not be the zero address"));
    }
    let checksummed = checksum_address(&lower);
    let has_lower = hex.bytes().any(|b| b.is_ascii_lowercase());
    let has_upper = hex.bytes().any(|b| b.is_ascii_uppercase());
    if has_lower && has_upper && checksummed[2..] != *hex {
        return Err(err(
            "owner",
            "has mixed case that is not a valid EIP-55 checksum",
        ));
    }
    Ok(checksummed)
}

/// Defense in depth: true when a rendered value uses only characters that are
/// inert in every template context. Every validator above already guarantees it.
pub fn is_inert(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b' ' || b == b'-' || b == b'_')
}
