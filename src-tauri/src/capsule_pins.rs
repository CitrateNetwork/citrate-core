//! Bundled capsule pins (HUP-S2.5, US-2.5 AC2).
//!
//! Citrate Core ships the starter capsules (`src-tauri/capsules/<name>/`) as
//! bundle resources and seeds them into the Hermes capsule dir on start.
//! Two checks guard every shipped capsule before it can load:
//!
//! 1. **Here, before seeding:** the SHA-256 of each bundled `<name>.cps`
//!    must equal the digest pinned below. The pins are compiled into the
//!    signed app binary, so a resource directory edited after install
//!    cannot swap a capsule in. A capsule that does not match is not
//!    seeded, and a seeded copy that no longer matches (tampered, or left
//!    from an older release) is replaced by the verified bundled copy.
//! 2. **In the agent runtime, at load:** the sidecar opens each `.cps`
//!    through the verified path (content hash over manifest and body, the
//!    publisher's ed25519 signature, and the fleet allowlist) and refuses
//!    any capsule that fails, with the reason.
//!
//! Re-packing a capsule changes its digest: update the pin in the same
//! change. `pins_match_the_bundled_files` fails until they agree.

use sha2::{Digest, Sha256};
use std::path::Path;

/// `(capsule name, lowercase hex SHA-256 of capsules/<name>/<name>.cps)`.
pub const BUNDLED_CAPSULES: &[(&str, &str)] = &[
    (
        "echo-chain",
        "4e20dcf03ab00f06053502c172734afc937260b9cd61cf1b809bc18948a5caf2",
    ),
    (
        "hello",
        "ec776b378c980b76c68357c9ddfc5871b0d056a7064f1fe6e39770969d375403",
    ),
];

/// What one seeding pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SeedReport {
    /// Copied fresh into the capsule dir.
    pub seeded: Vec<String>,
    /// Already present and matching the pin; left alone.
    pub kept: Vec<String>,
    /// Present but not matching the pin; replaced by the verified copy.
    pub repaired: Vec<String>,
    /// Not seeded, with the reason.
    pub refused: Vec<(String, String)>,
    /// Capsule dirs in the destination afterwards (user capsules included).
    pub present: usize,
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Check `<dir>/<name>.cps` against `pin`.
fn verify_cps(dir: &Path, name: &str, pin: &str) -> Result<(), String> {
    let path = dir.join(format!("{name}.cps"));
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let got = sha256_hex(&bytes);
    if got == pin {
        Ok(())
    } else {
        Err(format!(
            "{} does not match the digest pinned in this build (got {got})",
            path.display()
        ))
    }
}

/// Copy the regular files of `src` into a fresh `dst`.
fn copy_capsule_dir(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("cannot create {}: {e}", dst.display()))?;
    let files =
        std::fs::read_dir(src).map_err(|e| format!("cannot list {}: {e}", src.display()))?;
    for f in files.flatten() {
        let from = f.path();
        let is_file = f.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file {
            std::fs::copy(&from, dst.join(f.file_name()))
                .map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// Seed `dest` from `bundled` using the compiled-in [`BUNDLED_CAPSULES`].
pub fn seed_verified(bundled: &Path, dest: &Path) -> SeedReport {
    seed_verified_with(bundled, dest, BUNDLED_CAPSULES)
}

/// Seed `dest` from `bundled`, admitting only capsules whose `.cps` matches
/// `pins`. Capsule dirs in `dest` that are not bundled names (the member's
/// own) are never touched. Best effort: every failure is reported, none is
/// fatal.
pub fn seed_verified_with(bundled: &Path, dest: &Path, pins: &[(&str, &str)]) -> SeedReport {
    let mut report = SeedReport::default();
    let _ = std::fs::create_dir_all(dest);
    if let Ok(entries) = std::fs::read_dir(bundled) {
        let mut dirs: Vec<_> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect();
        dirs.sort_by_key(|e| e.file_name());
        for entry in dirs {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(pin) = pins.iter().find(|(n, _)| *n == name).map(|(_, d)| *d) else {
                report.refused.push((
                    name,
                    "not a capsule pinned in this build, so it is not installed".to_string(),
                ));
                continue;
            };
            if let Err(why) = verify_cps(&entry.path(), &name, pin) {
                report
                    .refused
                    .push((name, format!("bundled copy refused: {why}")));
                continue;
            }
            let target = dest.join(&name);
            if target.exists() {
                if verify_cps(&target, &name, pin).is_ok() {
                    report.kept.push(name);
                    continue;
                }
                // A copy that no longer matches the pin cannot load in the
                // runtime anyway; replace it with the verified bundled one.
                if let Err(e) = std::fs::remove_dir_all(&target) {
                    report.refused.push((
                        name,
                        format!(
                            "cannot replace the unverified copy at {}: {e}",
                            target.display()
                        ),
                    ));
                    continue;
                }
                match copy_capsule_dir(&entry.path(), &target) {
                    Ok(()) => report.repaired.push(name),
                    Err(why) => report.refused.push((name, why)),
                }
                continue;
            }
            match copy_capsule_dir(&entry.path(), &target) {
                Ok(()) => report.seeded.push(name),
                Err(why) => report.refused.push((name, why)),
            }
        }
    }
    report.present = std::fs::read_dir(dest)
        .map(|e| e.flatten().filter(|x| x.path().is_dir()).count())
        .unwrap_or(0);
    report
}

#[cfg(test)]
mod tests {
    include!("capsule_pins_tests.rs");
}
