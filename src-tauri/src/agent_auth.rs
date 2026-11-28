//! L1-WP14a (planset v060, lanes/L1_AGENT_RUNTIME.md WP14): one private home per ACP agent, the
//! environment an agent process starts with, and sign-out and removal.
//!
//! Every agent Core runs (Hermes, Codex, Claude Agent, Gemini CLI, goose, Grok Build, Muse Code)
//! keeps its own state under `<app data>/agents/<id>/home`, created owner-only (`0700`) at every
//! level. The agent's `HOME`, its XDG folders and the variables its own CLI reads for its config
//! folder (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `GEMINI_CLI_HOME`) all point inside that home, so an
//! agent's own sign-in (ChatGPT for Codex, Google for Gemini CLI) is written there and nowhere
//! else, and one agent's home never holds another agent's state.
//!
//! Rules (BYOK ADR P-2, P-6, P-8; lane rules in WP14):
//! - **No vendor key ever reaches an agent.** The environment starts empty ([`apply_env`] clears
//!   the inherited one) and takes only [`INHERITED_ENV`] names from the launch environment; any
//!   vendor key variable ([`crate::hermes::cloud::VENDOR_KEY_ENV`]) and any value shaped like a
//!   vendor key is dropped. A proxy-based agent gets exactly one credential: a scoped Citrate
//!   route token (`cmp_<hex>`, [`ModelProxy::mint_route`]) in [`ROUTE_TOKEN_ENV`], bound to one
//!   provider, model and agent session label. The key stays in the keychain, read by the proxy.
//! - **Arguments never carry a credential**, not even a route token ([`check_argv`]): argv is
//!   visible to every local process.
//! - **ACP `authenticate` never carries a vendor key** ([`check_authenticate`]).
//! - **Core never reads an agent's login files.** [`AgentHomes::home_status`] looks at metadata only.
//! - **Sign-out** revokes the agent's routes and empties its home; **removal** revokes its routes
//!   and deletes `<app data>/agents/<id>` (the uninstall cleanup). Neither follows a symlink out of
//!   the agent's folder, and neither touches another agent.
//! - **Cross-agent denial**: [`AgentHomes::resolve_in_home`] answers a path only when it resolves
//!   inside that agent's own home; another agent's home is refused by name.
//!
//! What this module does not do yet (honest scope): it does not spawn agents (the ACP host,
//! WP03, calls [`AgentHomes::launch_env`] and [`apply_env`]), and the OS-level boundary between
//! agents is the L2 sandbox; until it lands, isolation is by folder permissions and these checks.
//! Removal is an ordinary delete; the E4.3 secure-delete routine replaces it when it lands.
//! Agents that keep their sign-in in the OS keychain instead of a file (Claude Code on macOS) are
//! not isolated by the home, which is why Claude Agent signs in only through the Citrate proxy
//! (owner answer L1-Q3: API key only) and [`HomeLayout::own_login`] is false for it.
//!
//! Data sources (Rule 7): the agent id comes from `release/agents.lock.json` (WP13) through the
//! ACP host; the home is core's app data folder; route tokens come from the in-process model
//! proxy. Keyless: nothing here signs or holds a vendor key (Rule 3).

// Wired by the L1 agent runtime commands and the ACP host spawn (WP14b, WP03); until then only the
// tests below call it.
#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Component, Path, PathBuf};

use crate::model_provider::ProviderId;
use crate::model_proxy::{ModelProxy, ProxyError, ProxyGrant, ProxyRoute};

