//! `citrate-components`: release-side checks for the component updater.
//!
//! ```text
//! citrate-components check-bundle [--repo <dir>]
//! citrate-components manifest-from-bundle --sequence <n> --issued-at <unix> --expires-at <unix> --sigs <dir> [--repo <dir>]
//! citrate-components verify-manifest --manifest <file> --sig <file> --pubkey <key line> [--now <unix>]
//! citrate-components unpack --format <raw|tar.gz|tar.xz> --archive <file> --dest <new dir> [--name <raw file name>]
//! ```
//!
//! `--sigs` holds one `<sha256>.minisig` per measured artifact, made at the release ceremony
//! with `minisign -S -H -t "citrate-components-artifact <name> <version>"`. Nothing here signs.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use citrate_components::bundle::{
    check_bundle, manifest_from_bundle, Bundle, DepsLock, DEPS_LOCK_PATH,
};
use citrate_components::extract::extract;
use citrate_components::key::TrustRoot;
use citrate_components::manifest::{verify_manifest, ArchiveFormat};

const BUNDLE_PATH: &str = "components/toolchain-bundle.json";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("citrate-components: {e}");
            ExitCode::FAILURE
        }
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name).ok_or_else(|| format!("missing {name}"))
}

fn num(args: &[String], name: &str) -> Result<u64, String> {
    need(args, name)?
        .parse()
        .map_err(|_| format!("{name} must be a whole number"))
}

fn read(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn repo(args: &[String]) -> PathBuf {
    PathBuf::from(flag(args, "--repo").unwrap_or("."))
}

fn load(args: &[String]) -> Result<(Bundle, DepsLock, PathBuf), String> {
    let root = repo(args);
    let b = Bundle::parse(&read(&root.join(BUNDLE_PATH))?).map_err(|e| e.to_string())?;
    let d = DepsLock::parse(&read(&root.join(DEPS_LOCK_PATH))?).map_err(|e| e.to_string())?;
    Ok((b, d, root))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("check-bundle") => {
            let (b, d, root) = load(args)?;
            let problems = check_bundle(&b, &d, &root);
            if problems.is_empty() {
                Ok(format!("bundle ok: {} tools, {} libraries", b.tools.len(), b.libraries.archives.len()))
            } else {
                Err(problems.join("\n"))
            }
        }
        Some("manifest-from-bundle") => {
            let (b, d, root) = load(args)?;
            let problems = check_bundle(&b, &d, &root);
            if !problems.is_empty() {
                return Err(problems.join("\n"));
            }
            let dir = PathBuf::from(need(args, "--sigs")?);
            let mut sigs = BTreeMap::new();
            for e in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
                let p = e.map_err(|e| e.to_string())?.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if let Some(h) = name.strip_suffix(".minisig") {
                    sigs.insert(h.to_string(), read(&p)?);
                }
            }
            let m = manifest_from_bundle(&b, &d, &sigs, num(args, "--sequence")?, num(args, "--issued-at")?, num(args, "--expires-at")?)
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&m).map_err(|e| e.to_string())
        }
        Some("verify-manifest") => {
            let bytes = std::fs::read(need(args, "--manifest")?).map_err(|e| e.to_string())?;
            let sig = read(Path::new(need(args, "--sig")?))?;
            let root = TrustRoot::from_base64(need(args, "--pubkey")?).map_err(|e| e.to_string())?;
            let at = match flag(args, "--now") {
                Some(_) => num(args, "--now")?,
                None => now(),
            };
            let vm = verify_manifest(&bytes, &sig, &root, None, at).map_err(|e| e.to_string())?;
            let names: Vec<String> = vm
                .manifest()
                .components
                .iter()
                .map(|c| format!("{} {}", c.name, c.version))
                .collect();
            Ok(format!(
                "manifest ok: sequence {}, key {}, sha256 {}\n{}",
                vm.manifest().sequence,
                root.fingerprint(),
                vm.digest_hex(),
                names.join("\n")
            ))
        }
        Some("unpack") => {
            let format: ArchiveFormat = serde_json::from_value(serde_json::Value::String(need(args, "--format")?.to_string()))
                .map_err(|_| "unknown --format".to_string())?;
            let rep = extract(
                format,
                Path::new(need(args, "--archive")?),
                Path::new(need(args, "--dest")?),
                flag(args, "--name").unwrap_or("artifact"),
            )
            .map_err(|e| e.to_string())?;
            Ok(format!(
                "unpacked: {} files, {} dirs, {} symlinks, {} bytes",
                rep.files, rep.dirs, rep.symlinks, rep.bytes
            ))
        }
        _ => Err("usage: citrate-components <check-bundle|manifest-from-bundle|verify-manifest|unpack> ...".into()),
    }
}
