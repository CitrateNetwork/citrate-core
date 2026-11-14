//! V60 SVC-1: `citrate-svc`, the background service, and the app's link to it.
//!
//! Planset: fed `.agentile/planset/2026-11-07-v060/02_ARCHITECTURE.md` §9; spike notes:
//! `docs/spikes/svc-2026-11-10/SPIKE_NOTES.md` (core#623). Owner decision 2026-11-07: the service
//! is on by default and the member can turn it off at setup.
//!
//! The service is the same executable as the app, started headless: `citrate-core --svc`
//! ([`SVC_FLAG`]), dispatched in `main` before any window or plugin exists. "citrate-svc" names
//! that mode, not a second binary, for three reasons:
//! - the AppImage case works: its binaries sit in a FUSE mount that exists only while the
//!   AppImage runs, so the unit runs the AppImage itself with `--svc` (spike finding 3);
//! - nothing new is bundled, signed or notarized;
//! - the app's startup orphan sweep (`owned_cleanup::select_orphans`) leaves a sidecar alone when
//!   its parent is a live instance of this installation's main executable. The service's node and
//!   kubo have the main executable as their parent, so an app opened while the service runs never
//!   signals them. A separate service binary would make them look orphaned
//!   (`svc_children_are_not_orphans_to_the_app`).
//!
//! What the service runs: the node and kubo, through the same [`crate::node::NodeManager`] and
//! [`crate::ipfs::IpfsManager`] the app uses in-process, under the same kit supervisor and the
//! same app-wide [`citrate_core_kit::supervisor::SpawnGate`]. It also runs the mining gate loop
//! the app runs (`spawn_mining_watchdog`), so a node keeps producing while the app is closed.
//! Block production is the one signature the service makes (planset §9 "Never in background").
//! It never touches the wallet vault: the coinbase is a public address the app hands over
//! ([`SvcOp::SetCoinbase`]).
//!
//! ## The link (this file)
//! - **Socket:** a Unix socket at `<app data>/svc/svc.sock`, in a `0700` directory, the socket
//!   itself `0600`. Newline-delimited JSON, one request per line, one response per line.
//! - **Bearer:** 32 random bytes, hex, minted by the service at every start and written `0600`
//!   to `<app data>/svc/svc.bearer` before the socket listens. Every request carries it; the
//!   service compares it in constant time. The client refuses a bearer file that is a link, is
//!   not owned by this user, or is readable by anyone else.
//! - **Attach or fall back:** [`attach`] answers [`Link::Attached`] only when the socket accepts,
//!   the bearer is valid and `hello` answers with this protocol version. Anything else is
//!   [`Link::InProcess`] with the reason, and the app runs its supervisors in-process as before.
//!
//! Unix only. Windows waits for its signing certificate (owner, 2026-11-07); there the app is
//! always in-process.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// The argument that starts the app executable as the background service.
pub const SVC_FLAG: &str = "--svc";
/// The bundle identifier: the app data dir is `<platform data dir>/ai.citrate.core`, the same
/// directory Tauri's `app_data_dir()` resolves (`dirs::data_dir()` joined with the identifier).
pub const APP_IDENTIFIER: &str = "ai.citrate.core";
/// Overrides the app data dir the service uses (tests and side-by-side dev installs).
pub const DATA_DIR_ENV: &str = "CITRATE_SVC_DATA_DIR";
/// The service's directory inside the app data dir.
pub const SVC_DIR: &str = "svc";
/// The socket's file name.
pub const SOCKET_FILE: &str = "svc.sock";
/// The bearer file's name.
pub const BEARER_FILE: &str = "svc.bearer";
/// The persisted coinbase (a public address), so a service restarted with the app closed still
/// mines to it.
pub const COINBASE_FILE: &str = "coinbase";
/// The wire protocol version; a service that answers another one is not attached to.
pub const PROTOCOL_VERSION: u32 = 1;
/// The service exits with this code when it wants the unit manager to start it again (an update
/// replaced its executable). `Restart=on-failure` and `KeepAlive { SuccessfulExit = false }` both
/// restart a non-zero exit; a requested stop exits 0 and stays stopped.
pub const EXIT_RESTART: i32 = 75;
/// Bearer length in bytes (256-bit).
pub const BEARER_LEN: usize = 32;
/// How long a client waits on one request. A drain can take up to the 30 s exit wait.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
/// How long `attach` waits for `hello`.
pub const ATTACH_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest request line the service reads.
pub const MAX_LINE: usize = 16 * 1024;

/// Where the service keeps its socket, bearer and coinbase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvcPaths {
    /// The app data dir (the node and kubo data live under it, as in-process).
    pub data_root: PathBuf,
    pub dir: PathBuf,
    pub socket: PathBuf,
    pub bearer: PathBuf,
    pub coinbase: PathBuf,
}

