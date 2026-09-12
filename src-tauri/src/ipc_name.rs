//! Cross-platform local-IPC endpoint naming (issue #46).
//!
//! The three sidecar clients (`memory.rs`, `cluster.rs`, `comms.rs`) speak a
//! newline-delimited JSON-RPC framing over a LOCAL socket to their daemons. The
//! transport used to be `std::os::unix::net::UnixStream`, which does not exist on
//! Windows. This module centralises the ONE rule that turns the unix-socket path
//! string both ends already share into an [`interprocess`] local-socket [`Name`],
//! so the desktop app builds and runs on Windows while staying byte-identical on
//! Unix.
//!
//! ## The rule (must match the daemon side byte-for-byte)
//! Both the client (here) and the daemon derive the endpoint from the SAME unix
//! socket path string `P`:
//! - **Unix**: the filesystem path `P` unchanged (`GenericFilePath`). An
//!   interprocess `Stream::connect` over a `GenericFilePath` name wraps exactly a
//!   `UnixStream::connect(P)`, and a `ListenerOptions` over it wraps exactly a
//!   `UnixListener::bind(P)` — so the on-wire behaviour (same `AF_UNIX` path) is
//!   identical to the pre-port code.
//! - **Windows**: a namespaced pipe (`GenericNamespaced`) whose name is the
//!   basename of `P` with every char outside `[A-Za-z0-9._-]` replaced by `-`
//!   (e.g. `".../memory/memdag.sock"` -> `"memdag.sock"`). A namespaced name maps
//!   to `\\.\pipe\<name>`; the sanitisation keeps it a legal single pipe segment.
//!
//! `interprocess`' `local_socket::Stream` implements `Read`/`Write` (by value and
//! by `&`), `TryClone` (-> `UnixStream::try_clone` on unix), and the `Stream`
//! trait's `set_recv_timeout`/`set_send_timeout` (-> `UnixStream::set_read_timeout`
//! / `set_write_timeout` on unix), so the existing `BufReader`/`read_line`/
//! `write_all` framing is reused unchanged — only the stream TYPE changes.

use std::io;

use interprocess::local_socket::{prelude::*, Name};
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

/// The cross-platform local-socket stream type used by every sidecar client. On
/// Unix it wraps a `UnixStream`; on Windows a named pipe. Re-exported so the
/// client modules name one type instead of the unix-only `UnixStream`.
pub use interprocess::local_socket::Stream as IpcStream;

/// Turn the shared unix-socket path string `p` into the interprocess [`Name`] both
/// ends agree on. See the module docs for the exact rule.
///
/// Unix keeps `p` verbatim (`GenericFilePath`); Windows uses the sanitised
/// basename (`GenericNamespaced`). The returned `Name` owns its data (`'static`).
pub fn endpoint_name(p: &str) -> io::Result<Name<'static>> {
    #[cfg(unix)]
    {
        p.to_string().to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        let base = std::path::Path::new(p)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("citrate.sock");
        let slug: String = base
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        slug.to_ns_name::<GenericNamespaced>()
    }
}

/// Connect to the local sidecar endpoint identified by the unix-socket path `p`.
///
/// A thin wrapper over `IpcStream::connect(endpoint_name(p)?)` used at every
/// client connect site so the naming rule lives in exactly one place. The caller
/// still applies timeouts / cloning / framing on the returned stream, exactly as
/// it did with `UnixStream`.
pub fn connect(p: &str) -> io::Result<IpcStream> {
    IpcStream::connect(endpoint_name(p)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_name_is_the_path_verbatim() {
        // On unix the endpoint name is the filesystem path unchanged, so it stays
        // on-wire identical to `UnixStream::connect(path)`.
        let n = endpoint_name("/tmp/citrate/memory/memdag.sock")
            .expect("fs name builds on unix");
        // A Name has no public getter for its bytes, but building it must succeed
        // and Debug must reflect the path we handed in (round-trip smoke).
        let dbg = format!("{n:?}");
        #[cfg(unix)]
        assert!(
            dbg.contains("memdag.sock"),
            "unix name must carry the path: {dbg}"
        );
    }

    #[test]
    fn empty_and_odd_paths_still_build_a_name() {
        // A degenerate path must not panic — it returns a Name or an honest error.
        let _ = endpoint_name("");
        let _ = endpoint_name("relative-name.sock");
        let _ = endpoint_name("/a/b/c/d.sock");
    }
}