/// The folder under core's app data that holds every agent's folder.
pub const AGENTS_DIR: &str = "agents";
/// The agent's home inside its folder (`<app data>/agents/<id>/home`).
pub const HOME_DIR: &str = "home";
/// The one credential a proxy-based agent receives: its scoped route token.
pub const ROUTE_TOKEN_ENV: &str = "CITRATE_ROUTE_TOKEN";
/// The proxy's loopback base URL for that route.
pub const ROUTE_BASE_ENV: &str = "CITRATE_ROUTE_BASE_URL";
/// Session labels of agent routes: `ag:<id>:<16 hex>`. `:` is not an id character, so one agent's
/// prefix never matches another agent's labels (`ag:codex:` is not a prefix of `ag:codex-x:`).
pub const AGENT_ROUTE_PREFIX: &str = "ag:";
/// The longest agent id.
pub const MAX_ID_LEN: usize = 32;

/// Launch-environment names an agent process may inherit. Nothing else crosses. `PATH` is
/// inherited, so the ACP host (WP03) must launch the agent by the absolute path of its pinned,
/// hash-checked binary, never by a name looked up on `PATH`.
pub const INHERITED_ENV: &[&str] = &[
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TZ",
    "SYSTEMROOT",
];

/// The agent's own temp folder, `<home>/tmp` (`TMPDIR`, `TMP`, `TEMP`); the member's shared temp
/// folder never crosses.
pub const TMP_DIR: &str = "tmp";

/// Why a home operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The agent id is not a valid id.
    BadId(String),
    /// The path or folder is not safe to use (a symlink, outside the home, another agent's).
    Refused(String),
    /// The file system said no.
    Io(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::BadId(m) | AuthError::Refused(m) | AuthError::Io(m) => f.write_str(m),
        }
    }
}

impl From<AuthError> for String {
    fn from(e: AuthError) -> String {
        e.to_string()
    }
}

fn io(what: &str, e: std::io::Error) -> AuthError {
    AuthError::Io(format!("{what}: {e}"))
}

