//! HUP-S1.6 — hardware tier recommendation at onboarding (planset 2026-09-30-hermes-upskill,
//! D-5, 02_ARCHITECTURE §3, US-1.6).
//!
//! A small, honest, LOCAL hardware read plus a pure tier decision:
//!
//! | Tier | Usable memory                         | Local chat model (hint)          | ctx  |
//! |------|---------------------------------------|----------------------------------|------|
//! | T0   | < 12 GB                               | Gemma 4 E4B Q4                   | 16k  |
//! | T1   | 12–24 GB                              | Qwen 3.6 27B Q4 / Gemma 4 26B    | 32k  |
//! | T2   | ≥ 24 GB, or ≥ 16 GB dedicated VRAM    | Qwen 3.6 35B-A3B                 | 64k  |
//!
//! T0 is the explicit guided / escalate tier (red-team correction #11): `guided = true`, and the
//! UI recommends planner escalation. The concrete model picks are placeholders until the S1.7
//! eval gate finalizes them; the thresholds are the planset's.
//!
//! **This is a subset of a citrate-sizeup receipt, not sizeup itself.** Sizeup
//! (`citrate-sizeup/crates/sizeup-core`) reads far more (per-DIMM SMBIOS, GGUF-header fit,
//! measured bandwidth, the macOS `iogpu.wired_limit_mb` GPU budget, posture). Taking it as a
//! Cargo dependency needs a Rule-12 `[[drift]]` entry in `citrate-federation/manifest.toml`
//! first, so full sizeup integration is a FOLLOW-UP. What we read here, per OS:
//!
//! - **macOS:** `sysctl hw.memsize hw.optional.arm64` (total RAM; Apple Silicon ⇒ the GPU shares
//!   that unified memory). Dedicated VRAM is not read (Intel-Mac discrete GPUs stay unknown).
//! - **Linux:** `/proc/meminfo` `MemTotal`; `nvidia-smi --query-gpu=memory.total` if present
//!   (other vendors' VRAM stays unknown).
//! - **Windows:** PowerShell `Win32_ComputerSystem.TotalPhysicalMemory`; `nvidia-smi` if present.
//! - **All unix:** `statvfs` free space in the app data dir. (Windows: unknown.)
//!
//! Anything that cannot be read is `None` and is shown as unknown — never guessed (Rule 1).
//!
//! **Usable memory** = total RAM minus [`NODE_RESERVE_BYTES`], the RAM of sizeup's pinned Citrate
//! daemon set (`budget::DAEMON_SET`: citrate 4 GiB + node-agent 1 + mem-mcp 1 + ipfs 1). This is
//! sizeup's "daemons first" budget idea: a model is only offered against what is left after the
//! node stack it runs beside.
//!
//! The user's override is persisted in the existing app config store (`config.json`, key
//! [`OVERRIDE_KEY`]) as the tier id ONLY — no hardware details are stored. No network.

use serde::Serialize;

/// One GiB. Tier thresholds are in GiB (the unit `sysctl`/`/proc/meminfo` report).
pub const GIB: u64 = 1024 * 1024 * 1024;

/// RAM set aside for the node + its daemons before a model is sized — the RAM column of
/// citrate-sizeup `budget::DAEMON_SET` (4 + 1 + 1 + 1 GiB). Re-pin when that set changes.
pub const NODE_RESERVE_BYTES: u64 = 7 * GIB;

/// Usable memory at or above which a host is T1.
const T1_USABLE: u64 = 12 * GIB;
/// Usable memory at or above which a host is T2.
const T2_USABLE: u64 = 24 * GIB;
/// Dedicated GPU memory at or above which a host is T2 regardless of system RAM.
const T2_VRAM: u64 = 16 * GIB;

/// The config-store file (shared with `config::AppConfig`) and the key the override lives under.
const STORE_FILE: &str = "config.json";
pub const OVERRIDE_KEY: &str = "tierOverride";

/// A hardware tier (02_ARCHITECTURE §3). Serialized as its id: `"T0" | "T1" | "T2"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tier {
    T0,
    T1,
    T2,
}

impl Tier {
    pub fn id(self) -> &'static str {
        match self {
            Tier::T0 => "T0",
            Tier::T1 => "T1",
            Tier::T2 => "T2",
        }
    }

    /// Strict parse of a tier id. Anything but exactly `T0`/`T1`/`T2` is rejected.
    pub fn parse(s: &str) -> Option<Tier> {
        match s {
            "T0" => Some(Tier::T0),
            "T1" => Some(Tier::T1),
            "T2" => Some(Tier::T2),
            _ => None,
        }
    }
}

