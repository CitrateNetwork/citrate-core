//! The agent child process: spawned with exactly the caller's environment, as the leader of its own
//! process group (so a kill reaches `npx`/`node` grandchildren too), with stderr kept in a bounded
//! ring that is never logged, and stdio wrapped so every ACP frame Core writes is scanned for
//! credentials first.

use futures::io::{AsyncRead, AsyncWrite};
use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// Core's credential detector (in the app: the same key-shape check `ensure_keyless` uses).
pub type SecretScan = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    ToAgent,
    FromAgent,
}

/// An optional observer of every ACP line (transcripts in the probe harness and tests).
pub type WireTap = Arc<dyn Fn(Direction, &str) + Send + Sync>;

/// How much agent stderr is kept (the newest bytes).
pub const STDERR_CAP: usize = 64 * 1024;

/// Refuses an environment or argument list that carries a credential shape.
pub fn check_launch(
    env: &[(String, String)],
    args: &[String],
    scan: &SecretScan,
) -> Result<(), String> {
    if let Some((k, _)) = env.iter().find(|(k, v)| scan(k) || scan(v)) {
        return Err(format!(
            "refused: the environment variable {k} carries a credential shape"
        ));
    }
    if args.iter().any(|a| scan(a)) {
        return Err("refused: an agent argument carries a credential shape".into());
    }
    Ok(())
}

/// Spawn `command args` in `cwd` with `env` as its whole environment, in a new process group.
pub fn spawn(
    command: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: &Path,
) -> io::Result<tokio::process::Child> {
    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    cmd.spawn()
}

/// Signal the agent's whole process group (the child is its leader). No-op off Unix, where the
/// child handle's kill is the fallback.
pub fn signal_group(pid: u32, kill: bool) {
    #[cfg(unix)]
    if let Ok(pgid) = libc::pid_t::try_from(pid) {
        if pgid > 0 {
            // SAFETY: killpg only sends a signal; an error means the group is already gone.
            unsafe { libc::killpg(pgid, if kill { libc::SIGKILL } else { libc::SIGTERM }) };
        }
    }
    #[cfg(not(unix))]
    let _ = (pid, kill);
}

/// The newest [`STDERR_CAP`] bytes of the agent's stderr.
#[derive(Default)]
pub struct StderrRing(VecDeque<u8>);

impl StderrRing {
    pub fn push(&mut self, b: &[u8]) {
        let b = &b[b.len().saturating_sub(STDERR_CAP)..];
        let over = (self.0.len() + b.len()).saturating_sub(STDERR_CAP);
        self.0.drain(..over);
        self.0.extend(b);
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.iter().copied().collect::<Vec<u8>>()).into_owned()
    }
}

/// Drains the child's stderr into `ring` until EOF.
pub async fn pump_stderr(mut err: tokio::process::ChildStderr, ring: Arc<Mutex<StderrRing>>) {
    use tokio::io::AsyncReadExt as _;
    let mut buf = [0u8; 8192];
    while let Ok(n) = err.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if let Ok(mut r) = ring.lock() {
            r.push(&buf[..n]);
        }
    }
}

/// The writer to the agent: buffers each line, refuses it if `scan` flags it, and only then
/// forwards it.
pub struct ScannedWriter<W> {
    inner: W,
    line: Vec<u8>,
    out: Vec<u8>,
    scan: SecretScan,
    tap: Option<WireTap>,
}

impl<W> ScannedWriter<W> {
    pub fn new(inner: W, scan: SecretScan, tap: Option<WireTap>) -> Self {
        ScannedWriter {
            inner,
            line: Vec::new(),
            out: Vec::new(),
            scan,
            tap,
        }
    }
}

impl<W: AsyncWrite + Unpin> ScannedWriter<W> {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while !self.out.is_empty() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.out) {
                Poll::Ready(Ok(0)) => return Poll::Ready(Err(io::ErrorKind::WriteZero.into())),
                Poll::Ready(Ok(n)) => drop(self.out.drain(..n)),
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ScannedWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        if this.drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        this.line.extend_from_slice(buf);
        while let Some(i) = this.line.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = this.line.drain(..=i).collect();
            let text = String::from_utf8_lossy(&line[..i]);
            if (this.scan)(&text) {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "refused: an outbound ACP frame carried a credential shape",
                )));
            }
            if let Some(tap) = &this.tap {
                tap(Direction::ToAgent, &text);
            }
            this.out.extend_from_slice(&line);
        }
        let _ = this.drain(cx)?;
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.drain(cx)?.is_pending() {
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

/// The reader from the agent: passes bytes through unchanged and shows each line to the tap.
pub struct TappedReader<R> {
    inner: R,
    line: Vec<u8>,
    tap: Option<WireTap>,
}

impl<R> TappedReader<R> {
    pub fn new(inner: R, tap: Option<WireTap>) -> Self {
        TappedReader {
            inner,
            line: Vec::new(),
            tap,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for TappedReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let n = match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(n)) => n,
            other => return other,
        };
        if let Some(tap) = &this.tap {
            this.line.extend_from_slice(&buf[..n]);
            while let Some(i) = this.line.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = this.line.drain(..=i).collect();
                tap(Direction::FromAgent, &String::from_utf8_lossy(&line[..i]));
            }
            if this.line.len() > 16 << 20 {
                this.line.clear();
            }
        }
        Poll::Ready(Ok(n))
    }
}