impl SvcPaths {
    /// The paths under an explicit app data dir.
    pub fn under(data_root: impl Into<PathBuf>) -> Self {
        let data_root = data_root.into();
        let dir = data_root.join(SVC_DIR);
        SvcPaths {
            socket: dir.join(SOCKET_FILE),
            bearer: dir.join(BEARER_FILE),
            coinbase: dir.join(COINBASE_FILE),
            dir,
            data_root,
        }
    }

    /// The paths the service uses when no app handle exists: [`DATA_DIR_ENV`] if set, else the
    /// platform data dir joined with [`APP_IDENTIFIER`] (what the app's `app_data_dir()` is).
    pub fn headless() -> Result<Self, String> {
        Self::headless_from(
            std::env::var_os(DATA_DIR_ENV).map(PathBuf::from),
            dirs::data_dir(),
        )
    }

    /// [`Self::headless`] over explicit inputs (tested on every platform).
    pub fn headless_from(
        env_override: Option<PathBuf>,
        platform_data_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        if let Some(p) = env_override.filter(|p| !p.as_os_str().is_empty()) {
            if !p.is_absolute() {
                return Err(format!("{DATA_DIR_ENV} must be an absolute path"));
            }
            return Ok(Self::under(p));
        }
        platform_data_dir
            .map(|d| Self::under(d.join(APP_IDENTIFIER)))
            .ok_or_else(|| "no per-user data directory on this system".to_string())
    }
}

/// One request: the protocol version, the bearer, and the operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvcRequest {
    pub v: u32,
    /// The session bearer (zeroized on drop; written to the wire only here).
    #[serde(serialize_with = "citrate_core_kit::secret::serialize_exposed")]
    pub bearer: citrate_core_kit::secret::SecretString,
    #[serde(flatten)]
    pub op: SvcOp,
}

/// What the app can ask the service. Nothing here signs or reads a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SvcOp {
    /// Identify the service (version, pid).
    Hello,
    /// The node's status (the same [`crate::node::NodeStatus`] the app reads in-process), kubo,
    /// and whether the service is drained.
    Status,
    NodeStart,
    NodeStop,
    IpfsStart,
    IpfsStop,
    /// The node's coinbase, resolved by the app from the member's wallet (a public address).
    SetCoinbase {
        coinbase: String,
    },
    /// Run the mining gate now (the app's "arm" button); the service also runs it every 30 s.
    ArmMining,
    /// CORE-D6 drain before an update: hold the spawn gate, stop the node and kubo, wait for the
    /// children to exit.
    Drain,
    /// Release a drain (the update was cancelled or failed) and start the services again.
    Resume,
    /// Exit with [`EXIT_RESTART`] so the unit manager starts the (new) executable.
    Restart,
    /// Drain and exit 0: the member turned the service off.
    Shutdown,
}

/// One response line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvcResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SvcResponse {
    pub fn ok(data: impl Serialize) -> Self {
        SvcResponse {
            ok: true,
            data: serde_json::to_value(data).ok(),
            error: None,
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        SvcResponse {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }
}

/// `hello`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SvcHello {
    pub protocol: u32,
    pub app_version: String,
    pub pid: u32,
    pub started_unix_ms: u64,
}

/// `status`'s answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SvcStatus {
    pub node: crate::node::NodeStatus,
    pub ipfs_running: bool,
    /// Why the service has not started the node (the keyring is locked, another process holds
    /// the chain database, the node failed to start). `None` once it is started.
    pub node_hold: Option<String>,
    /// A drain is in effect: nothing starts until `resume` or a restart.
    pub drained: bool,
    pub coinbase: Option<String>,
}

/// `drain`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SvcDrain {
    /// Supervised children still alive when the drain stopped waiting.
    pub still_exiting: Vec<String>,
    pub took_ms: u64,
}

/// The file an update replaces, which is also what the unit runs: the AppImage file itself
/// (`$APPIMAGE`, set by the AppImage runtime; the running executable sits in its read-only
/// mount), else the running executable.
pub fn exe_to_watch(appimage: Option<PathBuf>, current_exe: Option<PathBuf>) -> Option<PathBuf> {
    appimage.filter(|p| p.is_absolute()).or(current_exe)
}

/// Constant-time byte compare (length-equal inputs leak nothing through timing).
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Mint a fresh bearer (hex of [`BEARER_LEN`] bytes from the OS RNG).
pub(crate) fn mint_bearer() -> Zeroizing<String> {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = Zeroizing::new([0u8; BEARER_LEN]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(hex::encode(bytes.as_ref()))
}

/// Why a bearer file is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BearerRefusal {
    Missing,
    NotARegularFile,
    NotOwnedByThisUser,
    ReadableByOthers,
    Malformed,
    Io(String),
}