/// What this machine reported. `None` = could not be read (shown as unknown, never guessed).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareFacts {
    /// `std::env::consts::OS` (`macos` | `linux` | `windows` | …).
    pub os: String,
    /// The HOST CPU arch (a Rosetta-translated build on Apple Silicon still reports `aarch64`).
    pub arch: String,
    pub total_ram_bytes: Option<u64>,
    /// `Some(true)` on Apple Silicon (the GPU shares system RAM). `None` = not known either way.
    pub unified_memory: Option<bool>,
    /// Largest dedicated GPU memory found (NVIDIA via `nvidia-smi` only). `None` = unknown/none.
    pub gpu_vram_bytes: Option<u64>,
    /// Free space in the app data dir.
    pub disk_free_bytes: Option<u64>,
}

/// The fixed model hint + ctx for a tier.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierProfile {
    pub tier: Tier,
    /// Human-readable model family (display only; S1.7 finalizes the picks).
    pub model_hint: String,
    /// Normalized filename fragments (`normalize_model_key`) that identify a matching model,
    /// so the Models surface can label an already-present file "recommended".
    pub model_match: Vec<String>,
    pub ctx_tokens: u32,
}

/// The recommendation for this machine.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierRecommendation {
    pub tier: Tier,
    pub model_hint: String,
    pub model_match: Vec<String>,
    pub ctx_tokens: u32,
    /// Plain-language reasons, one per line, in the order they were applied.
    pub rationale: Vec<String>,
    /// T0: the guided / escalate tier — escalating the planner is recommended.
    pub guided: bool,
    /// Total RAM minus the node reserve (saturating). `None` when RAM is unknown.
    pub usable_bytes: Option<u64>,
}

/// What `tier_recommend` returns: the facts, the recommendation, the stored override, the tier
/// in effect, and every profile (so the UI can show the override's model + ctx).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TierReport {
    pub facts: HardwareFacts,
    pub recommendation: TierRecommendation,
    pub override_tier: Option<Tier>,
    pub effective: Tier,
    pub profiles: Vec<TierProfile>,
}

/// The planset tier table (02_ARCHITECTURE §3).
pub fn profile(tier: Tier) -> TierProfile {
    let (hint, keys, ctx): (&str, &[&str], u32) = match tier {
        Tier::T0 => ("Gemma 4 E4B (Q4)", &["gemma4e4b"], 16_384),
        Tier::T1 => (
            "Qwen 3.6 27B (Q4) or Gemma 4 26B",
            &["qwen3627b", "gemma426b"],
            32_768,
        ),
        Tier::T2 => ("Qwen 3.6 35B-A3B", &["qwen3635ba3b"], 65_536),
    };
    TierProfile {
        tier,
        model_hint: hint.to_string(),
        model_match: keys.iter().map(|k| k.to_string()).collect(),
        ctx_tokens: ctx,
    }
}

/// Lowercase ASCII alphanumerics only — `"Qwen3.6-27B-Q4_K_M.gguf"` → `"qwen3627bq4kmgguf"`. The
/// TS side (`slices/tier.ts::normalizeModelKey`) applies the same normalization to a model
/// filename before matching `model_match`; this Rust twin pins the match keys in tests.
#[cfg(test)]
pub(crate) fn normalize_model_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// A byte count as GiB for display: whole numbers bare, otherwise one decimal (truncated, so
/// 11.99 GiB never displays as "12").
pub(crate) fn fmt_gb(bytes: u64) -> String {
    let tenths = bytes.saturating_mul(10) / GIB;
    if tenths.is_multiple_of(10) {
        format!("{}", tenths / 10)
    } else {
        format!("{}.{}", tenths / 10, tenths % 10)
    }
}

