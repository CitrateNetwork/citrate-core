// HUP-S1.6 — tier recommendation tests. Included into `tier.rs` as `mod tests` (the
// model_catalog pattern), so private helpers are reachable via `super::*`.
use super::*;

const G: u64 = GIB;

/// Facts for a machine whose USABLE memory (total minus the daemon reserve) is `usable_tenths`
/// tenths of a GiB, with no known dedicated GPU memory.
fn facts_usable_tenths(usable_tenths: u64) -> HardwareFacts {
    HardwareFacts {
        os: "linux".into(),
        arch: "x86_64".into(),
        total_ram_bytes: Some(NODE_RESERVE_BYTES + usable_tenths * G / 10),
        unified_memory: None,
        gpu_vram_bytes: None,
        disk_free_bytes: None,
    }
}

// ---------------------------------------------------------------------------
// recommend_tier — the boundaries of the 02_ARCHITECTURE §3 table.
// ---------------------------------------------------------------------------

#[test]
fn usable_11_9_gib_is_t0_guided() {
    let r = recommend_tier(&facts_usable_tenths(119));
    assert_eq!(r.tier, Tier::T0);
    assert!(r.guided, "T0 is the guided / escalate tier (red-team correction #11)");
    assert_eq!(r.ctx_tokens, 16_384);
}

#[test]
fn usable_12_gib_is_t1() {
    let r = recommend_tier(&facts_usable_tenths(120));
    assert_eq!(r.tier, Tier::T1);
    assert!(!r.guided);
    assert_eq!(r.ctx_tokens, 32_768);
}

#[test]
fn usable_23_9_gib_is_t1() {
    let r = recommend_tier(&facts_usable_tenths(239));
    assert_eq!(r.tier, Tier::T1);
}

#[test]
fn usable_24_gib_is_t2() {
    let r = recommend_tier(&facts_usable_tenths(240));
    assert_eq!(r.tier, Tier::T2);
    assert!(!r.guided);
    assert_eq!(r.ctx_tokens, 65_536);
}

#[test]
fn sixteen_gib_of_dedicated_vram_lifts_a_small_host_to_t2() {
    let mut f = facts_usable_tenths(80);
    f.gpu_vram_bytes = Some(16 * G);
    let r = recommend_tier(&f);
    assert_eq!(r.tier, Tier::T2);
    assert!(r.rationale.iter().any(|l| l.contains("GPU")));
}

#[test]
fn vram_just_under_16_gib_does_not_lift_the_tier() {
    let mut f = facts_usable_tenths(80);
    f.gpu_vram_bytes = Some(16 * G - 1);
    assert_eq!(recommend_tier(&f).tier, Tier::T0);
}

#[test]
fn ram_below_the_reserve_saturates_to_t0_not_a_panic() {
    let mut f = facts_usable_tenths(0);
    f.total_ram_bytes = Some(2 * G);
    let r = recommend_tier(&f);
    assert_eq!(r.tier, Tier::T0);
    assert_eq!(r.usable_bytes, Some(0));
}

#[test]
fn unknown_ram_is_t0_guided_and_says_so_never_a_guess() {
    let mut f = facts_usable_tenths(0);
    f.total_ram_bytes = None;
    let r = recommend_tier(&f);
    assert_eq!(r.tier, Tier::T0);
    assert!(r.guided);
    assert_eq!(r.usable_bytes, None);
    assert!(r.rationale.iter().any(|l| l.contains("could not be read")));
}

#[test]
fn the_reserve_is_the_sizeup_daemon_set_ram_sum() {
    // citrate 4 GiB + node-agent 1 + mem-mcp 1 + ipfs 1 (citrate-sizeup budget::DAEMON_SET).
    assert_eq!(NODE_RESERVE_BYTES, 7 * G);
}

#[test]
fn a_16_gib_laptop_is_t0_and_a_32_gib_mac_is_t2() {
    // The two reference machines the tier table has to get right.
    let mut f = facts_usable_tenths(0);
    f.total_ram_bytes = Some(16 * G);
    assert_eq!(recommend_tier(&f).tier, Tier::T0);
    f.total_ram_bytes = Some(32 * G);
    f.unified_memory = Some(true);
    f.os = "macos".into();
    f.arch = "aarch64".into();
    let r = recommend_tier(&f);
    assert_eq!(r.tier, Tier::T2);
    assert!(r.rationale.iter().any(|l| l.contains("32 GB") && l.contains("unified")));
}

#[test]
fn recommendation_is_deterministic() {
    let f = facts_usable_tenths(150);
    assert_eq!(recommend_tier(&f), recommend_tier(&f));
}