impl std::fmt::Display for BearerRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BearerRefusal::Missing => write!(f, "no bearer file"),
            BearerRefusal::NotARegularFile => write!(f, "the bearer file is not a regular file"),
            BearerRefusal::NotOwnedByThisUser => {
                write!(f, "the bearer file is not owned by this user")
            }
            BearerRefusal::ReadableByOthers => {
                write!(f, "the bearer file is readable by other users")
            }
            BearerRefusal::Malformed => write!(f, "the bearer file is malformed"),
            BearerRefusal::Io(e) => write!(f, "cannot read the bearer file: {e}"),
        }
    }
}

/// This process's effective uid.
#[cfg(unix)]
pub(crate) fn euid() -> u32 {
    // SAFETY: geteuid takes no arguments, touches no memory and cannot fail (POSIX).
    #[allow(unsafe_code)]
    unsafe {
        libc::geteuid()
    }
}

/// Read the bearer the service wrote, refusing a file another user could have planted or read.
#[cfg(unix)]
pub fn read_bearer(path: &Path) -> Result<Zeroizing<String>, BearerRefusal> {
    use std::os::unix::fs::MetadataExt;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(BearerRefusal::Missing),
        Err(e) => return Err(BearerRefusal::Io(e.kind().to_string())),
    };
    if !meta.file_type().is_file() {
        return Err(BearerRefusal::NotARegularFile);
    }
    if meta.uid() != euid() {
        return Err(BearerRefusal::NotOwnedByThisUser);
    }
    if meta.mode() & 0o077 != 0 {
        return Err(BearerRefusal::ReadableByOthers);
    }
    let raw = Zeroizing::new(
        std::fs::read_to_string(path).map_err(|e| BearerRefusal::Io(e.kind().to_string()))?,
    );
    let token = raw.trim();
    if token.len() != BEARER_LEN * 2 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BearerRefusal::Malformed);
    }
    Ok(Zeroizing::new(token.to_string()))
}

/// A connection to a running service (one request per call, a fresh stream each time, so a
/// service restart between calls is seen as such).
#[derive(Clone)]
pub struct SvcClient {
    socket: PathBuf,
    bearer: Zeroizing<String>,
    pub hello: SvcHello,
}

impl std::fmt::Debug for SvcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the bearer.
        f.debug_struct("SvcClient")
            .field("socket", &self.socket)
            .field("hello", &self.hello)
            .finish()
    }
}

/// The app's decision: talk to the service, or run in-process.
#[derive(Debug, Clone)]
pub enum Link {
    Attached(SvcClient),
    InProcess(String),
}

/// One request over a fresh connection.
#[cfg(unix)]
fn round_trip(
    socket: &Path,
    bearer: &str,
    op: SvcOp,
    timeout: Duration,
) -> Result<SvcResponse, String> {
    use std::io::Read;
    use std::os::unix::net::UnixStream;
    let stream = UnixStream::connect(socket).map_err(|e| format!("connect: {}", e.kind()))?;
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|_| stream.set_write_timeout(Some(timeout)))
        .map_err(|e| format!("socket timeout: {}", e.kind()))?;
    let req = SvcRequest {
        v: PROTOCOL_VERSION,
        bearer: citrate_core_kit::secret::SecretString::new(bearer.to_string()),
        op,
    };
    let mut line = Zeroizing::new(serde_json::to_string(&req).map_err(|e| e.to_string())?);
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|e| format!("write: {}", e.kind()))?;
    let mut reader = BufReader::new(&stream).take(MAX_LINE as u64);
    let mut out = String::new();
    reader
        .read_line(&mut out)
        .map_err(|e| format!("read: {}", e.kind()))?;
    if out.is_empty() {
        return Err("the service closed the connection".to_string());
    }
    serde_json::from_str(&out).map_err(|e| format!("bad response: {e}"))
}

/// Decide the link: attached only to a live service that proves it holds the bearer this user's
/// service wrote and speaks [`PROTOCOL_VERSION`].
#[cfg(unix)]
pub fn attach(paths: &SvcPaths) -> Link {
    match std::fs::symlink_metadata(&paths.socket) {
        Ok(_) => {}
        Err(_) => return Link::InProcess("the background service is not running".to_string()),
    }
    let bearer = match read_bearer(&paths.bearer) {
        Ok(b) => b,
        Err(r) => return Link::InProcess(format!("background service not attached: {r}")),
    };
    let resp = match round_trip(&paths.socket, &bearer, SvcOp::Hello, ATTACH_TIMEOUT) {
        Ok(r) => r,
        Err(e) => return Link::InProcess(format!("background service not answering ({e})")),
    };
    if !resp.ok {
        return Link::InProcess(format!(
            "background service refused: {}",
            resp.error.unwrap_or_default()
        ));
    }
    let hello: SvcHello = match resp.data.map(serde_json::from_value) {
        Some(Ok(h)) => h,
        _ => return Link::InProcess("background service sent a bad hello".to_string()),
    };
    if hello.protocol != PROTOCOL_VERSION {
        return Link::InProcess(format!(
            "background service speaks protocol {} (this app: {PROTOCOL_VERSION})",
            hello.protocol
        ));
    }
    Link::Attached(SvcClient {
        socket: paths.socket.clone(),
        bearer,
        hello,
    })
}

