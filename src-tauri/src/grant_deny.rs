//! The folders no agent grant may be rooted in: core's copy of the agent's default-deny list.
//!
//! The authority is `agent-guard` in citrate-agent-runtime (`RULES`, `PREFIX_RULES`); the sidecar
//! checks it on every file operation and ignores any grant rooted in one of these places. Core
//! keeps the same list so the member gets a clear refusal when picking such a folder, and so the
//! Grants panel never shows as active a grant the agent would ignore. Keep the two in step: a row
//! here is a run of folded path components (lowercase, trailing dots and spaces removed), matched
//! anywhere in the path unless it is anchored at the filesystem root.

use std::path::{Component, Path};

const AS: &str = "application support";

/// Runs matched anywhere in the path.
const ANYWHERE: &[(&str, &[&str])] = &[
    ("credentials", &[".ssh"]),
    ("credentials", &[".gnupg"]),
    ("credentials", &[".aws"]),
    ("credentials", &[".azure"]),
    ("credentials", &[".kube"]),
    ("credentials", &[".config", "gcloud"]),
    ("credentials", &[".docker", "config.json"]),
    ("credentials", &[".netrc"]),
    ("credentials", &[".npmrc"]),
    ("credentials", &[".pypirc"]),
    ("credentials", &[".git-credentials"]),
    ("credentials", &[".config", "gh"]),
    ("credentials", &[".config", "hub"]),
    ("credentials", &[".cargo", "credentials"]),
    ("credentials", &[".cargo", "credentials.toml"]),
    ("credentials", &[".terraform.d", "credentials.tfrc.json"]),
    ("credentials", &[".password-store"]),
    ("keychain", &["library", "keychains"]),
    ("keychain", &[".local", "share", "keyrings"]),
    ("keychain", &[".gnome2", "keyrings"]),
    ("keychain", &[".local", "share", "kwalletd"]),
    ("keychain", &[".kde", "share", "apps", "kwallet"]),
    ("keychain", &[".kde4", "share", "apps", "kwallet"]),
    (
        "keychain",
        &["appdata", "roaming", "microsoft", "credentials"],
    ),
    (
        "keychain",
        &["appdata", "local", "microsoft", "credentials"],
    ),
    ("keychain", &["appdata", "roaming", "microsoft", "protect"]),
    ("keychain", &["appdata", "roaming", "microsoft", "vault"]),
    ("keychain", &["appdata", "local", "microsoft", "vault"]),
    ("browser profile", &["library", AS, "google", "chrome"]),
    ("browser profile", &["library", AS, "google", "chrome beta"]),
    (
        "browser profile",
        &["library", AS, "google", "chrome canary"],
    ),
    ("browser profile", &["library", AS, "chromium"]),
    ("browser profile", &["library", AS, "bravesoftware"]),
    ("browser profile", &["library", AS, "microsoft edge"]),
    ("browser profile", &["library", AS, "firefox"]),
    ("browser profile", &["library", AS, "arc"]),
    ("browser profile", &["library", AS, "vivaldi"]),
    (
        "browser profile",
        &["library", AS, "com.operasoftware.opera"],
    ),
    ("browser profile", &["library", "safari"]),
    (
        "browser profile",
        &["library", "containers", "com.apple.safari"],
    ),
    ("browser profile", &["library", "cookies"]),
    ("browser profile", &[".config", "google-chrome"]),
    ("browser profile", &[".config", "google-chrome-beta"]),
    ("browser profile", &[".config", "chromium"]),
    ("browser profile", &[".config", "bravesoftware"]),
    ("browser profile", &[".config", "microsoft-edge"]),
    ("browser profile", &[".config", "vivaldi"]),
    ("browser profile", &[".config", "opera"]),
    ("browser profile", &[".mozilla"]),
    ("browser profile", &["snap", "chromium"]),
    ("browser profile", &["snap", "firefox"]),
    ("browser profile", &[".var", "app", "org.mozilla.firefox"]),
    ("browser profile", &[".var", "app", "com.google.chrome"]),
    ("browser profile", &[".var", "app", "com.brave.browser"]),
    ("browser profile", &["appdata", "local", "google", "chrome"]),
    ("browser profile", &["appdata", "local", "chromium"]),
    ("browser profile", &["appdata", "local", "bravesoftware"]),
    (
        "browser profile",
        &["appdata", "local", "microsoft", "edge"],
    ),
    ("browser profile", &["appdata", "local", "vivaldi"]),
    ("browser profile", &["appdata", "roaming", "opera software"]),
    ("browser profile", &["appdata", "roaming", "mozilla"]),
    ("browser profile", &["appdata", "local", "mozilla"]),
    ("wallet storage", &["local extension settings"]),
    ("wallet storage", &["sync extension settings"]),
    ("wallet storage", &["managed extension settings"]),
    ("wallet storage", &[".ethereum", "keystore"]),
    ("wallet storage", &["library", "ethereum", "keystore"]),
    ("wallet storage", &[".foundry", "keystores"]),
    ("wallet storage", &[".citrate-wallet"]),
    ("wallet storage", &[".electrum"]),
    ("wallet storage", &["library", AS, "exodus"]),
    ("wallet storage", &[".bitcoin", "wallets"]),
    ("wallet storage", &["library", AS, "bitcoin", "wallets"]),
    ("Citrate app data", &[".citrate", "noise"]),
    ("Citrate app data", &[".citrate", "proposer"]),
    ("Citrate app data", &[".citrate", "keystore"]),
    ("Citrate app data", &[".citrate", "node"]),
    ("shell history", &[".zsh_history"]),
    ("shell history", &[".bash_history"]),
    ("shell history", &[".sh_history"]),
    ("shell history", &[".ksh_history"]),
    ("shell history", &[".history"]),
    ("shell history", &["fish_history"]),
    ("shell history", &[".zsh_sessions"]),
    ("shell history", &[".bash_sessions"]),
    ("shell history", &[".python_history"]),
    ("shell history", &[".node_repl_history"]),
    ("shell history", &[".psql_history"]),
    ("shell history", &[".mysql_history"]),
    ("shell history", &[".sqlite_history"]),
    ("shell history", &[".rediscli_history"]),
    ("shell history", &[".irb_history"]),
    ("shell history", &[".lesshst"]),
    ("shell history", &["consolehost_history.txt"]),
    ("system secrets", &["windows", "system32", "config"]),
];

