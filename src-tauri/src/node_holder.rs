//! Who is holding the node's chain database or ports (SCL-S0.6, SCL-S8.5a).
//!
//! After an update that changes the genesis (the 40204 reroll), the new build deletes the old
//! chain database before it starts its node ([`crate::node_genesis`]). An older node left
//! running by the previous version (not quit, crashed out of its parent, or started by another
//! copy of the app) can still hold that database open, or the node's ports. Before this module
//! the reset refused only when a node **answered** on the local RPC; a starting or wedged node
//! that held the database without answering passed that check (red-team RT-11), and an orphan
//! holding the ports made the new node fail silently.
//!
//! This module answers two questions before any node start, and the reset asks the first one
//! again while it deletes:
//!
//! 1. **Can the chain database lock be taken?** RocksDB guards its database directory with the
//!    `LOCK` file. On macOS and Linux it takes a POSIX record lock on it
//!    (`fcntl(F_SETLK)`, write lock, whole file); on Windows it opens the file with no sharing.
//!    [`acquire_chain_db_lock`] does the same, so it conflicts with a live RocksDB holder, and
//!    on macOS and Linux it asks the kernel for the holder's pid (`F_GETLK`).
//! 2. **Is a node port already in use?** The ports come from the `node.toml` the node is
//!    launched with ([`node_ports_from_config`]). A port counts as in use when a loopback TCP
//!    connect succeeds or a loopback bind fails with "address in use". The holder's pid comes
//!    from `lsof` (macOS, Linux) or `netstat -ano` (Windows) when the tool is present.
//!
//! When either is held, the start is refused and the UI is told which process holds what
//! ([`NodeBlocker`]), with the action to take. Nothing is deleted and nothing is signalled:
//! this module only looks.
//!
//! ## Residual windows (stated, not closed)
//! - The lock is advisory. It excludes RocksDB openers, not a process that writes the files
//!   without taking it.
//! - The reset deletes `LOCK` itself (it is a RocksDB file; the set of deleted files is
//!   unchanged), and deletes it **last**. From that unlink until our handle closes, a new
//!   opener can create a fresh `LOCK` and take it; on macOS and Linux the lock we hold is on
//!   the unlinked file. The window is the last unlink plus the marker write, and only a
//!   process that starts a node in that instant (not one already running) can use it.
//! - Between this check and our own node's spawn, another process can still take the lock or
//!   a port; our node then fails to open and the next start names the holder.
//! - Windows does not report which process holds a file, so a Windows database holder is
//!   named as unidentified; a port holder is named when `netstat` is available.
//! - Only TCP ports are probed.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// RocksDB's lock file in the database directory (the node data dir).
pub const CHAIN_DB_LOCK_FILE: &str = "LOCK";

/// How long the loopback connect probe waits per port.
const PORT_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// The process holding a resource, as far as the OS will say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolderProcess {
    /// The holder's process id, when the OS reports it.
    pub pid: Option<u32>,
    /// The holder's executable path, when it can be read.
    pub path: Option<String>,
}

impl HolderProcess {
    /// Look up the executable path for `pid` (best effort).
    pub fn from_pid(pid: Option<u32>) -> Self {
        HolderProcess {
            pid,
            path: pid.and_then(exe_path_of),
        }
    }

    /// True when the holder's executable is a Citrate node binary (`citrate`, `citrate.exe`, or
    /// a target-suffixed dev build such as `citrate-aarch64-apple-darwin`).
    pub fn is_citrate_node(&self) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let base = base.strip_suffix(".exe").unwrap_or(base);
        if base == "citrate" {
            return true;
        }
        // A dev build keeps Tauri's target-triple suffix, e.g. `citrate-aarch64-apple-darwin`.
        base.strip_prefix("citrate-").is_some_and(|triple| {
            ["-apple-", "-linux-", "-windows-", "-pc-"]
                .iter()
                .any(|os| triple.contains(os))
        })
    }

    fn describe(&self) -> String {
        match (self.pid, self.path.as_deref()) {
            (Some(pid), Some(path)) => format!("process {pid}, {path}"),
            (Some(pid), None) => format!("process {pid}"),
            (None, Some(path)) => path.to_string(),
            (None, None) => "a process the system did not identify".to_string(),
        }
    }

    fn who(&self) -> &'static str {
        if self.is_citrate_node() {
            "An older Citrate node is still running"
        } else {
            "Another process is running"
        }
    }
}

/// What the node needs and someone else holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockedResource {
    /// The chain database lock (`LOCK` in the node data dir).
    ChainDatabase,
    /// A TCP port from the node's config.
    Port,
}