/// Windows: no service yet (see the module doc).
#[cfg(not(unix))]
pub fn attach(_paths: &SvcPaths) -> Link {
    Link::InProcess("the background service is not available on this platform yet".to_string())
}

#[cfg(unix)]
impl SvcClient {
    /// Send one operation; `Err` carries the service's error or the transport failure.
    pub fn call(&self, op: SvcOp) -> Result<Option<serde_json::Value>, String> {
        let resp = round_trip(&self.socket, &self.bearer, op, REQUEST_TIMEOUT)?;
        if resp.ok {
            Ok(resp.data)
        } else {
            Err(resp
                .error
                .unwrap_or_else(|| "the service refused".to_string()))
        }
    }

    pub fn status(&self) -> Result<SvcStatus, String> {
        let v = self.call(SvcOp::Status)?.ok_or("empty status")?;
        serde_json::from_value(v).map_err(|e| format!("bad status: {e}"))
    }

    pub fn drain(&self) -> Result<SvcDrain, String> {
        let v = self.call(SvcOp::Drain)?.ok_or("empty drain report")?;
        serde_json::from_value(v).map_err(|e| format!("bad drain report: {e}"))
    }
}

#[cfg(not(unix))]
impl SvcClient {
    pub fn call(&self, _op: SvcOp) -> Result<Option<serde_json::Value>, String> {
        Err("the background service is not available on this platform yet".to_string())
    }
    pub fn status(&self) -> Result<SvcStatus, String> {
        Err("the background service is not available on this platform yet".to_string())
    }
    pub fn drain(&self) -> Result<SvcDrain, String> {
        Err("the background service is not available on this platform yet".to_string())
    }
}

/// The app's view of the service, cached briefly so the 2 s status poll does not open a socket
/// and read the bearer on every tick. Managed as Tauri state.
pub struct SvcLinkState {
    paths: Option<SvcPaths>,
    cache: std::sync::Mutex<Option<(Link, std::time::Instant)>>,
}

/// How long one attach decision is reused.
pub const LINK_TTL: Duration = Duration::from_secs(5);

impl SvcLinkState {
    pub fn new(paths: Option<SvcPaths>) -> Self {
        SvcLinkState {
            paths,
            cache: std::sync::Mutex::new(None),
        }
    }

    /// The current link, re-decided at most every [`LINK_TTL`].
    pub fn link(&self) -> Link {
        let Some(paths) = &self.paths else {
            return Link::InProcess("no app data directory".to_string());
        };
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((link, at)) = guard.as_ref() {
            if at.elapsed() < LINK_TTL {
                return link.clone();
            }
        }
        let link = attach(paths);
        *guard = Some((link.clone(), std::time::Instant::now()));
        link
    }

    /// The attached client, if any.
    pub fn client(&self) -> Option<SvcClient> {
        match self.link() {
            Link::Attached(c) => Some(c),
            Link::InProcess(_) => None,
        }
    }

    /// Forget the cached decision (after a call failed, or the member changed the setting).
    pub fn invalidate(&self) {
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The attached service from a live app, if any (none when the state is not managed).
pub(crate) fn attached_client<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<SvcClient> {
    use tauri::Manager;
    app.try_state::<SvcLinkState>().and_then(|s| s.client())
}

/// Forget the cached link decision of a live app.
pub(crate) fn invalidate_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(s) = app.try_state::<SvcLinkState>() {
        s.invalidate();
    }
}

/// At app start: a service still running another app version (an update replaced the app
/// while it ran, drained or not) is told to restart so the unit manager starts this version.
/// Returns whether it was told.
pub(crate) fn restart_if_stale(client: &SvcClient, app_version: &str) -> bool {
    if client.hello.app_version == app_version {
        return false;
    }
    match client.call(SvcOp::Restart) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[svc] could not restart the background service: {e}");
            false
        }
    }
}

/// Build the link state from the app data dir.
pub(crate) fn build_link_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> SvcLinkState {
    use tauri::Manager;
    SvcLinkState::new(app.path().app_data_dir().ok().map(SvcPaths::under))
}

#[cfg(test)]
#[path = "svc_tests.rs"]
mod tests;