/// Runs anchored at the filesystem root.
const AT_ROOT: &[&[&str]] = &[
    &["etc", "shadow"],
    &["etc", "shadow-"],
    &["etc", "gshadow"],
    &["etc", "gshadow-"],
    &["etc", "master.passwd"],
    &["etc", "sudoers"],
    &["etc", "sudoers.d"],
    &["etc", "ssh"],
    &["etc", "ssl", "private"],
    &["etc", "krb5.keytab"],
    &["etc", "security", "opasswd"],
    &["private", "etc", "master.passwd"],
    &["private", "etc", "sudoers"],
    &["private", "etc", "sudoers.d"],
    &["private", "etc", "ssh"],
    &["private", "etc", "krb5.keytab"],
    &["private", "var", "db"],
    &["var", "db"],
    &["root"],
    &["proc"],
    &["dev", "mem"],
    &["dev", "kmem"],
    &["dev", "port"],
];

/// A component whose folded form starts with one of these is denied.
const PREFIXES: &[(&str, &str)] = &[
    ("wallet storage", "chrome-extension_"),
    ("wallet storage", "moz-extension+++"),
    ("Citrate app data", "ai.citrate.core"),
];

/// Lowercase, without trailing dots and spaces and without a Windows stream suffix.
fn fold(c: &str) -> String {
    let base = c.split("::").next().unwrap_or(c);
    base.to_lowercase().trim_end_matches(['.', ' ']).to_string()
}

/// The deny category `path` falls in, if any (it, or a folder above it, is a denied location).
pub fn denied_location(path: &Path) -> Option<&'static str> {
    let parts: Vec<String> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(fold(&s.to_string_lossy())),
            _ => None,
        })
        .collect();
    let matches_at = |run: &[&str], i: usize| {
        run.len() <= parts.len() - i && run.iter().enumerate().all(|(k, r)| parts[i + k] == *r)
    };
    for (cat, run) in ANYWHERE {
        if (0..parts.len()).any(|i| matches_at(run, i)) {
            return Some(cat);
        }
    }
    if path.has_root() && AT_ROOT.iter().any(|run| matches_at(run, 0)) {
        return Some("system secrets");
    }
    for (cat, prefix) in PREFIXES {
        if parts.iter().any(|p| p.starts_with(prefix)) {
            return Some(cat);
        }
    }
    None
}