/// **The pure decision.** Deterministic over `facts`.
pub fn recommend_tier(facts: &HardwareFacts) -> TierRecommendation {
    let mut why = Vec::new();

    let usable = facts
        .total_ram_bytes
        .map(|t| t.saturating_sub(NODE_RESERVE_BYTES));
    match facts.total_ram_bytes {
        Some(total) => {
            let unified = if facts.unified_memory == Some(true) {
                " unified memory (shared with the GPU)"
            } else {
                " memory"
            };
            why.push(format!("{} GB{unified}", fmt_gb(total)));
            why.push(format!(
                "{} GB kept for the node and its helpers, leaving {} GB for a model",
                fmt_gb(NODE_RESERVE_BYTES),
                fmt_gb(usable.unwrap_or(0))
            ));
        }
        None => why.push(
            "Memory size could not be read on this machine, so the smallest tier is used to stay safe"
                .to_string(),
        ),
    }

    match (facts.gpu_vram_bytes, facts.unified_memory) {
        (Some(v), _) => why.push(format!("Dedicated GPU memory: {} GB", fmt_gb(v))),
        (None, Some(true)) => {}
        (None, _) => why.push("Dedicated GPU memory: unknown".to_string()),
    }

    let vram_t2 = facts.gpu_vram_bytes.is_some_and(|v| v >= T2_VRAM);
    let tier = if vram_t2 {
        why.push("16 GB or more of dedicated GPU memory → T2".to_string());
        Tier::T2
    } else {
        match usable {
            Some(u) if u >= T2_USABLE => {
                why.push("24 GB or more usable → T2".to_string());
                Tier::T2
            }
            Some(u) if u >= T1_USABLE => {
                why.push("12–24 GB usable → T1".to_string());
                Tier::T1
            }
            Some(_) => {
                why.push("Under 12 GB usable → T0".to_string());
                Tier::T0
            }
            None => Tier::T0,
        }
    };
    let guided = tier == Tier::T0;
    if guided {
        why.push(
            "T0 is the guided tier: a small local model, with bigger planning jobs escalated to a larger model you choose"
                .to_string(),
        );
    }

    match facts.disk_free_bytes {
        Some(d) => why.push(format!("{} GB free in the app data folder", fmt_gb(d))),
        None => why.push("Free disk: unknown".to_string()),
    }

    let p = profile(tier);
    TierRecommendation {
        tier,
        model_hint: p.model_hint,
        model_match: p.model_match,
        ctx_tokens: p.ctx_tokens,
        rationale: why,
        guided,
        usable_bytes: usable,
    }
}