#[test]
fn unknown_gpu_and_disk_are_stated_as_unknown() {
    let r = recommend_tier(&facts_usable_tenths(150));
    assert!(r.rationale.iter().any(|l| l.contains("GPU memory: unknown")));
    assert!(r.rationale.iter().any(|l| l.contains("Free disk: unknown")));
}

#[test]
fn known_disk_is_reported_in_the_rationale() {
    let mut f = facts_usable_tenths(150);
    f.disk_free_bytes = Some(200 * G);
    let r = recommend_tier(&f);
    assert!(r.rationale.iter().any(|l| l.contains("200 GB free")));
}

#[test]
fn every_tier_profile_matches_the_planset_table() {
    let t0 = profile(Tier::T0);
    assert!(t0.model_hint.contains("Gemma 4 E4B"));
    assert_eq!(t0.ctx_tokens, 16_384);
    let t1 = profile(Tier::T1);
    assert!(t1.model_hint.contains("Qwen 3.6 27B") && t1.model_hint.contains("Gemma 4 26B"));
    assert_eq!(t1.ctx_tokens, 32_768);
    let t2 = profile(Tier::T2);
    assert!(t2.model_hint.contains("Qwen 3.6 35B-A3B"));
    assert_eq!(t2.ctx_tokens, 65_536);
}

#[test]
fn the_bundled_gemma_file_matches_the_t0_hint() {
    // The app's default model must be recognised as the T0 pick, so the Models surface can label
    // it without a download.
    let key = normalize_model_key(crate::model::MODEL_FILE);
    assert!(profile(Tier::T0).model_match.iter().any(|m| key.contains(m.as_str())));
    assert!(!profile(Tier::T2).model_match.iter().any(|m| key.contains(m.as_str())));
}

#[test]
fn match_keys_distinguish_qwen_27b_from_35b() {
    let q27 = normalize_model_key("Qwen3.6-27B-Q4_K_M.gguf");
    let q35 = normalize_model_key("Qwen3.6-35B-A3B-Q4_K_M.gguf");
    let hit = |t: Tier, k: &str| profile(t).model_match.iter().any(|m| k.contains(m.as_str()));
    assert!(hit(Tier::T1, &q27) && !hit(Tier::T2, &q27));
    assert!(hit(Tier::T2, &q35) && !hit(Tier::T1, &q35));
}

// ---------------------------------------------------------------------------
// Tier ids + the persisted override.
// ---------------------------------------------------------------------------

#[test]
fn tier_ids_round_trip_and_reject_anything_else() {
    for t in [Tier::T0, Tier::T1, Tier::T2] {
        assert_eq!(Tier::parse(t.id()), Some(t));
    }
    for bad in ["", "t1", "T3", "T1 ", "gpu"] {
        assert_eq!(Tier::parse(bad), None, "{bad:?}");
    }
    assert_eq!(serde_json::to_value(Tier::T1).unwrap(), serde_json::json!("T1"));
}

#[test]
fn decode_override_accepts_only_a_tier_id() {
    assert_eq!(decode_override(Some(serde_json::json!("T2"))), Some(Tier::T2));
    assert_eq!(decode_override(Some(serde_json::json!("T9"))), None);
    assert_eq!(decode_override(Some(serde_json::json!({"tier":"T1"}))), None);
    assert_eq!(decode_override(Some(serde_json::Value::Null)), None);
    assert_eq!(decode_override(None), None);
}

#[test]
fn encode_override_persists_only_the_tier_id() {
    assert_eq!(encode_override(Some(Tier::T0)), serde_json::json!("T0"));
    assert_eq!(encode_override(None), serde_json::Value::Null);
}

#[test]
fn parse_override_arg_validates_before_anything_is_stored() {
    assert_eq!(parse_override_arg(Some("T1".into())), Ok(Some(Tier::T1)));
    assert_eq!(parse_override_arg(None), Ok(None));
    assert!(parse_override_arg(Some("T7".into())).is_err());
}

// ---------------------------------------------------------------------------
// OS readers — pure parsers over fixture text.
// ---------------------------------------------------------------------------

#[test]
fn parses_macos_sysctl_memsize_and_arm64() {
    let out = "hw.memsize: 34359738368\nhw.optional.arm64: 1\n";
    let (ram, arm) = parse_macos_sysctl(out);
    assert_eq!(ram, Some(32 * G));
    assert_eq!(arm, Some(true));
}

