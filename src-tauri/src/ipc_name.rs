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
//! - **Windows**: a namespaced pipe (`GenericNamespaced`) with a per-user,
//!   per-install pipe name (PBA-L7b-004). The name is
//!   `citrate-<hex(sha256("citrate/ipc-pipe/v1" || P || NUL || nonce))[..32]>-<slug>`
//!   where `nonce` is 32 random bytes kept in the owner-only file `P.pipe-nonce`
//!   (created with `create_new` on first use, inside the member's per-user data
//!   dir) and `slug` is the sanitised basename of `P` (chars outside
//!   `[A-Za-z0-9._-]` become `-`). `P` itself is per-user (it lives under the
//!   member's app-data dir). See [`windows_pipe_name`] (a pure fn, unit-tested on
//!   every platform).
//!
//! `interprocess`' `local_socket::Stream` implements `Read`/`Write` (by value and
//! by `&`), `TryClone` (-> `UnixStream::try_clone` on unix), and the `Stream`
//! trait's `set_recv_timeout`/`set_send_timeout` (-> `UnixStream::set_read_timeout`
//! / `set_write_timeout` on unix), so the existing `BufReader`/`read_line`/
//! `write_all` framing is reused unchanged — only the stream TYPE changes.

use std::io;

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{prelude::*, Name};

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
        let nonce = pipe_nonce(p)?;
        windows_pipe_name(p, &nonce).to_ns_name::<GenericNamespaced>()
    }
}

/// PBA-L7b-004: the length of the per-install pipe nonce.
pub const PIPE_NONCE_LEN: usize = 32;

/// The owner-only nonce file for the endpoint `p` (`<p>.pipe-nonce`).
#[cfg_attr(unix, allow(dead_code))]
pub fn pipe_nonce_path(p: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{p}.pipe-nonce"))
}

/// Read the per-install pipe nonce for `p`, creating it (32 random bytes, `create_new`, 0600 on
/// unix; inherits the per-user profile ACL on Windows) on first use. A concurrent creator wins
/// the `create_new` race and the loser reads the winner's nonce, so both ends always agree.
#[cfg_attr(unix, allow(dead_code))]
pub fn pipe_nonce(p: &str) -> io::Result<[u8; PIPE_NONCE_LEN]> {
    use std::io::{Read, Write};
    let path = pipe_nonce_path(p);
    let read = |path: &std::path::Path| -> io::Result<[u8; PIPE_NONCE_LEN]> {
        let mut buf = Vec::with_capacity(PIPE_NONCE_LEN);
        std::fs::File::open(path)?
            .take(PIPE_NONCE_LEN as u64 + 1)
            .read_to_end(&mut buf)?;
        buf.try_into().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "pipe nonce file has the wrong length",
            )
        })
    };
    match read(&path) {
        Ok(n) => return Ok(n),
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        Err(_) => {}
    }
    let mut nonce = [0u8; PIPE_NONCE_LEN];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut f) => {
            f.write_all(&nonce)?;
            Ok(nonce)
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read(&path),
        Err(e) => Err(e),
    }
}

/// PBA-L7b-004: the Windows pipe name for endpoint path `p` and per-install `nonce` (pure; see
/// the module docs). The per-user component is a domain-separated SHA-256 over the per-user path
/// and the secret nonce, so two accounts (or two installs) never share a name and another account
/// cannot predict it.
#[cfg_attr(unix, allow(dead_code))]
pub fn windows_pipe_name(p: &str, nonce: &[u8]) -> String {
    use sha2::{Digest, Sha256};
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
    let mut h = Sha256::new();
    h.update(b"citrate/ipc-pipe/v1");
    h.update(p.as_bytes());
    h.update([0u8]);
    h.update(nonce);
    let tag = hex::encode(&h.finalize()[..16]);
    format!("citrate-{tag}-{slug}")
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
        let n = endpoint_name("/tmp/citrate/memory/memdag.sock").expect("fs name builds on unix");
        // A Name has no public getter for its bytes, but building it must succeed
        // and Debug must reflect the path we handed in (round-trip smoke).
        let dbg = format!("{n:?}");
        #[cfg(unix)]
        assert!(
            dbg.contains("memdag.sock"),
            "unix name must carry the path: {dbg}"
        );
    }

    /// PBA-L7b-004 tripwire: the Windows pipe name carries a per-user, per-install component, so
    /// two accounts (or two installs) never share a pipe name.
    #[test]
    fn pba_l7b_004_windows_pipe_name_is_per_user_and_per_install() {
        let alice = r"C:\Users\alice\AppData\Roaming\ai.citrate.core\comms\member.sock";
        let bob = r"C:\Users\bob\AppData\Roaming\ai.citrate.core\comms\member.sock";
        // Fresh random nonces (as production mints them), not hard-coded values.
        let n1: [u8; PIPE_NONCE_LEN] = rand::random();
        let mut n2: [u8; PIPE_NONCE_LEN] = rand::random();
        while n2 == n1 {
            n2 = rand::random();
        }
        let a = windows_pipe_name(alice, &n1);
        // Not the old machine-global basename.
        assert_ne!(a, "member.sock");
        assert!(a.len() > "member.sock".len() + 16, "{a}");
        // Two users → two names, even with the same nonce.
        assert_ne!(a, windows_pipe_name(bob, &n1));
        // Same path, different install nonce → different name (unguessable without the nonce).
        assert_ne!(a, windows_pipe_name(alice, &n2));
        // Deterministic for both ends (client + daemon derive the same name).
        assert_eq!(a, windows_pipe_name(alice, &n1));
        // Still a single legal pipe segment.
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'));
        assert!(a.ends_with("-member.sock"));
        // The basename sanitisation keeps [A-Za-z0-9._-] and maps anything else to '-'.
        let odd = windows_pipe_name(r"C:\u\a_b.c-d e$f.sock", &n1);
        assert!(odd.ends_with("-a_b.c-d-e-f.sock"), "{odd}");
    }

    /// The nonce is created once (owner-only) and then read back identically by every caller.
    #[test]
    fn pba_l7b_004_pipe_nonce_is_stable_and_owner_only() {
        let dir = std::env::temp_dir().join(format!("citrate-ipc-nonce-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("member.sock");
        let p = p.to_string_lossy().to_string();
        let a = pipe_nonce(&p).expect("create");
        let b = pipe_nonce(&p).expect("read back");
        assert_eq!(a, b);
        assert_ne!(a, [0u8; PIPE_NONCE_LEN]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(pipe_nonce_path(&p))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "nonce file must be owner-only ({mode:o})");
        }
        // A corrupted (wrong-length) nonce fails closed rather than silently regenerating.
        std::fs::write(pipe_nonce_path(&p), b"short").unwrap();
        assert!(pipe_nonce(&p).is_err());
        // Too long is also corrupt (never silently truncated to 32 bytes).
        std::fs::write(pipe_nonce_path(&p), [1u8; PIPE_NONCE_LEN + 1]).unwrap();
        assert!(pipe_nonce(&p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_and_odd_paths_still_build_a_name() {
        // A degenerate path must not panic — it returns a Name or an honest error.
        let _ = endpoint_name("");
        let _ = endpoint_name("relative-name.sock");
        let _ = endpoint_name("/a/b/c/d.sock");
    }
}
