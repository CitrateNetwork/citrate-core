//! Secret strings (API keys, OAuth tokens) held in one type end to end.
//!
//! [`SecretString`] deserializes straight from a JSON string, so it can be a `#[tauri::command]`
//! argument or a field of a provider response. The string serde builds is moved into a wiping
//! buffer, not copied. It has no `Serialize`: a record that must store the value (a sealed vault or
//! keyring blob) opts in per field with [`serialize_exposed`]. `Debug` never prints it.
//!
//! Boundary note: for a command argument, Tauri parses the IPC payload into its own JSON value
//! before serde hands us the string; that copy belongs to Tauri and is not wiped here.

use zeroize::Zeroizing;

/// A secret string: wiped on drop, redacted in `Debug`, never serialized by default.
#[derive(Clone, Default)]
pub struct SecretString(Zeroizing<String>);

impl zeroize::ZeroizeOnDrop for SecretString {}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = if self.0.is_empty() {
            "empty"
        } else {
            "redacted"
        };
        f.debug_tuple("SecretString").field(&state).finish()
    }
}

impl<'de> serde::Deserialize<'de> for SecretString {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(SecretString::new)
    }
}

impl SecretString {
    /// Take ownership of `s` (moved, not copied).
    pub fn new(s: String) -> Self {
        SecretString(Zeroizing::new(s))
    }

    /// The secret, for the one call that needs it (a header, a request body, a sealed record).
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Move the secret into a wiping buffer for an API that takes one.
    pub fn into_zeroizing(mut self) -> Zeroizing<String> {
        std::mem::take(&mut self.0)
    }

    /// Whether the secret is empty or whitespace only.
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        SecretString::new(s)
    }
}

/// `serialize_with` for a sealed record's [`SecretString`] field (vault and keyring blobs only).
pub fn serialize_exposed<S: serde::Serializer>(
    s: &SecretString,
    ser: S,
) -> Result<S::Ok, S::Error> {
    ser.serialize_str(s.expose())
}

/// `serialize_with` for an optional [`SecretString`] field of a sealed record.
pub fn serialize_exposed_opt<S: serde::Serializer>(
    s: &Option<SecretString>,
    ser: S,
) -> Result<S::Ok, S::Error> {
    match s {
        Some(v) => ser.serialize_some(v.expose()),
        None => ser.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_secret() {
        let s = SecretString::new("sk-live-0123456789abcdef".into());
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("sk-live"), "secret in Debug: {dbg}");
        let opt = Some(s.clone());
        assert!(!format!("{opt:?}").contains("0123456789"));
    }

    #[test]
    fn wipes_on_drop_and_moves_into_a_wiping_buffer() {
        fn wipes_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
        let s = SecretString::new("k".into());
        wipes_on_drop(&s);
        let z: Zeroizing<String> = s.into_zeroizing();
        assert_eq!(z.as_str(), "k");
    }

    #[test]
    fn deserializes_from_a_json_string_and_round_trips_through_a_sealed_record() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Sealed {
            #[serde(serialize_with = "serialize_exposed")]
            key: SecretString,
            #[serde(default, serialize_with = "serialize_exposed_opt")]
            refresh: Option<SecretString>,
        }
        let s: Sealed = serde_json::from_str(r#"{"key":"abc","refresh":null}"#).unwrap();
        assert_eq!(s.key.expose(), "abc");
        assert!(s.refresh.is_none());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"key":"abc","refresh":null}"#);
        assert!(SecretString::new("  ".into()).is_blank());
        assert!(
            serde_json::from_str::<SecretString>("42").is_err(),
            "only a string is a secret"
        );
    }
}