/// Assemble the command result from probed facts + the stored override.
pub fn build_report(facts: HardwareFacts, override_tier: Option<Tier>) -> TierReport {
    let recommendation = recommend_tier(&facts);
    let effective = override_tier.unwrap_or(recommendation.tier);
    TierReport {
        facts,
        recommendation,
        override_tier,
        effective,
        profiles: [Tier::T0, Tier::T1, Tier::T2].map(profile).to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Persisted override — only the tier id is stored.
// ---------------------------------------------------------------------------

/// Decode the stored override. Anything but a bare tier-id string reads as "no override".
pub fn decode_override(v: Option<serde_json::Value>) -> Option<Tier> {
    v.as_ref().and_then(|v| v.as_str()).and_then(Tier::parse)
}

/// Encode the override for the store (`null` clears it).
pub fn encode_override(t: Option<Tier>) -> serde_json::Value {
    t.map_or(serde_json::Value::Null, |t| {
        serde_json::Value::String(t.id().into())
    })
}

/// Validate the command argument BEFORE anything touches the store.
pub fn parse_override_arg(arg: Option<String>) -> Result<Option<Tier>, String> {
    match arg {
        None => Ok(None),
        Some(s) => Tier::parse(&s)
            .map(Some)
            .ok_or_else(|| format!("unknown tier '{s}' (expected T0, T1 or T2)")),
    }
}

// ---------------------------------------------------------------------------
// OS readers — pure parsers (fixture-tested) + thin I/O wrappers.
// ---------------------------------------------------------------------------

/// `sysctl hw.memsize hw.optional.arm64` stdout → (total RAM bytes, Apple Silicon?).
pub fn parse_macos_sysctl(out: &str) -> (Option<u64>, Option<bool>) {
    let mut ram = None;
    let mut arm = None;
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim() {
            "hw.memsize" => ram = v.trim().parse::<u64>().ok(),
            "hw.optional.arm64" => {
                arm = match v.trim() {
                    "1" => Some(true),
                    "0" => Some(false),
                    _ => None,
                }
            }
            _ => {}
        }
    }
    (ram, arm)
}

/// `/proc/meminfo` → `MemTotal` in bytes.
pub fn parse_meminfo_total(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kib: u64 = line
        .trim_start_matches("MemTotal:")
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    kib.checked_mul(1024)
}

/// `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits` (MiB per GPU) → the
/// largest GPU's memory in bytes.
pub fn parse_nvidia_smi_mib(out: &str) -> Option<u64> {
    out.lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .max()
        .and_then(|mib| mib.checked_mul(1024 * 1024))
}

/// PowerShell `(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory` → bytes.
pub fn parse_windows_total_memory(out: &str) -> Option<u64> {
    out.trim().parse().ok()
}

/// Combine raw readings into facts. `apple_silicon` comes from `hw.optional.arm64`, so a
/// Rosetta-translated build still reports the host as `aarch64` with unified memory.
pub fn facts_from_parts(
    os: &str,
    arch: &str,
    total_ram_bytes: Option<u64>,
    apple_silicon: Option<bool>,
    gpu_vram_bytes: Option<u64>,
    disk_free_bytes: Option<u64>,
) -> HardwareFacts {
    let is_apple_silicon = os == "macos" && apple_silicon == Some(true);
    HardwareFacts {
        os: os.to_string(),
        arch: if is_apple_silicon {
            "aarch64".to_string()
        } else {
            arch.to_string()
        },
        total_ram_bytes,
        unified_memory: is_apple_silicon.then_some(true),
        gpu_vram_bytes,
        disk_free_bytes,
    }
}

/// Run a short read-only command and return its stdout (whatever the exit status — `sysctl`
/// exits non-zero when one of several keys is absent but still prints the others). `None` when
/// the program is missing.
fn run_stdout(program: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn nvidia_vram() -> Option<u64> {
    run_stdout(
        "nvidia-smi",
        &["--query-gpu=memory.total", "--format=csv,noheader,nounits"],
    )
    .as_deref()
    .and_then(parse_nvidia_smi_mib)
}

/// Free bytes available to this user on the filesystem holding `dir` (or its nearest existing
/// ancestor — the app data dir may not exist yet on a first run).
#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // statvfs field widths differ between macOS and Linux
fn disk_free(dir: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = dir.ancestors().find(|p| p.exists())?;
    let c = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    let mut st = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `c` is a valid NUL-terminated path and `st` points to writable storage of the
    // right type; statvfs only writes into it and we read it only after a 0 return.
    let rc = unsafe { libc::statvfs(c.as_ptr(), st.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so the struct is initialized.
    let st = unsafe { st.assume_init() };
    (st.f_bavail as u64).checked_mul(st.f_frsize as u64)
}

#[cfg(not(unix))]
fn disk_free(_dir: &std::path::Path) -> Option<u64> {
    None
}

/// Read this machine. Blocking (spawns `sysctl` / `nvidia-smi` / PowerShell) — call it only via
/// [`crate::blocking::off_main`].
pub fn probe(data_dir: &std::path::Path) -> HardwareFacts {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let disk = disk_free(data_dir);
    match os {
        "macos" => {
            let (ram, arm) = run_stdout("sysctl", &["hw.memsize", "hw.optional.arm64"])
                .as_deref()
                .map(parse_macos_sysctl)
                .unwrap_or((None, None));
            facts_from_parts(os, arch, ram, arm, None, disk)
        }
        "linux" => {
            let ram = std::fs::read_to_string("/proc/meminfo")
                .ok()
                .as_deref()
                .and_then(parse_meminfo_total);
            facts_from_parts(os, arch, ram, None, nvidia_vram(), disk)
        }
        "windows" => {
            let ram = run_stdout(
                "powershell",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
                ],
            )
            .as_deref()
            .and_then(parse_windows_total_memory);
            facts_from_parts(os, arch, ram, None, nvidia_vram(), disk)
        }
        _ => facts_from_parts(os, arch, None, None, None, disk),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands (HUP-S0.1: async + off_main — they spawn processes and touch the store).
// ---------------------------------------------------------------------------

pub(crate) fn load_override<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<Tier>, String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    Ok(decode_override(store.get(OVERRIDE_KEY)))
}

/// **Command — tier_recommend.** Probe this machine locally and recommend a tier (US-1.6 AC1).
/// Returns the facts, the recommendation + rationale, and the stored override. No network.
#[tauri::command]
pub async fn tier_recommend(app_h: tauri::AppHandle) -> Result<TierReport, String> {
    crate::blocking::off_main(move || tier_recommend_sync(app_h.clone())).await
}

/// Blocking body of [`tier_recommend`]; reached only through [`crate::blocking::off_main`].
pub fn tier_recommend_sync(app: tauri::AppHandle) -> Result<TierReport, String> {
    use tauri::Manager;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let facts = probe(&data_dir);
    Ok(build_report(facts, load_override(&app)?))
}

/// **Command — tier_set_override.** Persist the user's tier choice (US-1.6 AC2); `null` clears it
/// and the recommendation applies again. Stores ONLY the tier id. Returns the stored value.
#[tauri::command]
pub async fn tier_set_override(
    app_h: tauri::AppHandle,
    tier: Option<String>,
) -> Result<Option<Tier>, String> {
    crate::blocking::off_main(move || tier_set_override_sync(app_h.clone(), tier)).await
}

/// Blocking body of [`tier_set_override`]; reached only through [`crate::blocking::off_main`].
pub fn tier_set_override_sync(
    app: tauri::AppHandle,
    tier: Option<String>,
) -> Result<Option<Tier>, String> {
    use tauri_plugin_store::StoreExt;
    let t = parse_override_arg(tier)?;
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(OVERRIDE_KEY, encode_override(t));
    store.save().map_err(|e| e.to_string())?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    include!("tier_tests.rs");
}
