//! L7-WP06 (AR-E9.1, F-21): a read-only client for the Tailscale LocalAPI, so Citrate Core can read
//! the tailnet without starting a program. The CLI (`fleet_tailscale::detect_tailscale_cli`) stays
//! the fallback: LocalAPI is not a stable public API.
//!
//! **Read-only by construction.** A request is built only from [`Endpoint`], which has two values:
//! `GET /localapi/v0/status` and `GET /localapi/v0/whois?addr=<ip>`. There is no method, path or body
//! parameter anywhere in this module, so no write endpoint is reachable from it.
//!
//! Where the LocalAPI listens, checked against tailscale/tailscale main d486755f (2026-11-28),
//! `paths/paths.go` `DefaultTailscaledSocket`, `safesocket/safesocket_darwin.go`,
//! `client/local/local.go`, `ipn/localapi/localapi.go`:
//! - Linux: the unix socket `/var/run/tailscale/tailscaled.sock` (`/run/...` on systems without
//!   `/var/run`).
//! - macOS, standalone app (System Extension): loopback TCP; the port is the target of the
//!   symlink `/Library/Tailscale/ipnport`, the token is in `/Library/Tailscale/sameuserproof-<port>`
//!   (mode 0640, group admin), sent as HTTP Basic auth with an empty user. macOS with Homebrew
//!   `tailscaled`: the unix socket `/var/run/tailscaled.socket`. The App Store app keeps its token
//!   in its sandbox container (the CLI finds it with `lsof`); this client does not read other apps'
//!   containers, so that build uses the CLI fallback.
//! - Windows: the named pipe `\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled`.
//! - Every request carries `Host: local-tailscaled.sock` and no `Origin` or `Referer` (the LocalAPI
//!   refuses requests with either).

// `whois` has its first reader in the tailnet auto-fill (L7-WP07); until then it is exercised by
// tests only. `status` is read by `fleet_tailscale::detect_tailscale_via`.
#![cfg_attr(not(test), allow(dead_code))]

use std::fmt;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine as _;

use crate::fleet_tailscale::{parse_status, TailscaleReport};

/// Deadline for one LocalAPI call, the same as the CLI path's.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(4);
/// Largest body read, the same cap as the CLI path's.
pub(crate) const MAX_BODY: usize = 4 * 1024 * 1024;
/// Largest response head read.
const MAX_HEAD: usize = 16 * 1024;
/// The Host header the LocalAPI expects.
pub(crate) const HOST: &str = "local-tailscaled.sock";

// Where each OS's LocalAPI listens (see the module docs). Each OS reads only its own.
pub(crate) const MACSYS_DIR: &str = "/Library/Tailscale";
pub(crate) const MAC_SOCKETS: &[&str] = &["/var/run/tailscaled.socket"];
pub(crate) const LINUX_SOCKETS: &[&str] = &[
    "/var/run/tailscale/tailscaled.sock",
    "/run/tailscale/tailscaled.sock",
];
pub(crate) const WINDOWS_PIPE: &str =
    r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled";

/// The only two things this client can ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Endpoint {
    Status,
    WhoIs(IpAddr),
}

impl Endpoint {
    pub(crate) fn path(&self) -> String {
        match self {
            Endpoint::Status => "/localapi/v0/status".to_string(),
            // An IP's text is digits, hex letters, '.' and ':'; only ':' needs escaping.
            Endpoint::WhoIs(ip) => format!(
                "/localapi/v0/whois?addr={}",
                ip.to_string().replace(':', "%3A")
            ),
        }
    }
}

/// The macOS LocalAPI token. Never printed: its `Debug` is redacted, and no error carries it.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(redacted)")
    }
}

/// How to reach this machine's LocalAPI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transport {
    #[cfg(unix)]
    Unix(PathBuf),
    /// Loopback TCP with a token (macOS standalone app). Always 127.0.0.1.
    Tcp { port: u16, token: Token },
    #[cfg(windows)]
    Pipe(PathBuf),
}

