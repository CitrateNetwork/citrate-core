// HUP-S1.6 (rest) — the tier drives the served context + GPU offload. RED-FIRST.
//
// Pure: a synthetic GGUF header writer feeds the bounded reader, and the plan is a function over
// (tier, hardware facts, model size, GGUF facts). No llama-server, no real model file.

use super::*;
use crate::tier::{facts_from_parts, HardwareFacts, Tier, GIB};

// ---------------------------------------------------------------------------
// A tiny GGUF v3 header writer (test-only).
// ---------------------------------------------------------------------------

enum V {
    U32(u32),
    U64(u64),
    I32(i32),
    F32(f32),
    Bool(bool),
    Str(&'static str),
    ArrU32(Vec<u32>),
    ArrStr(Vec<&'static str>),
    ArrF32(Vec<f32>),
    ArrBool(Vec<bool>),
}

fn put_str(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(&(s.len() as u64).to_le_bytes());
    b.extend_from_slice(s.as_bytes());
}

fn gguf(version: u32, kvs: &[(&str, V)]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"GGUF");
    b.extend_from_slice(&version.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes()); // tensor count
    b.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
    for (k, v) in kvs {
        put_str(&mut b, k);
        match v {
            V::U32(x) => {
                b.extend_from_slice(&4u32.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
            V::U64(x) => {
                b.extend_from_slice(&10u32.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
            V::I32(x) => {
                b.extend_from_slice(&5u32.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
            V::F32(x) => {
                b.extend_from_slice(&6u32.to_le_bytes());
                b.extend_from_slice(&x.to_le_bytes());
            }
            V::Bool(x) => {
                b.extend_from_slice(&7u32.to_le_bytes());
                b.push(u8::from(*x));
            }
            V::Str(s) => {
                b.extend_from_slice(&8u32.to_le_bytes());
                put_str(&mut b, s);
            }
            V::ArrU32(xs) => {
                b.extend_from_slice(&9u32.to_le_bytes());
                b.extend_from_slice(&4u32.to_le_bytes());
                b.extend_from_slice(&(xs.len() as u64).to_le_bytes());
                for x in xs {
                    b.extend_from_slice(&x.to_le_bytes());
                }
            }
            V::ArrStr(xs) => {
                b.extend_from_slice(&9u32.to_le_bytes());
                b.extend_from_slice(&8u32.to_le_bytes());
                b.extend_from_slice(&(xs.len() as u64).to_le_bytes());
                for x in xs {
                    put_str(&mut b, x);
                }
            }
            V::ArrF32(xs) => {
                b.extend_from_slice(&9u32.to_le_bytes());
                b.extend_from_slice(&6u32.to_le_bytes());
                b.extend_from_slice(&(xs.len() as u64).to_le_bytes());
                for x in xs {
                    b.extend_from_slice(&x.to_le_bytes());
                }
            }
            V::ArrBool(xs) => {
                b.extend_from_slice(&9u32.to_le_bytes());
                b.extend_from_slice(&7u32.to_le_bytes());
                b.extend_from_slice(&(xs.len() as u64).to_le_bytes());
                for x in xs {
                    b.push(u8::from(*x));
                }
            }
        }
    }
    b
}

/// The real header values of the bundled Gemma 4 E4B Q4_0 GGUF (read 2026-10-01 from the
/// verified file), plus tokenizer-shaped arrays the reader must skip.
fn gemma4_header() -> Vec<u8> {
    gguf(
        3,
        &[
            ("general.architecture", V::Str("gemma4")),
            ("general.sampling.top_k", V::I32(64)),
            ("general.sampling.temp", V::F32(1.0)),
            ("gemma4.block_count", V::U32(42)),
            ("gemma4.context_length", V::U32(131_072)),
            ("gemma4.embedding_length", V::U32(2560)),
            ("gemma4.attention.head_count", V::U32(8)),
            ("gemma4.attention.head_count_kv", V::U32(2)),
            ("gemma4.attention.key_length", V::U32(512)),
            ("gemma4.attention.value_length", V::U32(512)),
            (
                "gemma4.attention.sliding_window_pattern",
                V::ArrBool(vec![true, true, false]),
            ),
            (
                "tokenizer.ggml.tokens",
                V::ArrStr(vec!["<pad>", "<eos>", "hello"]),
            ),
            ("tokenizer.ggml.scores", V::ArrF32(vec![0.0, -1.0, -2.0])),
            ("tokenizer.ggml.add_bos_token", V::Bool(true)),
            ("general.file_type", V::U32(2)),
        ],
    )
}

fn read(bytes: &[u8]) -> Option<GgufFacts> {
    read_gguf_facts_from(&mut std::io::Cursor::new(bytes))
}

// ---------------------------------------------------------------------------
// (1) The bounded GGUF header reader.
// ---------------------------------------------------------------------------

#[test]
fn reads_the_architecture_keys_and_skips_tokenizer_arrays() {
    let g = read(&gemma4_header()).expect("a well-formed v3 header parses");
    assert_eq!(g.architecture.as_deref(), Some("gemma4"));
    assert_eq!(g.context_length, Some(131_072));
    assert_eq!(g.block_count, Some(42));
    assert_eq!(g.head_count, Some(8));
    assert_eq!(g.head_count_kv, Some(2));
    assert_eq!(g.embedding_length, Some(2560));
    assert_eq!(g.key_length, Some(512));
    assert_eq!(g.value_length, Some(512));
}

#[test]
fn only_the_declared_architecture_counts_for_context_length() {
    // A projector's context_length must not be mistaken for the language model's.
    let h = gguf(
        3,
        &[
            ("clip.context_length", V::U32(77)),
            ("general.architecture", V::Str("llama")),
            ("llama.context_length", V::U64(8192)),
        ],
    );
    assert_eq!(read(&h).and_then(|g| g.context_length), Some(8192));
    // Without a declared architecture nothing is guessed.
    let h = gguf(3, &[("llama.context_length", V::U32(8192))]);
    assert_eq!(read(&h).and_then(|g| g.context_length), None);
}

#[test]
fn a_per_layer_kv_head_array_reads_as_its_maximum() {
    let h = gguf(
        3,
        &[
            ("general.architecture", V::Str("x")),
            ("x.attention.head_count_kv", V::ArrU32(vec![2, 8, 4])),
        ],
    );
    assert_eq!(read(&h).and_then(|g| g.head_count_kv), Some(8));
}

#[test]
fn version_two_is_accepted_and_other_versions_are_refused() {
    let mut h = gemma4_header();
    h[4..8].copy_from_slice(&2u32.to_le_bytes());
    assert!(read(&h).is_some());
    for v in [0u32, 1, 4, 99] {
        let mut h = gemma4_header();
        h[4..8].copy_from_slice(&v.to_le_bytes());
        assert!(read(&h).is_none(), "version {v} must be refused");
    }
}

#[test]
fn malformed_headers_read_as_unknown_never_panic() {
    assert!(read(b"").is_none());
    assert!(read(b"<html>not a model</html>").is_none());
    let full = gemma4_header();
    for cut in [3, 8, 20, 30, full.len() / 2, full.len() - 1] {
        assert!(read(&full[..cut]).is_none(), "truncated at {cut}");
    }
    // An absurd kv count is refused before any allocation.
    let mut h = gemma4_header();
    h[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(read(&h).is_none());
    // An absurd key length is refused.
    let mut h = Vec::new();
    h.extend_from_slice(b"GGUF");
    h.extend_from_slice(&3u32.to_le_bytes());
    h.extend_from_slice(&0u64.to_le_bytes());
    h.extend_from_slice(&1u64.to_le_bytes());
    h.extend_from_slice(&(u64::MAX).to_le_bytes());
    assert!(read(&h).is_none());
    // An unknown value type is refused.
    let mut h = gguf(3, &[("general.architecture", V::Str("x"))]);
    let ty = 4 + 4 + 8 + 8 + 8 + "general.architecture".len();
    h[ty..ty + 4].copy_from_slice(&42u32.to_le_bytes());
    assert!(read(&h).is_none());
}

#[test]
fn a_missing_file_reads_as_unknown() {
    assert!(read_gguf_facts(std::path::Path::new("/nonexistent/citrate/model.gguf")).is_none());
}

// ---------------------------------------------------------------------------
// (2) KV-cache cost per token: an upper bound (every layer full attention, f16 K and V).
// ---------------------------------------------------------------------------

#[test]
fn kv_bytes_per_token_from_header_and_fallbacks() {
    let g = read(&gemma4_header()).unwrap();
    // 42 layers x 2 kv heads x (512 + 512) x 2 bytes.
    assert_eq!(kv_bytes_per_token(Some(&g)), 172_032);
    // key/value length absent: head_dim = embedding / head_count.
    let g2 = GgufFacts {
        key_length: None,
        value_length: None,
        ..g.clone()
    };
    assert_eq!(kv_bytes_per_token(Some(&g2)), 42 * 2 * (320 + 320) * 2);
    // kv heads absent: assume head_count (no GQA): larger, never smaller.
    let g3 = GgufFacts {
        head_count_kv: None,
        ..g.clone()
    };
    assert_eq!(kv_bytes_per_token(Some(&g3)), 42 * 8 * 1024 * 2);
    // Too little to compute: the conservative constant.
    let g4 = GgufFacts {
        block_count: None,
        ..g
    };
    assert_eq!(kv_bytes_per_token(Some(&g4)), FALLBACK_KV_BYTES_PER_TOKEN);
    assert_eq!(kv_bytes_per_token(None), FALLBACK_KV_BYTES_PER_TOKEN);
}

// ---------------------------------------------------------------------------
// (3) The selection matrix.
// ---------------------------------------------------------------------------

/// The bundled model's exact size (crate::model::MODEL_SIZE_BYTES).
const GEMMA_BYTES: u64 = crate::model::MODEL_SIZE_BYTES;

fn mac(gb: u64) -> HardwareFacts {
    facts_from_parts(
        "macos",
        "aarch64",
        Some(gb * GIB),
        Some(true),
        None,
        Some(100 * GIB),
    )
}
fn intel_mac(gb: u64) -> HardwareFacts {
    facts_from_parts(
        "macos",
        "x86_64",
        Some(gb * GIB),
        Some(false),
        None,
        Some(100 * GIB),
    )
}
fn linux(gb: u64, vram_gb: Option<u64>) -> HardwareFacts {
    facts_from_parts(
        "linux",
        "x86_64",
        Some(gb * GIB),
        None,
        vram_gb.map(|v| v * GIB),
        Some(100 * GIB),
    )
}
fn gemma() -> Option<GgufFacts> {
    read(&gemma4_header())
}
fn plan(tier: Tier, f: &HardwareFacts) -> ServePlan {
    plan_serve(tier, f, Some(GEMMA_BYTES), gemma().as_ref())
}

#[test]
fn apple_silicon_offloads_every_layer_and_gets_the_tier_context_when_it_fits() {
    let p = plan(Tier::T0, &mac(16));
    assert_eq!(p.ctx_size, 16_384, "{p:?}");
    assert_eq!(p.gpu_layers, Some(99));
    assert!(p.fits);
    let p = plan(Tier::T1, &mac(24));
    assert_eq!(p.ctx_size, 32_768, "{p:?}");
    let p = plan(Tier::T2, &mac(64));
    assert_eq!(p.ctx_size, 65_536, "{p:?}");
    assert_eq!(p.gpu_layers, Some(99));
}

#[test]
fn an_override_above_the_hardware_is_capped_by_memory_with_a_note() {
    // T2 picked on a 16 GB Mac: 64k cannot fit beside the node; the largest fitting step wins.
    let p = plan(Tier::T2, &mac(16));
    assert_eq!(p.ctx_size, 16_384, "{p:?}");
    assert!(p.fits);
    assert!(
        p.notes.iter().any(|n| n.contains("memory")),
        "{:?}",
        p.notes
    );
}

#[test]
fn too_little_memory_falls_back_to_8192_honestly() {
    let p = plan(Tier::T0, &mac(8));
    assert_eq!(p.ctx_size, 8192);
    assert!(!p.fits);
    assert!(p.notes.iter().any(|n| n.contains("8192")), "{:?}", p.notes);
}

#[test]
fn unknown_ram_falls_back_to_8192() {
    let f = facts_from_parts("linux", "x86_64", None, None, None, None);
    let p = plan(Tier::T2, &f);
    assert_eq!(p.ctx_size, 8192);
    assert!(!p.fits);
    assert_eq!(p.gpu_layers, None);
}

#[test]
fn an_unreadable_model_header_caps_the_context_at_8192() {
    let p = plan_serve(Tier::T2, &mac(64), Some(GEMMA_BYTES), None);
    assert_eq!(p.ctx_size, 8192, "{p:?}");
    assert!(
        p.notes.iter().any(|n| n.contains("header")),
        "{:?}",
        p.notes
    );
    // An unknown model size is also capped (memory cannot be checked).
    let p = plan_serve(Tier::T2, &mac(64), None, gemma().as_ref());
    assert_eq!(p.ctx_size, 8192, "{p:?}");
}

#[test]
fn the_context_never_exceeds_what_the_model_was_trained_for() {
    let short = GgufFacts {
        context_length: Some(32_768),
        ..gemma().unwrap()
    };
    let p = plan_serve(Tier::T2, &mac(64), Some(GEMMA_BYTES), Some(&short));
    assert_eq!(p.ctx_size, 32_768);
    assert!(p.notes.iter().any(|n| n.contains("32768")), "{:?}", p.notes);
    // A model trained for less than the 8192 floor gets exactly what it supports.
    let tiny = GgufFacts {
        context_length: Some(4096),
        ..gemma().unwrap()
    };
    let p = plan_serve(Tier::T0, &mac(64), Some(GEMMA_BYTES), Some(&tiny));
    assert_eq!(p.ctx_size, 4096);
}

#[test]
fn cpu_hosts_keep_their_behaviour_and_size_by_ram() {
    let p = plan(Tier::T1, &linux(32, None));
    assert_eq!(p.gpu_layers, None, "no GPU probed: no -ngl");
    assert_eq!(p.ctx_size, 32_768, "{p:?}");
    let p = plan(Tier::T1, &intel_mac(32));
    assert_eq!(p.gpu_layers, None, "Intel Mac: no offload");
}

#[test]
fn a_probed_gpu_offloads_only_when_the_model_fits_in_its_memory() {
    let p = plan(Tier::T2, &linux(32, Some(24)));
    assert_eq!(p.gpu_layers, Some(99));
    // Budget is VRAM (24 GiB): 64k = 4.28 + 1.5 + 10.5 GiB fits.
    assert_eq!(p.ctx_size, 65_536, "{p:?}");
    // 6 GiB card: the model + overhead + 8k does not fit, so it stays on the CPU.
    let p = plan(Tier::T1, &linux(32, Some(6)));
    assert_eq!(p.gpu_layers, None, "{p:?}");
    assert!(p.notes.iter().any(|n| n.contains("GPU")), "{:?}", p.notes);
    assert_eq!(p.ctx_size, 32_768);
}

#[test]
fn the_metal_working_set_limit_bounds_unified_memory() {
    assert_eq!(metal_working_set(16 * GIB), 16 * GIB * 2 / 3);
    assert_eq!(metal_working_set(64 * GIB), 48 * GIB);
    // 24 GB Mac on T2: 64k needs ~16.3 GiB. That is under the 17 GiB left beside the node but
    // over the ~16 GiB the GPU may use, so the plan steps down to 32k.
    let p = plan(Tier::T2, &mac(24));
    assert_eq!(p.budget_bytes, Some(metal_working_set(24 * GIB)));
    assert_eq!(p.ctx_size, 32_768, "{p:?}");
}

#[test]
fn every_plan_fits_its_budget_or_says_it_does_not() {
    // Property over a grid: a plan marked `fits` always fits the budget it was sized against;
    // the context never exceeds the tier's target or the model's trained length.
    let sizes = [GEMMA_BYTES, 9 * GIB, 16 * GIB, 30 * GIB];
    let ctxs = [2048u64, 8192, 32_768, 131_072, 262_144];
    for tier in [Tier::T0, Tier::T1, Tier::T2] {
        for ram in [4u64, 8, 12, 16, 18, 24, 32, 36, 48, 64, 96, 128] {
            for vram in [
                None,
                Some(4u64),
                Some(8),
                Some(16),
                Some(24),
                Some(48),
                Some(80),
            ] {
                for &size in &sizes {
                    for &c in &ctxs {
                        for f in [mac(ram), linux(ram, vram)] {
                            let g = GgufFacts {
                                context_length: Some(c),
                                ..gemma().unwrap()
                            };
                            let p = plan_serve(tier, &f, Some(size), Some(&g));
                            let target = u64::from(crate::tier::profile(tier).ctx_tokens);
                            assert!(u64::from(p.ctx_size) <= target, "{p:?}");
                            assert!(u64::from(p.ctx_size) <= c, "{p:?}");
                            if p.fits {
                                let need =
                                    required_bytes(size, kv_bytes_per_token(Some(&g)), p.ctx_size);
                                assert!(need <= p.budget_bytes.unwrap_or(0), "{p:?}");
                            } else {
                                assert!(!p.notes.is_empty());
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Manual proof against a real model file (not run in CI): set `CITRATE_GGUF_PROOF=<path>` and
/// run with `--ignored --nocapture`. Prints the header facts and the plan for each tier on this
/// machine's probed hardware.
#[test]
#[ignore]
fn manual_proof_reads_a_real_gguf_header() {
    let Ok(p) = std::env::var("CITRATE_GGUF_PROOF") else {
        return;
    };
    let path = std::path::PathBuf::from(p);
    let g = read_gguf_facts(&path).expect("a real GGUF header parses");
    assert!(g.context_length.is_some(), "{g:?}");
    let facts = crate::tier::probe(&std::env::temp_dir());
    let size = std::fs::metadata(&path).ok().map(|m| m.len());
    println!(
        "facts: {facts:?}\ngguf: {g:?}\nkv/token: {}",
        kv_bytes_per_token(Some(&g))
    );
    for t in [Tier::T0, Tier::T1, Tier::T2] {
        println!("{t:?}: {:?}", plan_serve(t, &facts, size, Some(&g)));
    }
}