#[test]
fn macos_sysctl_on_intel_has_no_arm64_key() {
    // An Intel Mac prints only memsize (hw.optional.arm64 is absent → stderr, not stdout).
    let (ram, arm) = parse_macos_sysctl("hw.memsize: 17179869184\n");
    assert_eq!(ram, Some(16 * G));
    assert_eq!(arm, None);
    assert_eq!(parse_macos_sysctl("hw.optional.arm64: 0\n").1, Some(false));
}

#[test]
fn macos_sysctl_garbage_is_unknown() {
    assert_eq!(parse_macos_sysctl("hw.memsize: lots\n"), (None, None));
    assert_eq!(parse_macos_sysctl(""), (None, None));
}

#[test]
fn parses_linux_meminfo_total() {
    let text = "MemTotal:       65843212 kB\nMemFree:         1234 kB\nMemAvailable: 999 kB\n";
    assert_eq!(parse_meminfo_total(text), Some(65_843_212 * 1024));
    assert_eq!(parse_meminfo_total("MemFree: 1 kB\n"), None);
    assert_eq!(parse_meminfo_total("MemTotal: x kB\n"), None);
}

#[test]
fn parses_nvidia_smi_memory_total_taking_the_largest_gpu() {
    assert_eq!(parse_nvidia_smi_mib("8192\n24564\n"), Some(24_564 * 1024 * 1024));
    assert_eq!(parse_nvidia_smi_mib(" 16384 \r\n"), Some(16 * G));
    assert_eq!(parse_nvidia_smi_mib(""), None);
    assert_eq!(parse_nvidia_smi_mib("[N/A]\n"), None);
}

#[test]
fn parses_windows_total_physical_memory() {
    assert_eq!(parse_windows_total_memory("34359738368\r\n"), Some(32 * G));
    assert_eq!(parse_windows_total_memory(""), None);
    assert_eq!(parse_windows_total_memory("n/a"), None);
}

#[test]
fn apple_silicon_facts_mark_unified_memory() {
    let f = facts_from_parts("macos", "aarch64", Some(24 * G), Some(true), None, None);
    assert_eq!(f.unified_memory, Some(true));
    // A Rosetta-translated build reports x86_64 but the host is Apple Silicon.
    let f = facts_from_parts("macos", "x86_64", Some(24 * G), Some(true), None, None);
    assert_eq!(f.unified_memory, Some(true));
    assert_eq!(f.arch, "aarch64");
    // Linux / unknown: never claimed.
    let f = facts_from_parts("linux", "x86_64", Some(24 * G), None, None, None);
    assert_eq!(f.unified_memory, None);
}

#[test]
fn gb_formatting_is_honest_about_fractions() {
    assert_eq!(fmt_gb(32 * G), "32");
    assert_eq!(fmt_gb(23 * G / 2), "11.5");
    // Truncated, never rounded up: just under the 12 GB boundary never displays as "12".
    assert_eq!(fmt_gb(12 * G - 1), "11.9");
    assert_eq!(fmt_gb(0), "0");
}

#[test]
fn the_probe_runs_on_this_machine_without_fabricating() {
    // Live read on the test host: RAM must be readable on the CI/dev OSes we ship (macOS, Linux),
    // and whatever is unknown stays None rather than a made-up number.
    let f = probe(std::env::temp_dir().as_path());
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(f.total_ram_bytes.is_some_and(|b| b >= G), "{f:?}");
        assert!(f.disk_free_bytes.is_some(), "{f:?}");
    }
    assert_eq!(f.os, std::env::consts::OS);
}

#[test]
fn report_carries_all_three_profiles_and_the_override() {
    let f = facts_usable_tenths(150);
    let rep = build_report(f.clone(), Some(Tier::T2));
    assert_eq!(rep.recommendation.tier, Tier::T1);
    assert_eq!(rep.override_tier, Some(Tier::T2));
    assert_eq!(rep.effective, Tier::T2);
    assert_eq!(
        rep.profiles.iter().map(|p| p.tier).collect::<Vec<_>>(),
        vec![Tier::T0, Tier::T1, Tier::T2]
    );
    let rep = build_report(f, None);
    assert_eq!(rep.effective, Tier::T1);
    let json = serde_json::to_value(&rep).unwrap();
    assert!(json.get("overrideTier").is_some());
    assert!(json["facts"].get("totalRamBytes").is_some());
    assert!(json["recommendation"].get("ctxTokens").is_some());
}

#[test]
fn both_commands_are_registered_in_the_invoke_handler() {
    let src = include_str!("lib.rs");
    for cmd in ["tier::tier_recommend,", "tier::tier_set_override,"] {
        assert!(src.contains(cmd), "not registered: {cmd}");
    }
}