/// Why the node was not started (or the chain data not reset), shown to the member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeBlocker {
    pub resource: BlockedResource,
    /// The port, for [`BlockedResource::Port`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The one message the UI shows: who holds what, and what to do.
    pub message: String,
}

const ACTION: &str = "The node was not started and no chain data was deleted. Quit that \
                      process (or restart your computer), then start the node again.";

impl NodeBlocker {
    /// The chain database lock is held by `holder`.
    pub fn chain_database(holder: HolderProcess, data_dir: &Path) -> Self {
        let message = format!(
            "{} ({}) and holds the chain database in {}. {ACTION}",
            holder.who(),
            holder.describe(),
            data_dir.display()
        );
        NodeBlocker {
            resource: BlockedResource::ChainDatabase,
            port: None,
            pid: holder.pid,
            path: holder.path,
            message,
        }
    }

    /// `port` is in use by `holder`.
    pub fn port(port: u16, holder: HolderProcess) -> Self {
        let message = format!(
            "{} ({}) and is using port {port}, which the Citrate node needs. {ACTION}",
            holder.who(),
            holder.describe(),
        );
        NodeBlocker {
            resource: BlockedResource::Port,
            port: Some(port),
            pid: holder.pid,
            path: holder.path,
            message,
        }
    }
}

impl std::fmt::Display for NodeBlocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Why the chain database lock could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbLockError {
    /// Another process holds it.
    Held(HolderProcess),
    /// The lock file could not be opened or locked for another reason.
    Io(String),
}

/// The chain database lock, held until dropped. Holds nothing when the data dir has no `LOCK`
/// file: RocksDB creates that file when it opens the database, so no live RocksDB holder can be
/// missed by that (the file is never created here, so the set of files is unchanged).
#[derive(Debug)]
pub struct ChainDbLock {
    /// The locked `LOCK` file. Never read: holding it holds the lock, and dropping it (closing
    /// the descriptor or handle) releases it.
    _held: Option<std::fs::File>,
}

impl ChainDbLock {
    /// True when a `LOCK` file existed and is now locked by this process.
    #[cfg(test)]
    pub fn holds_file(&self) -> bool {
        self._held.is_some()
    }
}

/// Take the chain database lock in `data_dir` the way RocksDB does, without blocking.
pub fn acquire_chain_db_lock(data_dir: &Path) -> Result<ChainDbLock, DbLockError> {
    let path = data_dir.join(CHAIN_DB_LOCK_FILE);
    lock_file(&path).map(|held| ChainDbLock { _held: held })
}

#[cfg(unix)]
fn lock_file(path: &Path) -> Result<Option<std::fs::File>, DbLockError> {
    use std::os::unix::io::AsRawFd;
    // Write access is required for a write lock.
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(DbLockError::Io(format!("open {}: {e}", path.display()))),
    };
    let fd = file.as_raw_fd();
    let mut req = whole_file_write_lock();
    // SAFETY: `fd` is a valid open descriptor owned by `file` for this call, and `req` is a
    // fully initialised `flock` that outlives it.
    let rc = unsafe { libc::fcntl(fd, libc::F_SETLK, &req) };
    if rc == 0 {
        return Ok(Some(file));
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(code) if code == libc::EAGAIN || code == libc::EACCES => {
            // Ask who holds the conflicting lock.
            // SAFETY: as above; F_GETLK writes into `req`, which we own mutably.
            let rc = unsafe { libc::fcntl(fd, libc::F_GETLK, &mut req) };
            let pid =
                if rc == 0 && i64::from(req.l_type) != i64::from(libc::F_UNLCK) && req.l_pid > 0 {
                    u32::try_from(req.l_pid).ok()
                } else {
                    None
                };
            Err(DbLockError::Held(HolderProcess::from_pid(pid)))
        }
        _ => Err(DbLockError::Io(format!("lock {}: {err}", path.display()))),
    }
}

#[cfg(unix)]
fn whole_file_write_lock() -> libc::flock {
    // SAFETY: `flock` is a plain C struct; all-zero is a valid value for every field.
    let mut req: libc::flock = unsafe { std::mem::zeroed() };
    req.l_type = libc::F_WRLCK as _;
    req.l_whence = libc::SEEK_SET as _;
    req.l_start = 0;
    req.l_len = 0; // to end of file, whatever its size
    req
}

