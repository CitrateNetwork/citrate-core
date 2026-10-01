//! HUP-S1.6 (rest) — the effective hardware tier drives what `llama-server` is started with
//! (planset 2026-09-30-hermes-upskill, D-5, 02_ARCHITECTURE §3, US-1.6).
//!
//! [`plan_serve`] is a pure decision over (effective tier, hardware facts, model file size, GGUF
//! header facts) that yields a [`ServePlan`]: the `--ctx-size` to serve and whether to pass
//! `-ngl 99` (offload every layer to the GPU).
//!
//! ## Context size
//! The target is the tier's context ([`crate::tier::profile`]: T0 16k, T1 32k, T2 64k), capped by
//! the model's trained context (`<arch>.context_length` in the GGUF header, read by the bounded
//! [`read_gguf_facts`]). The plan then walks DOWN by halves (target, target/2, …, 8192) and takes
//! the largest context whose memory need fits the budget:
//!
//! `need(ctx) = model file bytes + RUNTIME_OVERHEAD + kv_bytes_per_token × ctx`
//!
//! [`kv_bytes_per_token`] is an UPPER bound: every layer is costed as full attention with f16 K
//! and V, so sliding-window, shared-KV and hybrid (SSM) layers are over-counted. Over-counting
//! only ever picks a smaller context, never one that cannot fit.
//!
//! ## Budget
//! - **Apple Silicon (unified memory):** the tier's usable memory (total minus the node reserve)
//!   further bounded by [`metal_working_set`], an approximation of macOS's default GPU
//!   working-set limit. `-ngl 99`. (citrate-sizeup reads the exact `iogpu.wired_limit_mb`; that
//!   integration is the documented follow-up in tier.rs.)
//! - **A probed dedicated GPU (NVIDIA via `nvidia-smi`):** `-ngl 99` and the budget is VRAM, but
//!   ONLY when the model, the overhead and an 8192 context fit in VRAM; otherwise the model stays
//!   on the CPU (no `-ngl`) and is sized against usable RAM.
//! - **Everything else:** no `-ngl` flag (the existing behaviour) and usable RAM.
//!
//! ## Fallback (honest)
//! When memory, the model size or the model header cannot be read, or when even 8192 tokens do not
//! fit, the plan is 8192 tokens (or less, if the model was trained for less) with `fits = false`
//! and a plain-language note saying why. Nothing is guessed (Rule 1).

use std::io::Read;
use std::path::Path;

use serde::Serialize;

use crate::tier::{HardwareFacts, Tier, GIB, NODE_RESERVE_BYTES};

/// The floor and fallback context, the value `serve.rs` used before this WP.
pub const MIN_CTX: u32 = 8192;

/// Compute buffers, the output logits for a large vocabulary, and llama.cpp's own allocations,
/// set aside beside the weights and the KV cache. Deliberately generous.
pub const RUNTIME_OVERHEAD_BYTES: u64 = 3 * GIB / 2;

/// The KV cost assumed when the model header does not say enough to compute it (256 KiB per
/// token: a dense ~30B model with 8 KV heads; larger than every model the app offers).
pub const FALLBACK_KV_BYTES_PER_TOKEN: u64 = 256 * 1024;

/// `-ngl` value meaning "offload every layer" (llama.cpp clamps it to the layer count).
pub const ALL_LAYERS: u32 = 99;

// ---------------------------------------------------------------------------
// The plan.
// ---------------------------------------------------------------------------

/// What `llama-server` is started with, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServePlan {
    /// `--ctx-size`.
    pub ctx_size: u32,
    /// `-ngl <n>`; `None` = the flag is not passed.
    pub gpu_layers: Option<u32>,
    /// The tier the plan was sized for (`None` = the pre-plan default).
    pub tier: Option<Tier>,
    /// `true` when the chosen context was checked to fit the memory budget.
    pub fits: bool,
    /// The memory budget the plan was sized against, when known.
    pub budget_bytes: Option<u64>,
    /// Plain-language notes, one per line.
    pub notes: Vec<String>,
}

impl ServePlan {
    /// The plan in force before a hardware plan is computed: the pre-HUP 8192 window, no `-ngl`.
    pub fn not_sized() -> Self {
        ServePlan {
            ctx_size: MIN_CTX,
            gpu_layers: None,
            tier: None,
            fits: false,
            budget_bytes: None,
            notes: vec![format!(
                "Context not sized for this machine yet: using {MIN_CTX} tokens"
            )],
        }
    }
}

