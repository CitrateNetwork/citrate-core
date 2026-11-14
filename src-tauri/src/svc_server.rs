//! V60 SVC-1: the service's side of the socket (see [`crate::svc`] for the protocol).
//!
//! [`SvcServer::bind`] refuses to start beside a live service, removes a stale socket, mints the
//! bearer and writes it `0600` before it listens, and makes the socket `0600` inside the `0700`
//! service directory. [`SvcServer::serve`] answers one request per connection on its own thread
//! and dispatches it to an [`SvcHost`]: the real one is [`crate::svc_run::NodeHost`] (the node and
//! kubo managers); the tests use a recording host to check the dispatch and the bearer gate.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use zeroize::Zeroizing;

use crate::svc::{
    ct_eq, mint_bearer, SvcDrain, SvcHello, SvcOp, SvcPaths, SvcRequest, SvcResponse, SvcStatus,
    MAX_LINE, PROTOCOL_VERSION,
};

/// What the service does for each operation.
pub trait SvcHost: Send + Sync {
    fn hello(&self) -> SvcHello;
    fn status(&self) -> SvcStatus;
    fn node_start(&self) -> Result<(), String>;
    fn node_stop(&self);
    fn ipfs_start(&self) -> Result<(), String>;
    fn ipfs_stop(&self);
    fn set_coinbase(&self, coinbase: String) -> Result<(), String>;
    /// Run the mining gate now; whether it armed.
    fn arm_mining(&self) -> bool;
    fn drain(&self) -> SvcDrain;
    fn resume(&self);
}

/// How the request handlers ask the main loop to exit.
#[derive(Default)]
pub struct Control {
    exit: Mutex<Option<i32>>,
    cv: Condvar,
}

impl Control {
    pub fn new() -> Arc<Self> {
        Arc::new(Control::default())
    }

    /// Ask the service to exit with `code` (the first request wins).
    pub fn request_exit(&self, code: i32) {
        let mut g = self.exit.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(code);
        }
        self.cv.notify_all();
    }

    /// The exit code once requested.
    #[cfg(test)]
    pub fn exit_requested(&self) -> Option<i32> {
        *self.exit.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait up to `timeout` for an exit request.
    pub fn wait(&self, timeout: Duration) -> Option<i32> {
        let g = self.exit.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self
            .cv
            .wait_timeout_while(g, timeout, |code| code.is_none())
            .unwrap_or_else(|e| e.into_inner());
        *g
    }
}

/// The most connections served at once; more wait in the listen backlog.
pub const MAX_CONNECTIONS: usize = 16;
/// How long a connection may take to send its request line.
pub const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// A bound socket and the bearer it accepts.
pub struct SvcServer {
    paths: SvcPaths,
    bearer: Zeroizing<String>,
    listener: UnixListener,
}

impl std::fmt::Debug for SvcServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvcServer")
            .field("socket", &self.paths.socket)
            .finish()
    }
}

/// Why the service could not bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindError {
    /// Another service answers on the socket.
    AlreadyRunning,
    Io(String),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BindError::AlreadyRunning => {
                write!(
                    f,
                    "another background service is already running for this user"
                )
            }
            BindError::Io(e) => write!(f, "cannot open the service socket: {e}"),
        }
    }
}

fn io(what: &str, e: std::io::Error) -> BindError {
    BindError::Io(format!("{what}: {}", e.kind()))
}