#[cfg(windows)]
fn lock_file(path: &Path) -> Result<Option<std::fs::File>, DbLockError> {
    use std::os::windows::fs::OpenOptionsExt;
    // Share delete only: the reset deletes LOCK while this handle is open. RocksDB opens LOCK
    // with no sharing, so either side's open fails while the other holds it.
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_DELETE)
        .open(path)
    {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
            ) =>
        {
            // Windows does not say which process holds a file.
            Err(DbLockError::Held(HolderProcess::from_pid(None)))
        }
        Err(e) => Err(DbLockError::Io(format!("open {}: {e}", path.display()))),
    }
}

#[cfg(not(any(unix, windows)))]
fn lock_file(_path: &Path) -> Result<Option<std::fs::File>, DbLockError> {
    Err(DbLockError::Io(
        "no chain database lock support on this platform".to_string(),
    ))
}

/// The executable path of `pid`, when it can be read.
#[cfg(target_os = "linux")]
fn exe_path_of(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(target_os = "macos")]
fn exe_path_of(pid: u32) -> Option<String> {
    let pid = i32::try_from(pid).ok()?;
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: `buf` is a writable buffer of exactly the length passed.
    let n = unsafe {
        libc::proc_pidpath(
            pid,
            buf.as_mut_ptr().cast::<libc::c_void>(),
            u32::try_from(buf.len()).ok()?,
        )
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(usize::try_from(n).ok()?);
    Some(String::from_utf8_lossy(&buf).to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exe_path_of(_pid: u32) -> Option<String> {
    None
}

/// The TCP ports a node launched with `config` listens on: `[rpc] listen_addr`, `[rpc] ws_addr`
/// and `[network] listen_addr`, deduplicated, in that order. Unparsable entries are skipped.
pub fn node_ports_from_config(config: &str) -> Vec<u16> {
    let Ok(value) = config.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for (section, key) in [
        ("rpc", "listen_addr"),
        ("rpc", "ws_addr"),
        ("network", "listen_addr"),
    ] {
        let port = value
            .get(section)
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_str())
            .and_then(|a| a.parse::<std::net::SocketAddr>().ok())
            .map(|a| a.port());
        if let Some(p) = port {
            if p != 0 && !ports.contains(&p) {
                ports.push(p);
            }
        }
    }
    ports
}

/// True when something already listens on `port` on this machine.
pub fn port_in_use(port: u16) -> bool {
    let loopback = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    if std::net::TcpStream::connect_timeout(&loopback, PORT_PROBE_TIMEOUT).is_ok() {
        return true;
    }
    matches!(
        std::net::TcpListener::bind(loopback),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse
    )
}

/// The pid listening on TCP `port`, when the OS tool that reports it is present.
fn port_holder_pid(port: u16) -> Option<u32> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("lsof")
            .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|l| l.trim().parse::<u32>().ok())
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .output()
            .ok()?;
        parse_netstat_listener(&String::from_utf8_lossy(&out.stdout), port)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = port;
        None
    }
}

/// The pid of the TCP listener on `port` in `netstat -ano` output (Windows format:
/// `TCP  127.0.0.1:8545  0.0.0.0:0  LISTENING  1234`).
#[cfg_attr(not(windows), allow(dead_code))] // used on Windows; tested everywhere
pub fn parse_netstat_listener(output: &str, port: u16) -> Option<u32> {
    output.lines().find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("TCP") {
            return None;
        }
        if !cols[3].eq_ignore_ascii_case("LISTENING") {
            return None;
        }
        let local_port = cols[1].rsplit(':').next()?.parse::<u16>().ok()?;
        if local_port != port {
            return None;
        }
        cols[4].parse::<u32>().ok()
    })
}

/// The first holder of a node resource, checked in order: the chain database lock in
/// `data_dir`, then each of `ports`. `Ok(None)` when everything is free. The lock is released
/// again before returning; the reset takes it again for itself.
pub fn find_blocker(data_dir: &Path, ports: &[u16]) -> Result<Option<NodeBlocker>, String> {
    match acquire_chain_db_lock(data_dir) {
        Ok(lock) => drop(lock),
        Err(DbLockError::Held(holder)) => {
            return Ok(Some(NodeBlocker::chain_database(holder, data_dir)))
        }
        Err(DbLockError::Io(e)) => return Err(e),
    }
    for &port in ports {
        if port_in_use(port) {
            let holder = HolderProcess::from_pid(port_holder_pid(port));
            return Ok(Some(NodeBlocker::port(port, holder)));
        }
    }
    Ok(None)
}

/// The node config path inside a node data dir (written by `node::ensure_node_config`).
pub fn node_config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("node.toml")
}

#[cfg(test)]
pub(crate) mod tests {
    include!("node_holder_tests.rs");
}
