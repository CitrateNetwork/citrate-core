//! Platform keys used in manifests and the bundle.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "macos-arm64")]
    MacosArm64,
    #[serde(rename = "macos-x64")]
    MacosX64,
    #[serde(rename = "linux-x64")]
    LinuxX64,
    #[serde(rename = "linux-arm64")]
    LinuxArm64,
    #[serde(rename = "windows-x64")]
    WindowsX64,
    /// Platform-independent (source libraries).
    #[serde(rename = "any")]
    Any,
}

impl Platform {
    pub const ALL: [Platform; 6] = [
        Platform::MacosArm64,
        Platform::MacosX64,
        Platform::LinuxX64,
        Platform::LinuxArm64,
        Platform::WindowsX64,
        Platform::Any,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Platform::MacosArm64 => "macos-arm64",
            Platform::MacosX64 => "macos-x64",
            Platform::LinuxX64 => "linux-x64",
            Platform::LinuxArm64 => "linux-arm64",
            Platform::WindowsX64 => "windows-x64",
            Platform::Any => "any",
        }
    }

    pub fn parse(s: &str) -> Option<Platform> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// The platform this binary was built for, if it is one the bundle covers.
    pub fn current() -> Option<Platform> {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Platform::MacosArm64)
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            Some(Platform::MacosX64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Some(Platform::LinuxX64)
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            Some(Platform::LinuxArm64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Platform::WindowsX64)
        } else {
            None
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::Platform;

    #[test]
    fn round_trips() {
        for p in Platform::ALL {
            assert_eq!(Platform::parse(p.as_str()), Some(p));
            let j = serde_json::to_string(&p).unwrap_or_default();
            assert_eq!(j, format!("\"{}\"", p.as_str()));
        }
        assert_eq!(Platform::parse("plan9-x64"), None);
    }

    #[test]
    fn current_is_never_any() {
        assert_ne!(Platform::current(), Some(Platform::Any));
    }
}
