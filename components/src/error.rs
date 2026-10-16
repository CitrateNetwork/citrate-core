//! One error type for the whole updater. Every refusal is a variant, so callers (and tests)
//! can tell a missing key from a bad signature from a hash mismatch.
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentError {
    /// The production key slot is empty (the @rule8 key ceremony has not happened).
    KeyNotConfigured,
    /// The configured key does not parse, or is a key the updater must not trust.
    BadKey(String),
    /// The signature does not decode, is the wrong algorithm, or does not verify.
    SignatureInvalid(String),
    /// A valid signature made for another purpose (its trusted comment names another domain).
    WrongTrustedComment,
    /// The manifest is signed but a field is out of bounds.
    ManifestMalformed(String),
    /// The manifest is older than the one already recorded.
    ManifestRollback {
        seen: u64,
        got: u64,
    },
    ManifestExpired,
    ManifestNotYetValid,
    UnknownComponent(String),
    NoArtifactForPlatform {
        component: String,
        platform: String,
    },
    /// The download was larger than the manifest allows.
    ArtifactTooLarge {
        limit: u64,
    },
    SizeMismatch {
        expected: u64,
        got: u64,
    },
    HashMismatch,
    Fetch(String),
    UnsupportedFormat(String),
    UnsafeArchiveEntry(String),
    HealthCheck(String),
    NoPrevious(String),
    /// The state file exists but does not parse. Never silently reset: the sequence in it is
    /// what stops a replayed older manifest.
    State(String),
    Io(String),
}

impl fmt::Display for ComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ComponentError::*;
        match self {
            KeyNotConfigured => write!(
                f,
                "component updates are off: the component signing key is not configured yet (pending the key ceremony)"
            ),
            BadKey(m) => write!(f, "the component signing key is not usable: {m}"),
            SignatureInvalid(m) => write!(f, "signature check failed: {m}"),
            WrongTrustedComment => write!(f, "the signature was made for a different purpose"),
            ManifestMalformed(m) => write!(f, "the component manifest is not valid: {m}"),
            ManifestRollback { seen, got } => write!(
                f,
                "the component manifest is older than the one already seen (sequence {got} < {seen})"
            ),
            ManifestExpired => write!(f, "the component manifest has expired"),
            ManifestNotYetValid => write!(f, "the component manifest is not valid yet (check the clock)"),
            UnknownComponent(n) => write!(f, "the manifest has no component named {n}"),
            NoArtifactForPlatform { component, platform } => {
                write!(f, "{component} has no download for {platform}")
            }
            ArtifactTooLarge { limit } => write!(f, "the download is larger than {limit} bytes"),
            SizeMismatch { expected, got } => {
                write!(f, "the download is {got} bytes, the manifest says {expected}")
            }
            HashMismatch => write!(f, "the download does not match the manifest hash"),
            Fetch(m) => write!(f, "download failed: {m}"),
            UnsupportedFormat(m) => write!(f, "unsupported archive format: {m}"),
            UnsafeArchiveEntry(m) => write!(f, "the archive has an unsafe entry: {m}"),
            HealthCheck(m) => write!(f, "the new version failed its check: {m}"),
            NoPrevious(n) => write!(f, "{n} has no previous version to roll back to"),
            State(m) => write!(f, "the component state file is not readable: {m}"),
            Io(m) => write!(f, "file error: {m}"),
        }
    }
}

impl std::error::Error for ComponentError {}

pub(crate) fn io(e: std::io::Error) -> ComponentError {
    ComponentError::Io(e.to_string())
}