/// A validated agent id: 1..=32 of `[a-z0-9-]`, starting with a letter, not ending with `-`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    pub fn parse(s: &str) -> Result<AgentId, AuthError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_ID_LEN
            && s.as_bytes()[0].is_ascii_lowercase()
            && !s.ends_with('-')
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(AgentId(s.to_string()))
        } else {
            Err(AuthError::BadId(format!(
                "\"{}\" is not an agent id (1 to {MAX_ID_LEN} of a-z, 0-9 and -, starting with a letter)",
                s.escape_debug()
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How one agent keeps its own state inside its home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeLayout {
    pub id: &'static str,
    /// Variables the agent's own CLI reads for its config folder: (name, path relative to the
    /// home; `""` is the home itself).
    pub dirs: &'static [(&'static str, &'static str)],
    /// Fixed variables that keep the agent's state in files inside the home.
    pub fixed: &'static [(&'static str, &'static str)],
    /// Files whose presence means the agent's own sign-in is stored (metadata only, never read).
    pub login_files: &'static [&'static str],
    /// Whether Core offers the agent's own sign-in. False: the agent signs in only through the
    /// Citrate proxy (a route token), or needs no sign-in (Hermes).
    pub own_login: bool,
}

/// The layouts Core knows, checked against each agent's published source on 2026-11-27:
/// `@agentclientprotocol/codex-acp` 2.12.1 reads `CODEX_HOME` (login in `auth.json`);
/// `@google/gemini-cli` 0.71.1 resolves its home from `GEMINI_CLI_HOME` (then `.gemini/`, login in
/// `oauth_creds.json`) and keeps tokens in files when `GEMINI_FORCE_FILE_STORAGE` is set;
/// Claude Code reads `CLAUDE_CONFIG_DIR`. goose, Grok Build and Muse Code use the XDG folders and
/// `HOME`; their sign-in files are not yet verified, so none is listed and none is offered.
pub const LAYOUTS: &[HomeLayout] = &[
    HomeLayout {
        id: "hermes",
        dirs: &[],
        fixed: &[],
        login_files: &[],
        own_login: false,
    },
    HomeLayout {
        id: "codex",
        dirs: &[("CODEX_HOME", ".codex")],
        fixed: &[],
        login_files: &[".codex/auth.json"],
        own_login: true,
    },
    HomeLayout {
        id: "claude",
        dirs: &[("CLAUDE_CONFIG_DIR", ".claude")],
        fixed: &[],
        login_files: &[],
        own_login: false,
    },
    HomeLayout {
        id: "gemini",
        dirs: &[("GEMINI_CLI_HOME", "")],
        fixed: &[("GEMINI_FORCE_FILE_STORAGE", "true")],
        login_files: &[".gemini/oauth_creds.json"],
        own_login: true,
    },
    HomeLayout {
        id: "goose",
        dirs: &[],
        fixed: &[],
        login_files: &[],
        own_login: false,
    },
    HomeLayout {
        id: "grok",
        dirs: &[],
        fixed: &[],
        login_files: &[],
        own_login: false,
    },
    HomeLayout {
        id: "muse",
        dirs: &[],
        fixed: &[],
        login_files: &[],
        own_login: false,
    },
];

/// The layout of `id`, or the plain one (HOME and XDG only, no sign-in offered).
pub fn layout_for(id: &AgentId) -> HomeLayout {
    LAYOUTS
        .iter()
        .find(|l| l.id == id.as_str())
        .copied()
        .unwrap_or(HomeLayout {
            id: "",
            dirs: &[],
            fixed: &[],
            login_files: &[],
            own_login: false,
        })
}

/// What Core can say about an agent's home without reading a secret.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeStatus {
    pub id: String,
    pub home_exists: bool,
    /// Whether the agent's own sign-in file is present (`None`: Core does not know where this
    /// agent keeps it, or does not offer its own sign-in).
    pub own_login_present: Option<bool>,
    pub own_login_offered: bool,
}

/// Every agent's folder under `<app data>/agents`.
#[derive(Debug, Clone)]
pub struct AgentHomes {
    root: PathBuf,
}

impl AgentHomes {
    /// `app_data` is core's app data folder; it must exist.
    pub fn new(app_data: &Path) -> AgentHomes {
        AgentHomes {
            root: app_data.join(AGENTS_DIR),
        }
    }

    /// `<root>/<id>`.
    pub fn agent_dir(&self, id: &AgentId) -> PathBuf {
        self.root.join(id.as_str())
    }

    /// `<root>/<id>/home`.
    pub fn home(&self, id: &AgentId) -> PathBuf {
        self.agent_dir(id).join(HOME_DIR)
    }

    /// Create (or check) the agent's home: every level from `agents/` down is a real folder this
    /// user owns, none a symlink (each is opened without following links), narrowed to `0700`.
    pub fn ensure_home(&self, id: &AgentId) -> Result<PathBuf, AuthError> {
        nf_ensure_chain(self.base(), &[AGENTS_DIR, id.as_str(), HOME_DIR])?;
        Ok(self.home(id))
    }

    /// Core's app data folder (trusted: core made it; everything below it is checked).
    fn base(&self) -> &Path {
        self.root.parent().unwrap_or(&self.root)
    }

    /// The environment the agent process starts with (after [`apply_env`] clears the inherited
    /// one): the allowed launch variables, the home and its folders, and at most one route.
    pub fn launch_env(
        &self,
        id: &AgentId,
        inherited: impl Iterator<Item = (String, String)>,
        route: Option<&ProxyGrant>,
    ) -> Result<Vec<(String, String)>, AuthError> {
        let home = self.ensure_home(id)?;
        nf_ensure_chain(self.base(), &[AGENTS_DIR, id.as_str(), HOME_DIR, TMP_DIR])?;
        let layout = layout_for(id);
        let mut env: Vec<(String, String)> = inherited
            .filter(|(k, v)| {
                INHERITED_ENV.iter().any(|n| n.eq_ignore_ascii_case(k))
                    && !crate::telemetry::has_key_shape(v)
            })
            .collect();
        let at = |rel: &str| -> String {
            if rel.is_empty() {
                home.display().to_string()
            } else {
                home.join(rel).display().to_string()
            }
        };
        let mut set = |k: &str, v: String| {
            env.retain(|(n, _)| !n.eq_ignore_ascii_case(k));
            env.push((k.to_string(), v));
        };
        set("HOME", at(""));
        set("USERPROFILE", at(""));
        set("XDG_CONFIG_HOME", at(".config"));
        set("XDG_DATA_HOME", at(".local/share"));
        set("XDG_STATE_HOME", at(".local/state"));
        set("XDG_CACHE_HOME", at(".cache"));
        for k in ["TMPDIR", "TMP", "TEMP"] {
            set(k, at(TMP_DIR));
        }
        for &(k, rel) in layout.dirs {
            set(k, at(rel));
        }
        for &(k, v) in layout.fixed {
            set(k, v.to_string());
        }
        if let Some(g) = route {
            let token = g.bearer.expose();
            if !is_route_token(token) {
                return Err(AuthError::Refused(
                    "refused: an agent receives only a Citrate route token".into(),
                ));
            }
            set(ROUTE_BASE_ENV, g.base_url.clone());
            set(ROUTE_TOKEN_ENV, token.to_string());
        }
        check_env(&env)?;
        Ok(env)
    }

    /// Whether the agent's home exists and its own sign-in file is present. Metadata only: no
    /// file in the home is opened.
    pub fn home_status(&self, id: &AgentId) -> HomeStatus {
        let home = self.home(id);
        let layout = layout_for(id);
        let home_exists = std::fs::symlink_metadata(&home).is_ok_and(|m| m.is_dir());
        let own_login_present = (layout.own_login && !layout.login_files.is_empty()).then(|| {
            home_exists
                && layout
                    .login_files
                    .iter()
                    .any(|f| std::fs::symlink_metadata(home.join(f)).is_ok_and(|m| m.is_file()))
        });
        HomeStatus {
            id: id.as_str().to_string(),
            home_exists,
            own_login_present,
            own_login_offered: layout.own_login,
        }
    }

    /// Sign the agent out: revoke its routes and empty its home (its own sign-in and everything
    /// else it kept). The path from `agents/` to the home is opened without following links, and
    /// the home is emptied through those handles, so a symlink anywhere on it is refused and the
    /// delete can never reach outside the agent's folder. The home is left empty and owner-only.
    pub fn sign_out_agent(
        &self,
        id: &AgentId,
        proxy: Option<&ModelProxy>,
    ) -> Result<(), AuthError> {
        revoke_agent_routes(proxy, id);
        nf_empty_chain(self.base(), &[AGENTS_DIR, id.as_str(), HOME_DIR])?;
        self.ensure_home(id).map(|_| ())
    }

    /// Remove the agent's folder (uninstall cleanup): revoke its routes and delete
    /// `agents/<id>` through handles opened without following links (a link inside is removed,
    /// never its target). Returns whether anything was removed.
    pub fn remove_agent_dir(
        &self,
        id: &AgentId,
        proxy: Option<&ModelProxy>,
    ) -> Result<bool, AuthError> {
        revoke_agent_routes(proxy, id);
        nf_remove_last(self.base(), &[AGENTS_DIR, id.as_str()])
    }

    /// `requested` resolved inside `id`'s own home, or refused: outside every home, or inside
    /// another agent's (named). Symlinks are resolved first, so a link out of the home is refused.
    pub fn resolve_in_home(&self, id: &AgentId, requested: &Path) -> Result<PathBuf, AuthError> {
        let home = self
            .home(id)
            .canonicalize()
            .map_err(|e| io("the agent's home", e))?;
        let path = resolve_existing_prefix(requested)?;
        if path.starts_with(&home) {
            return Ok(path);
        }
        if let Ok(root) = self.root.canonicalize() {
            if let Ok(rest) = path.strip_prefix(&root) {
                if let Some(Component::Normal(other)) = rest.components().next() {
                    return Err(AuthError::Refused(format!(
                        "refused: {} belongs to the agent \"{}\", not \"{}\"",
                        requested.display(),
                        other.to_string_lossy(),
                        id.as_str()
                    )));
                }
            }
        }
        Err(AuthError::Refused(format!(
            "refused: {} is outside the home of \"{}\"",
            requested.display(),
            id.as_str()
        )))
    }
}

fn refused_link(names: &[&str]) -> AuthError {
    AuthError::Refused(format!(
        "refused: {} is not a folder Core made (a symlink, a file or another user's)",
        names.join("/")
    ))
}

/// Folder operations below core's app data that never follow a link (review P2-1 on #842). On
/// unix every component is opened with `O_NOFOLLOW | O_DIRECTORY` relative to its parent's handle,
/// checked owned by this user, chmodded through the handle, and deleted with `unlinkat` relative to
/// its parent's handle, so swapping a component for a link between a check and a delete cannot
/// redirect it.
#[cfg(unix)]
mod nofollow {
    use std::ffi::{CStr, CString};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub(super) enum Kind {
        Missing,
        Dir,
        Other,
    }

    pub(super) fn is_link_or_not_dir(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ELOOP) | Some(libc::ENOTDIR))
    }

    pub(super) fn cname(s: &str) -> io::Result<CString> {
        CString::new(s).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    fn fd_or_err(fd: libc::c_int) -> io::Result<OwnedFd> {
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a descriptor the kernel just returned to us and nothing else owns.
        Ok(unsafe { OwnedFd::from_raw_fd(fd) })
    }

    fn rc(r: libc::c_int) -> io::Result<()> {
        if r < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// Core's app data folder (followed: core made it).
    pub(super) fn open_base(p: &Path) -> io::Result<OwnedFd> {
        let c = CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
        fd_or_err(unsafe {
            libc::open(
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        })
    }

    /// `name` inside `dir`, refused (ELOOP or ENOTDIR) when it is a link or not a folder.
    pub(super) fn open_child(dir: &OwnedFd, name: &CStr) -> io::Result<OwnedFd> {
        // SAFETY: `dir` is an open descriptor and `name` a valid C string for the call.
        fd_or_err(unsafe {
            libc::openat(
                dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })
    }

    pub(super) fn mkdir_child(dir: &OwnedFd, name: &CStr) -> io::Result<()> {
        // SAFETY: as in `open_child`.
        match rc(unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), 0o700) }) {
            Err(e) if e.raw_os_error() == Some(libc::EEXIST) => Ok(()),
            r => r,
        }
    }

    /// Owned by this user, then narrowed to `0700` through the handle (never by path).
    pub(super) fn own_and_narrow(fd: &OwnedFd) -> io::Result<bool> {
        // SAFETY: `stat` is plain old data; zeroed is a valid value that fstat overwrites.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is open and `st` is a valid out pointer.
        rc(unsafe { libc::fstat(fd.as_raw_fd(), &mut st) })?;
        // SAFETY: geteuid has no preconditions.
        if st.st_uid != unsafe { libc::geteuid() } {
            return Ok(false);
        }
        // SAFETY: `fd` is open.
        rc(unsafe { libc::fchmod(fd.as_raw_fd(), 0o700) })?;
        Ok(true)
    }

    pub(super) fn kind_at(dir: &OwnedFd, name: &CStr) -> io::Result<Kind> {
        // SAFETY: as in `own_and_narrow`.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dir` is open, `name` valid, `st` a valid out pointer.
        let r = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        match rc(r) {
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => Ok(Kind::Missing),
            Err(e) => Err(e),
            Ok(()) if (st.st_mode & libc::S_IFMT) == libc::S_IFDIR => Ok(Kind::Dir),
            Ok(()) => Ok(Kind::Other),
        }
    }

    /// The entry names of `dir` (without `.` and `..`).
    fn entry_names(dir: &OwnedFd) -> io::Result<Vec<CString>> {
        // SAFETY: `dir` is open; the duplicate is owned by the DIR stream from here on.
        let dup = unsafe { libc::dup(dir.as_raw_fd()) };
        if dup < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `dup` is a fresh descriptor; on success fdopendir takes ownership of it.
        let d = unsafe { libc::fdopendir(dup) };
        if d.is_null() {
            let e = io::Error::last_os_error();
            // SAFETY: fdopendir failed, so `dup` is still ours to close.
            unsafe { libc::close(dup) };
            return Err(e);
        }
        // SAFETY: `d` is a valid stream until closedir.
        unsafe { libc::rewinddir(d) };
        let mut out = Vec::new();
        loop {
            // SAFETY: `d` is a valid stream; the entry is read before the next readdir call.
            let ent = unsafe { libc::readdir(d) };
            if ent.is_null() {
                break;
            }
            // SAFETY: `d_name` is a NUL-terminated name inside the entry readdir returned.
            let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
            let b = name.to_bytes();
            if b != b"." && b != b".." {
                out.push(name.to_owned());
            }
        }
        // SAFETY: `d` is a valid stream, closed once (this also closes `dup`).
        unsafe { libc::closedir(d) };
        Ok(out)
    }

    fn unlink_at(dir: &OwnedFd, name: &CStr, flags: libc::c_int) -> io::Result<()> {
        // SAFETY: `dir` is open and `name` valid.
        rc(unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) })
    }

    /// Delete everything inside `dir`, relative to its handle: links and files are unlinked,
    /// folders are opened without following links, emptied, then removed.
    pub(super) fn empty_dir(dir: &OwnedFd) -> io::Result<()> {
        for n in entry_names(dir)? {
            match kind_at(dir, &n)? {
                Kind::Missing => {}
                Kind::Other => unlink_at(dir, &n, 0)?,
                Kind::Dir => {
                    let sub = open_child(dir, &n)?;
                    empty_dir(&sub)?;
                    drop(sub);
                    unlink_at(dir, &n, libc::AT_REMOVEDIR)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn remove_dir_at(dir: &OwnedFd, name: &CStr) -> io::Result<()> {
        unlink_at(dir, name, libc::AT_REMOVEDIR)
    }
}

/// Open `names` below `base` one handle at a time, never following a link; `create` makes a
/// missing component (`0700`). `Ok(None)`: a component is missing and `create` is off.
#[cfg(unix)]
fn nf_open_chain(
    base: &Path,
    names: &[&str],
    create: bool,
) -> Result<Option<std::os::fd::OwnedFd>, AuthError> {
    let mut cur = nofollow::open_base(base).map_err(|e| io("core's app data folder", e))?;
    for (i, n) in names.iter().enumerate() {
        let c = nofollow::cname(n).map_err(|e| io("an agent folder name", e))?;
        if create {
            nofollow::mkdir_child(&cur, &c).map_err(|e| io("create the agent's folder", e))?;
        }
        let next = match nofollow::open_child(&cur, &c) {
            Ok(fd) => fd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => return Ok(None),
            Err(e) if nofollow::is_link_or_not_dir(&e) => return Err(refused_link(&names[..=i])),
            Err(e) => return Err(io("open the agent's folder", e)),
        };
        if !nofollow::own_and_narrow(&next).map_err(|e| io("the agent's folder", e))? {
            return Err(refused_link(&names[..=i]));
        }
        cur = next;
    }
    Ok(Some(cur))
}

#[cfg(unix)]
fn nf_ensure_chain(base: &Path, names: &[&str]) -> Result<(), AuthError> {
    nf_open_chain(base, names, true).map(|_| ())
}

/// Empty the last folder of `names`; `Ok(false)` when it does not exist.
#[cfg(unix)]
fn nf_empty_chain(base: &Path, names: &[&str]) -> Result<bool, AuthError> {
    match nf_open_chain(base, names, false)? {
        None => Ok(false),
        Some(fd) => nofollow::empty_dir(&fd)
            .map(|()| true)
            .map_err(|e| io("empty the agent's folder", e)),
    }
}

/// Delete the last folder of `names` (a link there is refused); `Ok(false)` when it is missing.
#[cfg(unix)]
fn nf_remove_last(base: &Path, names: &[&str]) -> Result<bool, AuthError> {
    let Some((last, parents)) = names.split_last() else {
        return Ok(false);
    };
    let Some(parent) = nf_open_chain(base, parents, false)? else {
        return Ok(false);
    };
    let c = nofollow::cname(last).map_err(|e| io("an agent folder name", e))?;
    match nofollow::kind_at(&parent, &c).map_err(|e| io("the agent's folder", e))? {
        nofollow::Kind::Missing => return Ok(false),
        nofollow::Kind::Other => return Err(refused_link(names)),
        nofollow::Kind::Dir => {}
    }
    let fd = match nofollow::open_child(&parent, &c) {
        Ok(fd) => fd,
        Err(e) if nofollow::is_link_or_not_dir(&e) => return Err(refused_link(names)),
        Err(e) => return Err(io("open the agent's folder", e)),
    };
    if !nofollow::own_and_narrow(&fd).map_err(|e| io("the agent's folder", e))? {
        return Err(refused_link(names));
    }
    nofollow::empty_dir(&fd).map_err(|e| io("remove the agent's folder", e))?;
    drop(fd);
    nofollow::remove_dir_at(&parent, &c).map_err(|e| io("remove the agent's folder", e))?;
    Ok(true)
}

/// Non-unix: every component is checked with `symlink_metadata` before use (no handle-relative
/// delete is available here, so this is check-then-act).
#[cfg(not(unix))]
fn nf_checked_path(
    base: &Path,
    names: &[&str],
    create: bool,
) -> Result<Option<PathBuf>, AuthError> {
    let mut cur = base.to_path_buf();
    for (i, n) in names.iter().enumerate() {
        cur.push(n);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                return Err(refused_link(&names[..=i]))
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                match std::fs::create_dir(&cur) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(io("create the agent's folder", e)),
                }
                let m = std::fs::symlink_metadata(&cur).map_err(|e| io("the agent's folder", e))?;
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Err(refused_link(&names[..=i]));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io("the agent's folder", e)),
        }
    }
    Ok(Some(cur))
}