impl Transport {
    fn token(&self) -> Option<&Token> {
        match self {
            Transport::Tcp { token, .. } => Some(token),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Why a LocalAPI read failed. Secret-free; the caller falls back to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalApiError {
    /// No LocalAPI endpoint was found on this machine.
    NotFound,
    /// The endpoint exists but could not be reached (I/O error kind).
    Connect(String),
    /// No answer within the deadline.
    Timeout,
    /// The answer is larger than the cap.
    TooLarge,
    /// The LocalAPI answered with a non-200 status.
    Http(u16),
    /// The answer is not HTTP or not JSON.
    Malformed,
}

impl fmt::Display for LocalApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LocalApiError::NotFound => f.write_str("no Tailscale LocalAPI on this machine"),
            LocalApiError::Connect(k) => write!(f, "could not reach the Tailscale LocalAPI: {k}"),
            LocalApiError::Timeout => f.write_str("the Tailscale LocalAPI did not answer in time"),
            LocalApiError::TooLarge => f.write_str("the Tailscale LocalAPI answer is too large"),
            LocalApiError::Http(c) => write!(f, "the Tailscale LocalAPI answered {c}"),
            LocalApiError::Malformed => {
                f.write_str("the Tailscale LocalAPI answer was not understood")
            }
        }
    }
}

fn io_err(e: std::io::Error) -> LocalApiError {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => LocalApiError::Timeout,
        k => LocalApiError::Connect(k.to_string()),
    }
}

/// The bytes of one request. HTTP/1.0, so the server closes the connection after one answer.
pub(crate) fn request_bytes(ep: &Endpoint, token: Option<&Token>) -> Vec<u8> {
    let mut s = format!(
        "GET {} HTTP/1.0\r\nHost: {HOST}\r\nUser-Agent: citrate-core\r\nConnection: close\r\n",
        ep.path()
    );
    if let Some(Token(t)) = token {
        let basic = base64::engine::general_purpose::STANDARD.encode(format!(":{t}"));
        s.push_str(&format!("Authorization: Basic {basic}\r\n"));
    }
    s.push_str("\r\n");
    s.into_bytes()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, LocalApiError> {
    let mut out = Vec::new();
    loop {
        let eol = find(b, b"\r\n").ok_or(LocalApiError::Malformed)?;
        let line = std::str::from_utf8(&b[..eol]).map_err(|_| LocalApiError::Malformed)?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| LocalApiError::Malformed)?;
        b = &b[eol + 2..];
        if size == 0 {
            return Ok(out);
        }
        if size > MAX_BODY || out.len() + size > MAX_BODY {
            return Err(LocalApiError::TooLarge);
        }
        if b.len() < size + 2 || &b[size..size + 2] != b"\r\n" {
            return Err(LocalApiError::Malformed);
        }
        out.extend_from_slice(&b[..size]);
        b = &b[size + 2..];
    }
}

/// The body of a raw HTTP answer, if it is a 200 within the caps.
pub(crate) fn parse_response(raw: &[u8]) -> Result<Vec<u8>, LocalApiError> {
    let end = find(raw, b"\r\n\r\n").ok_or(LocalApiError::Malformed)?;
    if end > MAX_HEAD {
        return Err(LocalApiError::Malformed);
    }
    let head = std::str::from_utf8(&raw[..end]).map_err(|_| LocalApiError::Malformed)?;
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or("");
    let mut parts = status.split(' ');
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(LocalApiError::Malformed);
    }
    let code: u16 = parts
        .next()
        .and_then(|c| c.parse().ok())
        .ok_or(LocalApiError::Malformed)?;
    let chunked = lines.any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let rest = &raw[end + 4..];
    let body = if chunked {
        dechunk(rest)?
    } else {
        rest.to_vec()
    };
    if body.len() > MAX_BODY {
        return Err(LocalApiError::TooLarge);
    }
    if code != 200 {
        return Err(LocalApiError::Http(code));
    }
    Ok(body)
}

fn roundtrip<S: Read + Write>(mut s: S, req: &[u8]) -> Result<Vec<u8>, LocalApiError> {
    s.write_all(req).map_err(io_err)?;
    s.flush().map_err(io_err)?;
    let cap = (MAX_HEAD + MAX_BODY + 1) as u64;
    let mut raw = Vec::new();
    (&mut s).take(cap).read_to_end(&mut raw).map_err(io_err)?;
    if raw.len() as u64 >= cap {
        return Err(LocalApiError::TooLarge);
    }
    parse_response(&raw)
}

fn exchange(t: &Transport, req: &[u8], timeout: Duration) -> Result<Vec<u8>, LocalApiError> {
    match t {
        #[cfg(unix)]
        Transport::Unix(p) => {
            let s = std::os::unix::net::UnixStream::connect(p).map_err(io_err)?;
            s.set_read_timeout(Some(timeout)).map_err(io_err)?;
            s.set_write_timeout(Some(timeout)).map_err(io_err)?;
            roundtrip(s, req)
        }
        Transport::Tcp { port, .. } => {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), *port);
            let s = TcpStream::connect_timeout(&addr, timeout).map_err(io_err)?;
            s.set_read_timeout(Some(timeout)).map_err(io_err)?;
            s.set_write_timeout(Some(timeout)).map_err(io_err)?;
            roundtrip(s, req)
        }
        #[cfg(windows)]
        Transport::Pipe(p) => {
            let f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(p)
                .map_err(io_err)?;
            roundtrip(f, req)
        }
    }
}

