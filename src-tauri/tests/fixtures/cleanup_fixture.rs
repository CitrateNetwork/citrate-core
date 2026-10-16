// Native fixture for the owned-cleanup tests (src-tauri/src/owned_cleanup_tests.rs).
// Compiled by the tests with rustc into OUT_DIR, then copied to the paths each test needs.
//
//   sleep <ms> [ignored...]          sleep, then exit
//   detach <exe> <arg0> [args...]    start <exe> with that argv[0] and args, print its pid, exit
//                                    at once (the started process is left without its parent)
//   hold <exe> [args...]             start <exe> with args, print its pid, wait for it
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("detach") if args.len() >= 4 => {
            let child = Command::new(&args[2])
                .arg0(&args[3])
                .args(&args[4..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match child {
                Ok(c) => {
                    println!("{}", c.id());
                    let _ = std::io::stdout().flush();
                }
                Err(_) => std::process::exit(2),
            }
        }
        Some("hold") if args.len() >= 3 => {
            let child = Command::new(&args[2])
                .args(&args[3..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match child {
                Ok(mut c) => {
                    println!("{}", c.id());
                    let _ = std::io::stdout().flush();
                    let _ = c.wait();
                }
                Err(_) => std::process::exit(2),
            }
        }
        Some("sleep") => {
            let ms: u64 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(1000);
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        _ => std::process::exit(2),
    }
}