#[cfg(not(unix))]
fn nf_ensure_chain(base: &Path, names: &[&str]) -> Result<(), AuthError> {
    nf_checked_path(base, names, true).map(|_| ())
}

#[cfg(not(unix))]
fn nf_empty_chain(base: &Path, names: &[&str]) -> Result<bool, AuthError> {
    let Some(p) = nf_checked_path(base, names, false)? else {
        return Ok(false);
    };
    for e in std::fs::read_dir(&p).map_err(|e| io("empty the agent's folder", e))? {
        let e = e.map_err(|e| io("empty the agent's folder", e))?;
        let ft = e
            .file_type()
            .map_err(|e| io("empty the agent's folder", e))?;
        let r = if ft.is_dir() && !ft.is_symlink() {
            std::fs::remove_dir_all(e.path())
        } else {
            std::fs::remove_file(e.path())
        };
        r.map_err(|e| io("empty the agent's folder", e))?;
    }
    Ok(true)
}

#[cfg(not(unix))]
fn nf_remove_last(base: &Path, names: &[&str]) -> Result<bool, AuthError> {
    match nf_checked_path(base, names, false)? {
        None => Ok(false),
        Some(p) => std::fs::remove_dir_all(&p)
            .map(|()| true)
            .map_err(|e| io("remove the agent's folder", e)),
    }
}

