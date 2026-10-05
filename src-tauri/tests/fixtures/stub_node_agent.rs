//! Cross-platform native node-agent fixture for AgentManager unit tests.
//!
//! It mirrors the process and HTTP contract exercised by the supervision test:
//! read the parent's bearer file, bind the requested loopback address, enforce
//! bearer authentication, and remain alive until the supervisor terminates it.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ADDR_ENV: &str = "CITRATE_NODE_AGENT_ADDR";
const TOKEN_FILE_ENV: &str = "CITRATE_NODE_AGENT_TOKEN_FILE";

fn main() {
    if let Err(message) = run() {
        eprintln!("stub_node_agent: {message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let address = std::env::var(ADDR_ENV).unwrap_or_else(|_| "127.0.0.1:19600".to_string());
    let token_path = std::env::var_os(TOKEN_FILE_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{TOKEN_FILE_ENV} required"))?;
    let token = read_token(&token_path)?;
    let listener = TcpListener::bind(&address)
        .map_err(|error| format!("bind supervision address {address}: {error}"))?;

    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                if let Err(error) = serve_one(&mut stream, &token) {
                    eprintln!("stub_node_agent: request failed: {error}");
                }
            }
            Err(error) => return Err(format!("accept supervision request: {error}")),
        }
    }
    Ok(())
}

fn read_token(path: &Path) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match std::fs::read_to_string(path) {
            Ok(value) if !value.trim().is_empty() => return Ok(value.trim().to_string()),
            Ok(_) | Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(_) => return Err(format!("token file is empty: {}", path.display())),
            Err(error) => {
                return Err(format!("read token file {}: {error}", path.display()));
            }
        }
    }
}

fn serve_one(stream: &mut TcpStream, expected_token: &str) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| format!("set read timeout: {error}"))?;
    let request = read_headers(stream)?;
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let presented = request.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then(|| value.trim())
    });
    let authed = presented
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value.trim() == expected_token);

    let (status, body) = if method == "GET" && path == "/health" {
        ("200 OK", r#"{"state":"idle","active_jobs":0}"#)
    } else if !authed {
        ("401 Unauthorized", r#""missing or invalid supervision token""#)
    } else if method == "GET" && path == "/status" {
        ("200 OK", r#""idle""#)
    } else if method == "GET" && path == "/signature-requests" {
        ("200 OK", "[]")
    } else if method == "POST" {
        ("200 OK", r#""ok""#)
    } else {
        ("404 Not Found", r#""not found""#)
    };
    write_response(stream, status, body)
}

fn read_headers(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    while bytes.len() < 16 * 1024 {
        let read = stream
            .read(&mut chunk)
            .map_err(|error| format!("read request: {error}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).map_err(|error| format!("request is not UTF-8: {error}"))
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .and_then(|_| stream.flush())
        .map_err(|error| format!("write response: {error}"))
}