/// macOS's default GPU working-set limit on unified memory, approximated: about two thirds of
/// RAM up to 36 GiB, three quarters above.
pub fn metal_working_set(total_bytes: u64) -> u64 {
    if total_bytes <= 36 * GIB {
        total_bytes / 3 * 2
    } else {
        total_bytes / 4 * 3
    }
}

/// The memory a context needs beside the model (see the module docs).
pub fn required_bytes(model_bytes: u64, kv_per_token: u64, ctx: u32) -> u64 {
    model_bytes
        .saturating_add(RUNTIME_OVERHEAD_BYTES)
        .saturating_add(kv_per_token.saturating_mul(u64::from(ctx)))
}

/// Upper-bound KV-cache bytes per token: layers × KV heads × (key + value dim) × 2 bytes (f16).
/// Missing dims fall back to `embedding / heads`; missing KV heads to the full head count (no
/// grouped-query saving assumed). Too little to compute ⇒ [`FALLBACK_KV_BYTES_PER_TOKEN`].
pub fn kv_bytes_per_token(g: Option<&GgufFacts>) -> u64 {
    let Some(g) = g else {
        return FALLBACK_KV_BYTES_PER_TOKEN;
    };
    let head_dim = match (g.embedding_length, g.head_count) {
        (Some(e), Some(h)) if h > 0 => Some(e / h),
        _ => None,
    };
    let (Some(layers), Some(k), Some(v)) = (
        g.block_count,
        g.key_length.or(head_dim),
        g.value_length.or(head_dim),
    ) else {
        return FALLBACK_KV_BYTES_PER_TOKEN;
    };
    let Some(kv_heads) = g.head_count_kv.or(g.head_count) else {
        return FALLBACK_KV_BYTES_PER_TOKEN;
    };
    let per = layers
        .saturating_mul(kv_heads)
        .saturating_mul(k.saturating_add(v))
        .saturating_mul(2);
    if per == 0 {
        FALLBACK_KV_BYTES_PER_TOKEN
    } else {
        per
    }
}

fn fmt_ctx(ctx: u32) -> String {
    ctx.to_string()
}

/// **The pure decision.** Deterministic over its inputs.
pub fn plan_serve(
    tier: Tier,
    facts: &HardwareFacts,
    model_bytes: Option<u64>,
    gguf: Option<&GgufFacts>,
) -> ServePlan {
    let mut notes = Vec::new();
    let target = crate::tier::profile(tier).ctx_tokens;
    let kv = kv_bytes_per_token(gguf);
    let usable = facts
        .total_ram_bytes
        .map(|t| t.saturating_sub(NODE_RESERVE_BYTES));
    let unified = facts.unified_memory == Some(true);

    // The model's trained context bounds everything, including the fallback.
    let trained = gguf
        .and_then(|g| g.context_length)
        .map(|c| u32::try_from(c).unwrap_or(u32::MAX));
    let fallback_ctx = trained.map_or(MIN_CTX, |t| t.min(MIN_CTX));

    // GPU offload + the budget the context is sized against.
    let (gpu_layers, budget) = if unified {
        (
            Some(ALL_LAYERS),
            facts
                .total_ram_bytes
                .zip(usable)
                .map(|(t, u)| u.min(metal_working_set(t))),
        )
    } else if let Some(vram) = facts.gpu_vram_bytes {
        let fits_gpu = model_bytes.is_some_and(|m| required_bytes(m, kv, MIN_CTX) <= vram);
        if fits_gpu {
            (Some(ALL_LAYERS), Some(vram))
        } else {
            notes.push(
                "The model does not fit in the GPU's memory, so it runs on the CPU".to_string(),
            );
            (None, usable)
        }
    } else {
        (None, usable)
    };

    let fallback = |mut notes: Vec<String>, why: String| ServePlan {
        ctx_size: fallback_ctx,
        gpu_layers,
        tier: Some(tier),
        fits: false,
        budget_bytes: budget,
        notes: {
            notes.push(why);
            notes
        },
    };

    let Some(trained) = trained else {
        return fallback(
            notes,
            format!(
                "The model file's header could not be read, so the context stays at {MIN_CTX} tokens"
            ),
        );
    };
    let (Some(budget_bytes), Some(model_bytes)) = (budget, model_bytes) else {
        return fallback(
            notes,
            format!(
                "Memory or the model size could not be read, so the context stays at {MIN_CTX} tokens"
            ),
        );
    };

    let capped = target.min(trained);
    if trained < target {
        notes.push(format!(
            "This model supports up to {} tokens of context",
            fmt_ctx(trained)
        ));
    }

    // Walk down by halves from the capped target to the floor; the largest fit wins.
    let mut ctx = capped;
    loop {
        if required_bytes(model_bytes, kv, ctx) <= budget_bytes {
            if ctx < capped {
                notes.push(format!(
                    "Context reduced to {} tokens to fit this machine's free memory beside the node",
                    fmt_ctx(ctx)
                ));
            }
            return ServePlan {
                ctx_size: ctx,
                gpu_layers,
                tier: Some(tier),
                fits: true,
                budget_bytes: Some(budget_bytes),
                notes,
            };
        }
        let next = ctx / 2;
        if next < MIN_CTX || ctx <= MIN_CTX {
            break;
        }
        ctx = next;
    }
    fallback(
        notes,
        format!(
            "Not enough free memory for this model beside the node: using {} tokens, and the model may run slowly or fail to start",
            fmt_ctx(fallback_ctx)
        ),
    )
}