/// One call, answered within `timeout` or not at all. The I/O runs on its own thread, so a pipe
/// that never answers (no read timeout on Windows pipes) cannot hold the caller past the deadline.
pub(crate) fn fetch(
    t: &Transport,
    ep: &Endpoint,
    timeout: Duration,
) -> Result<Vec<u8>, LocalApiError> {
    let req = request_bytes(ep, t.token());
    let t = t.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("ts-localapi".into())
        .spawn(move || {
            let _ = tx.send(exchange(&t, &req, timeout));
        })
        .map_err(io_err)?;
    rx.recv_timeout(timeout)
        .unwrap_or(Err(LocalApiError::Timeout))
}

/// The macOS standalone app's port and token, read from its shared directory.
pub(crate) fn macsys_from(dir: &Path) -> Option<Transport> {
    let port_text = std::fs::read_link(dir.join("ipnport")).ok()?;
    let port: u16 = port_text.to_str()?.parse().ok()?;
    if port == 0 {
        return None;
    }
    let token = std::fs::read_to_string(dir.join(format!("sameuserproof-{port}"))).ok()?;
    let token = token.trim();
    if token.is_empty() || token.len() > 256 || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(Transport::Tcp {
        port,
        token: Token(token.to_string()),
    })
}

#[cfg(unix)]
fn first_socket(paths: &[&str]) -> Option<Transport> {
    paths
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .map(Transport::Unix)
}

/// This machine's LocalAPI endpoint, if one is there.
#[cfg(target_os = "macos")]
pub(crate) fn discover() -> Option<Transport> {
    macsys_from(Path::new(MACSYS_DIR)).or_else(|| first_socket(MAC_SOCKETS))
}

/// This machine's LocalAPI endpoint, if one is there.
#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn discover() -> Option<Transport> {
    first_socket(LINUX_SOCKETS)
}

/// This machine's LocalAPI endpoint. The pipe is opened at the call; a missing pipe is a
/// `Connect` error there, and the caller falls back to the CLI.
#[cfg(windows)]
pub(crate) fn discover() -> Option<Transport> {
    Some(Transport::Pipe(PathBuf::from(WINDOWS_PIPE)))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn discover() -> Option<Transport> {
    None
}

/// `GET /localapi/v0/status` through `t`, parsed exactly as the CLI's `status --json` is.
pub(crate) fn status_via(
    t: &Transport,
    timeout: Duration,
) -> Result<TailscaleReport, LocalApiError> {
    let body = fetch(t, &Endpoint::Status, timeout)?;
    let text = std::str::from_utf8(&body).map_err(|_| LocalApiError::Malformed)?;
    if !text.trim_start().starts_with('{') {
        return Err(LocalApiError::Malformed);
    }
    Ok(parse_status(text))
}

/// This machine's tailnet status from the LocalAPI.
pub(crate) fn status() -> Result<TailscaleReport, LocalApiError> {
    status_via(&discover().ok_or(LocalApiError::NotFound)?, TIMEOUT)
}

/// Who a tailnet address belongs to: the Tailscale login and the node's MagicDNS name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WhoIs {
    pub login: Option<String>,
    pub node: Option<String>,
}

/// Parse a `/localapi/v0/whois` answer. Anything missing is `None`.
pub(crate) fn parse_whois(json: &str) -> Result<WhoIs, LocalApiError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| LocalApiError::Malformed)?;
    let s = |x: Option<&serde_json::Value>| {
        x.and_then(|y| y.as_str())
            .map(|y| y.trim_end_matches('.').to_string())
            .filter(|y| !y.is_empty())
    };
    Ok(WhoIs {
        login: s(v.pointer("/UserProfile/LoginName")),
        node: s(v.pointer("/Node/Name")),
    })
}

/// `GET /localapi/v0/whois?addr=<ip>` through `t`. The first reader is the tailnet auto-fill
/// (L7-WP07), which matches a connecting address to a machine linked under the member.
pub(crate) fn whois_via(
    t: &Transport,
    ip: IpAddr,
    timeout: Duration,
) -> Result<WhoIs, LocalApiError> {
    let body = fetch(t, &Endpoint::WhoIs(ip), timeout)?;
    parse_whois(std::str::from_utf8(&body).map_err(|_| LocalApiError::Malformed)?)
}

#[cfg(test)]
mod tests {
    include!("tailscale_local_tests.rs");
}