impl SvcServer {
    /// Bind the socket. Order: private dir, live-service check, stale socket removal, bearer
    /// written `0600`, bind, socket `0600`. A bind that fails removes the bearer it wrote.
    pub fn bind(paths: &SvcPaths) -> Result<SvcServer, BindError> {
        use std::os::unix::fs::PermissionsExt;
        citrate_core_kit::fsutil::ensure_private_dir(&paths.dir)
            .map_err(|e| io("service directory", e))?;
        if std::fs::symlink_metadata(&paths.socket).is_ok() {
            if UnixStream::connect(&paths.socket).is_ok() {
                return Err(BindError::AlreadyRunning);
            }
            // Nobody answers: a socket left by a service that did not exit cleanly.
            std::fs::remove_file(&paths.socket).map_err(|e| io("stale socket", e))?;
        }
        let bearer = mint_bearer();
        // Remove whatever is at the bearer path first, so the new bearer is always a new
        // regular file and never written through a link.
        match std::fs::remove_file(&paths.bearer) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io("old bearer", e)),
        }
        citrate_core_kit::fsutil::write_secret_file(&paths.bearer, bearer.as_bytes())
            .map_err(|e| io("bearer", e))?;
        let listener = match UnixListener::bind(&paths.socket) {
            Ok(l) => l,
            Err(e) => {
                let _ = std::fs::remove_file(&paths.bearer);
                return Err(io("bind", e));
            }
        };
        if let Err(e) =
            std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o600))
        {
            let _ = std::fs::remove_file(&paths.socket);
            let _ = std::fs::remove_file(&paths.bearer);
            return Err(io("socket mode", e));
        }
        Ok(SvcServer {
            paths: paths.clone(),
            bearer,
            listener,
        })
    }

    /// Serve until `stop` is set: one thread per connection, at most [`MAX_CONNECTIONS`] at once.
    /// On return the socket and bearer files are removed.
    pub fn serve(self, host: Arc<dyn SvcHost>, control: Arc<Control>, stop: Arc<AtomicBool>) {
        let busy = Arc::new(AtomicUsize::new(0));
        let bearer = Arc::new(self.bearer);
        if let Err(e) = self.listener.set_nonblocking(true) {
            eprintln!("[svc] cannot poll the socket: {}", e.kind());
            return;
        }
        while !stop.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if busy.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                        let _ = write_response(
                            &stream,
                            &SvcResponse::err("the service is busy; try again"),
                        );
                        continue;
                    }
                    busy.fetch_add(1, Ordering::SeqCst);
                    let (host, control, bearer, busy2) =
                        (host.clone(), control.clone(), bearer.clone(), busy.clone());
                    let spawned =
                        std::thread::Builder::new()
                            .name("svc-conn".into())
                            .spawn(move || {
                                handle(stream, host.as_ref(), &control, &bearer);
                                busy2.fetch_sub(1, Ordering::SeqCst);
                            });
                    if spawned.is_err() {
                        busy.fetch_sub(1, Ordering::SeqCst);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("[svc] accept: {}", e.kind());
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
        let _ = std::fs::remove_file(&self.paths.socket);
        let _ = std::fs::remove_file(&self.paths.bearer);
    }
}

fn write_response(mut stream: &UnixStream, resp: &SvcResponse) -> std::io::Result<()> {
    let mut line = serde_json::to_string(resp).unwrap_or_else(|_| {
        "{\"ok\":false,\"error\":\"the service could not encode its answer\"}".to_string()
    });
    line.push('\n');
    stream.write_all(line.as_bytes())
}

/// Read one request line, check it, dispatch it, answer it.
fn handle(stream: UnixStream, host: &dyn SvcHost, control: &Control, bearer: &str) {
    // The stream is accepted from a non-blocking listener; serve it blocking with deadlines.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let _ = stream.set_write_timeout(Some(READ_TIMEOUT));
    let mut line = Zeroizing::new(String::new());
    let read = BufReader::new(&stream)
        .take(MAX_LINE as u64)
        .read_line(&mut line);
    let resp = match read {
        Ok(0) => return,
        Ok(_) => answer(&line, host, control, bearer),
        Err(e) => SvcResponse::err(format!("read: {}", e.kind())),
    };
    let _ = write_response(&stream, &resp);
}

/// The answer to one request line. A bad bearer is refused before anything is dispatched.
pub(crate) fn answer(
    line: &str,
    host: &dyn SvcHost,
    control: &Control,
    bearer: &str,
) -> SvcResponse {
    let req: SvcRequest = match serde_json::from_str(line.trim_end()) {
        Ok(r) => r,
        Err(_) => return SvcResponse::err("malformed request"),
    };
    if !ct_eq(req.bearer.expose().as_bytes(), bearer.as_bytes()) {
        return SvcResponse::err("unauthorized");
    }
    if req.v != PROTOCOL_VERSION {
        return SvcResponse::err(format!(
            "protocol {} is not supported (this service: {PROTOCOL_VERSION})",
            req.v
        ));
    }
    match req.op {
        SvcOp::Hello => SvcResponse::ok(host.hello()),
        SvcOp::Status => SvcResponse::ok(host.status()),
        SvcOp::NodeStart => unit(host.node_start()),
        SvcOp::NodeStop => {
            host.node_stop();
            SvcResponse::ok(())
        }
        SvcOp::IpfsStart => unit(host.ipfs_start()),
        SvcOp::IpfsStop => {
            host.ipfs_stop();
            SvcResponse::ok(())
        }
        SvcOp::SetCoinbase { coinbase } => unit(host.set_coinbase(coinbase)),
        SvcOp::ArmMining => SvcResponse::ok(host.arm_mining()),
        SvcOp::Drain => SvcResponse::ok(host.drain()),
        SvcOp::Resume => {
            host.resume();
            SvcResponse::ok(())
        }
        SvcOp::Restart => {
            control.request_exit(crate::svc::EXIT_RESTART);
            SvcResponse::ok(())
        }
        SvcOp::Shutdown => {
            let report = host.drain();
            control.request_exit(0);
            SvcResponse::ok(report)
        }
    }
}

fn unit(r: Result<(), String>) -> SvcResponse {
    match r {
        Ok(()) => SvcResponse::ok(()),
        Err(e) => SvcResponse::err(e),
    }
}