// ---------------------------------------------------------------------------
// The bounded GGUF header reader (metadata only; tensors are never touched).
// ---------------------------------------------------------------------------

/// The header facts the plan uses. `None` = absent from the header.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GgufFacts {
    pub architecture: Option<String>,
    /// `<arch>.context_length` (n_ctx_train).
    pub context_length: Option<u64>,
    pub block_count: Option<u64>,
    pub head_count: Option<u64>,
    /// `<arch>.attention.head_count_kv`; a per-layer array reads as its maximum.
    pub head_count_kv: Option<u64>,
    pub embedding_length: Option<u64>,
    pub key_length: Option<u64>,
    pub value_length: Option<u64>,
}

/// Total header bytes the reader will consume before giving up (tokenizer vocabularies of a few
/// hundred thousand entries are a few MiB).
const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;
/// Most metadata entries a header may declare.
const MAX_KV: u64 = 65_536;
/// Longest key.
const MAX_KEY_LEN: u64 = 1024;
/// Longest string value (chat templates are a few KiB).
const MAX_STR_LEN: u64 = 16 * 1024 * 1024;
/// Most elements in one array.
const MAX_ARRAY_LEN: u64 = 16 * 1024 * 1024;

/// Read the plan's facts from a GGUF file's header. `None` when the file is missing, is not a
/// v2/v3 GGUF, or the header is malformed or exceeds the reader's bounds. Blocking file I/O:
/// call it only off the main thread.
pub fn read_gguf_facts(path: &Path) -> Option<GgufFacts> {
    let f = std::fs::File::open(path).ok()?;
    let mut r = std::io::BufReader::new(f).take(MAX_HEADER_BYTES);
    read_gguf_facts_from(&mut r)
}

/// [`read_gguf_facts`] over any reader (fixture-tested).
pub fn read_gguf_facts_from<R: Read>(r: &mut R) -> Option<GgufFacts> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).ok()?;
    if magic != crate::model::GGUF_MAGIC {
        return None;
    }
    let version = read_u32(r)?;
    if !(2..=3).contains(&version) {
        return None;
    }
    let _tensor_count = read_u64(r)?;
    let kv_count = read_u64(r)?;
    if kv_count > MAX_KV {
        return None;
    }

    let mut arch: Option<String> = None;
    let mut ints: Vec<(String, u64)> = Vec::new();
    for _ in 0..kv_count {
        let key = read_string(r, MAX_KEY_LEN)?;
        let ty = read_u32(r)?;
        if key == "general.architecture" && ty == T_STRING {
            arch = Some(read_string(r, MAX_KEY_LEN)?);
            continue;
        }
        if let Some(v) = read_value(r, ty, 0)? {
            if !key.starts_with("tokenizer.") {
                ints.push((key, v));
            }
        }
    }

    let mut g = GgufFacts::default();
    if let Some(a) = &arch {
        let get = |suffix: &str| {
            let k = format!("{a}.{suffix}");
            ints.iter().find(|(key, _)| *key == k).map(|(_, v)| *v)
        };
        g.context_length = get("context_length");
        g.block_count = get("block_count");
        g.head_count = get("attention.head_count");
        g.head_count_kv = get("attention.head_count_kv");
        g.embedding_length = get("embedding_length");
        g.key_length = get("attention.key_length");
        g.value_length = get("attention.value_length");
    }
    g.architecture = arch;
    Some(g)
}

const T_U8: u32 = 0;
const T_I8: u32 = 1;
const T_U16: u32 = 2;
const T_I16: u32 = 3;
const T_U32: u32 = 4;
const T_I32: u32 = 5;
const T_F32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;
const T_U64: u32 = 10;
const T_I64: u32 = 11;
const T_F64: u32 = 12;