/// The canonical form of `p` when it exists, else of its nearest existing ancestor joined with
/// the rest. A path that does not exist may not hold `..`.
fn resolve_existing_prefix(p: &Path) -> Result<PathBuf, AuthError> {
    if !p.is_absolute() {
        return Err(AuthError::Refused(format!(
            "refused: {} is not an absolute path",
            p.display()
        )));
    }
    if let Ok(c) = p.canonicalize() {
        return Ok(c);
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(AuthError::Refused(format!(
            "refused: {} climbs out of a path that does not exist",
            p.display()
        )));
    }
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p;
    while let (Some(name), Some(parent)) = (cur.file_name(), cur.parent()) {
        rest.push(name.to_os_string());
        cur = parent;
        if let Ok(c) = cur.canonicalize() {
            let mut out = c;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return Ok(out);
        }
    }
    Err(AuthError::Refused(format!(
        "refused: {} cannot be resolved",
        p.display()
    )))
}

fn is_route_token(s: &str) -> bool {
    s.strip_prefix("cmp_")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The environment check run on every agent launch: no vendor key variable by name, and no value
/// shaped like a vendor key (the route token is not one). Fails closed.
pub fn check_env(env: &[(String, String)]) -> Result<(), AuthError> {
    for (k, v) in env {
        if crate::hermes::cloud::VENDOR_KEY_ENV
            .iter()
            .any(|n| n.eq_ignore_ascii_case(k))
        {
            return Err(AuthError::Refused(format!(
                "refused: {k} is a vendor key variable and never reaches an agent"
            )));
        }
        crate::hermes::cloud::ensure_keyless("environment", v).map_err(AuthError::Refused)?;
        if k != ROUTE_TOKEN_ENV && is_route_token(v) {
            return Err(AuthError::Refused(format!(
                "refused: a route token rides only in {ROUTE_TOKEN_ENV}, not {k}"
            )));
        }
    }
    Ok(())
}

/// The argument check: no credential of any kind, route tokens included (argv is visible to every
/// local process).
pub fn check_argv(args: &[String]) -> Result<(), AuthError> {
    if args.iter().any(|a| crate::telemetry::has_key_shape(a)) {
        return Err(AuthError::Refused(
            "refused: an agent's arguments never carry a key or a route token".into(),
        ));
    }
    Ok(())
}

/// The ACP `authenticate` check: the request Core sends (method id and params as JSON) never
/// carries a vendor key. Core only ever selects a method; it never forwards a key. A per-agent
/// allowlist of method ids (from the agent's `initialize.authMethods`) lands with the ACP host
/// (WP03), which is the only caller.
pub fn check_authenticate(params_json: &str) -> Result<(), AuthError> {
    crate::hermes::cloud::ensure_keyless("ACP authenticate request", params_json)
        .map_err(AuthError::Refused)
}

/// Set `env` as the whole environment of `cmd`: the inherited one is cleared first.
pub fn apply_env(cmd: &mut std::process::Command, env: &[(String, String)]) {
    cmd.env_clear();
    cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
}

/// The route session label prefix of one agent.
pub fn agent_route_prefix(id: &AgentId) -> String {
    format!("{AGENT_ROUTE_PREFIX}{}:", id.as_str())
}

/// Mint a route for one agent session: bound to the provider and model the member chose, labelled
/// `ag:<id>:<16 hex>` so the agent's routes can be revoked together.
pub fn mint_agent_route(
    proxy: &ModelProxy,
    id: &AgentId,
    provider: ProviderId,
    model: &str,
    gate: PathBuf,
) -> Result<ProxyGrant, ProxyError> {
    let label = format!("{}{:016x}", agent_route_prefix(id), rand::random::<u64>());
    proxy.mint_route(
        ProxyRoute {
            provider,
            model: model.to_string(),
            session_id: label,
        },
        gate,
    )
}

/// Revoke every route of one agent (sign-out, removal, and the ACP host when it stops the agent).
pub fn revoke_agent_routes(proxy: Option<&ModelProxy>, id: &AgentId) {
    if let Some(p) = proxy {
        p.revoke_sessions_with_prefix(&agent_route_prefix(id));
    }
}

#[cfg(test)]
mod tests {
    include!("agent_auth_tests.rs");
}