/// Width of a fixed-size scalar type; `None` for strings, arrays and unknown types.
fn scalar_width(ty: u32) -> Option<u64> {
    match ty {
        T_U8 | T_I8 | T_BOOL => Some(1),
        T_U16 | T_I16 => Some(2),
        T_U32 | T_I32 | T_F32 => Some(4),
        T_U64 | T_I64 | T_F64 => Some(8),
        _ => None,
    }
}

/// Read one value. Outer `None` = malformed (stop); inner `Some(n)` = a non-negative integer (or
/// the maximum of an integer array), inner `None` = anything else (skipped).
fn read_value<R: Read>(r: &mut R, ty: u32, depth: u8) -> Option<Option<u64>> {
    match ty {
        T_U8 => Some(Some(u64::from(read_n::<1, _>(r)?[0]))),
        T_U16 => Some(Some(u64::from(u16::from_le_bytes(read_n::<2, _>(r)?)))),
        T_U32 => Some(Some(u64::from(read_u32(r)?))),
        T_U64 => Some(Some(read_u64(r)?)),
        T_I8 => Some(u64::try_from(i8::from_le_bytes(read_n::<1, _>(r)?)).ok()),
        T_I16 => Some(u64::try_from(i16::from_le_bytes(read_n::<2, _>(r)?)).ok()),
        T_I32 => Some(u64::try_from(i32::from_le_bytes(read_n::<4, _>(r)?)).ok()),
        T_I64 => Some(u64::try_from(i64::from_le_bytes(read_n::<8, _>(r)?)).ok()),
        T_F32 | T_F64 | T_BOOL => {
            skip(r, scalar_width(ty)?)?;
            Some(None)
        }
        T_STRING => {
            let n = read_u64(r)?;
            if n > MAX_STR_LEN {
                return None;
            }
            skip(r, n)?;
            Some(None)
        }
        T_ARRAY => {
            if depth > 1 {
                return None;
            }
            let ety = read_u32(r)?;
            let n = read_u64(r)?;
            if n > MAX_ARRAY_LEN {
                return None;
            }
            let integer = matches!(
                ety,
                T_U8 | T_I8 | T_U16 | T_I16 | T_U32 | T_I32 | T_U64 | T_I64
            );
            if !integer {
                if let Some(w) = scalar_width(ety) {
                    skip(r, n.checked_mul(w)?)?;
                    return Some(None);
                }
            }
            let mut max: Option<u64> = None;
            for _ in 0..n {
                let v = read_value(r, ety, depth + 1)?;
                if integer {
                    max = max.max(v);
                }
            }
            Some(max)
        }
        _ => None,
    }
}

fn read_n<const N: usize, R: Read>(r: &mut R) -> Option<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b).ok()?;
    Some(b)
}

fn read_u32<R: Read>(r: &mut R) -> Option<u32> {
    Some(u32::from_le_bytes(read_n::<4, _>(r)?))
}

fn read_u64<R: Read>(r: &mut R) -> Option<u64> {
    Some(u64::from_le_bytes(read_n::<8, _>(r)?))
}

fn read_string<R: Read>(r: &mut R, max: u64) -> Option<String> {
    let n = read_u64(r)?;
    if n > max {
        return None;
    }
    let mut buf = Vec::new();
    r.take(n).read_to_end(&mut buf).ok()?;
    if buf.len() as u64 != n {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// Skip exactly `n` bytes; `None` on a short read.
fn skip<R: Read>(r: &mut R, n: u64) -> Option<()> {
    let copied = std::io::copy(&mut r.take(n), &mut std::io::sink()).ok()?;
    (copied == n).then_some(())
}

// ---------------------------------------------------------------------------
// App wiring: the effective tier (probe + stored override) for a model file.
// ---------------------------------------------------------------------------

/// Compute the plan for serving `model_path` on this machine: probe the hardware, apply the
/// member's stored tier override, read the model's size and header. Blocking (spawns `sysctl` /
/// `nvidia-smi`, reads the file header): call it only via [`crate::blocking::off_main`].
pub fn plan_for_model<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    model_path: &Path,
) -> Result<ServePlan, String> {
    use tauri::Manager;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let facts = crate::tier::probe(&data_dir);
    let override_tier = crate::tier::load_override(app)?;
    let report = crate::tier::build_report(facts, override_tier);
    let model_bytes = std::fs::metadata(model_path).ok().map(|m| m.len());
    let gguf = read_gguf_facts(model_path);
    Ok(plan_serve(
        report.effective,
        &report.facts,
        model_bytes,
        gguf.as_ref(),
    ))
}

#[cfg(test)]
mod tests {
    include!("serve_plan_tests.rs");
}
