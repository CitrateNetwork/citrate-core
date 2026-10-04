// =====================================================================
// citrate-core — the 0x0110 BELNAP_AGGREGATE input encoder and output decoder (US-9.2 AC2).
//
// Pure byte-level helpers behind the read-only `belnap_codec` agent tool. They follow the layout
// in the bundled skill src-tauri/skills/citrate-belnap-aggregate/SKILL.md, whose authority is
// citrate-chain core/execution/src/precompiles/q16/belnap.rs (`decode`, `encode_output`):
//
//   input  = dim u32 | n u32 | embeddings n*dim i64 | confidences n*dim i64 | weights n i64
//            | threshold_pos i64 | threshold_neg i64           (big-endian, Q16.16 in i64)
//   output = dim aggregated i64 values, then dim state bytes  (0 Neither, 1 True, 2 False, 3 Both)
//
// Nothing here calls the chain or signs anything: preparing and reading 0x0110 data is local.
// BigInt keeps every i64 exact. src/agent/belnap.test.ts checks both directions against the
// skill's worked example (the bytes chain 40204 returned on 2026-10-01).
// =====================================================================

/** Q16.16: real value = raw / 65536. */
export const Q16_ONE = 65536;
/** `MAX_DIM` / `MAX_N` in belnap.rs. */
export const BELNAP_MAX = 1024;
export const BELNAP_ADDRESS = "0x0000000000000000000000000000000000000110";

const I64_MIN = -(2n ** 63n);
const I64_MAX = 2n ** 63n - 1n;

export const BELNAP_STATES: Readonly<Record<number, string>> = { 0: "Neither", 1: "True", 2: "False", 3: "Both" };

export interface BelnapParticipant {
  embedding: number[];
  confidence: number[];
  weight: number;
}

export interface BelnapEncodeInput {
  participants: BelnapParticipant[];
  thresholdPos: number;
  thresholdNeg: number;
}

export interface BelnapEncoded {
  address: string;
  dim: number;
  n: number;
  bytes: number;
  inputHex: string;
  /** The larger gas figure the skill says to budget: 2000 + 50 * dim * max(n, 1). */
  gasBudget: number;
  outputBytes: number;
}

export interface BelnapDecoded {
  dim: number;
  values: number[];
  rawValues: string[];
  states: { dimension: number; code: number; name: string }[];
}

/** A real number to Q16 (round half away from zero, like Rust's f64::round), clamped to i64. */
export function toQ16(x: number): bigint {
  if (!Number.isFinite(x)) throw new Error(`not a finite number: ${String(x)}`);
  const scaled = x * Q16_ONE;
  const r = BigInt(Math.sign(scaled) * Math.round(Math.abs(scaled)));
  return r < I64_MIN ? I64_MIN : r > I64_MAX ? I64_MAX : r;
}

function hexU32(v: number): string {
  return v.toString(16).padStart(8, "0");
}

function hexI64(v: bigint): string {
  return BigInt.asUintN(64, v).toString(16).padStart(16, "0");
}

function isNumList(v: unknown): v is number[] {
  return Array.isArray(v) && v.every((x) => typeof x === "number" && Number.isFinite(x));
}

/** Encode a 0x0110 input. Throws (with a plain reason) on anything the precompile would refuse. */
export function encodeBelnapInput(inp: BelnapEncodeInput): BelnapEncoded {
  const ps = inp.participants;
  if (!Array.isArray(ps) || ps.length < 1 || ps.length > BELNAP_MAX) {
    throw new Error(`participants must be a list of 1 to ${BELNAP_MAX}`);
  }
  const first = ps[0];
  const dim = Array.isArray(first?.embedding) ? first.embedding.length : 0;
  if (dim < 1 || dim > BELNAP_MAX) throw new Error(`each embedding needs 1 to ${BELNAP_MAX} dimensions`);
  ps.forEach((p, i) => {
    if (!isNumList(p?.embedding) || p.embedding.length !== dim) {
      throw new Error(`participant ${i}: embedding must be ${dim} numbers (every participant needs the same dimension)`);
    }
    if (!isNumList(p.confidence) || p.confidence.length !== dim) {
      throw new Error(`participant ${i}: confidence must be ${dim} numbers`);
    }
    if (typeof p.weight !== "number" || !Number.isFinite(p.weight)) throw new Error(`participant ${i}: weight must be a number`);
  });
  if (typeof inp.thresholdPos !== "number" || !Number.isFinite(inp.thresholdPos)) throw new Error("thresholdPos must be a number");
  if (typeof inp.thresholdNeg !== "number" || !Number.isFinite(inp.thresholdNeg)) throw new Error("thresholdNeg must be a number");
  const n = ps.length;
  let hex = hexU32(dim) + hexU32(n);
  for (const p of ps) for (const e of p.embedding) hex += hexI64(toQ16(e));
  for (const p of ps) for (const c of p.confidence) hex += hexI64(toQ16(c));
  for (const p of ps) hex += hexI64(toQ16(p.weight));
  hex += hexI64(toQ16(inp.thresholdPos)) + hexI64(toQ16(inp.thresholdNeg));
  const bytes = hex.length / 2;
  if (bytes !== 24 + 16 * n * dim + 8 * n) throw new Error("internal: encoded length does not match the layout");
  return {
    address: BELNAP_ADDRESS,
    dim,
    n,
    bytes,
    inputHex: "0x" + hex,
    gasBudget: 2000 + 50 * dim * Math.max(n, 1),
    outputBytes: 9 * dim,
  };
}

/** Normalise a hex string: optional 0x, whitespace ignored, even length, hex digits only. */
export function cleanHex(s: string): string {
  const h = s.replace(/\s+/g, "").replace(/^0x/i, "").toLowerCase();
  if (!/^[0-9a-f]*$/.test(h) || h.length % 2 !== 0) throw new Error("not an even-length hex string");
  return h;
}

/** Decode a 0x0110 output of `dim` dimensions: values block, then states block. */
export function decodeBelnapOutput(outputHex: string, dim: number): BelnapDecoded {
  if (!Number.isInteger(dim) || dim < 1 || dim > BELNAP_MAX) throw new Error(`dim must be an integer from 1 to ${BELNAP_MAX}`);
  const h = cleanHex(outputHex);
  if (h.length / 2 !== 9 * dim) {
    throw new Error(`a ${dim}-dimension output is ${9 * dim} bytes; this one is ${h.length / 2}`);
  }
  const rawValues: string[] = [];
  const values: number[] = [];
  for (let d = 0; d < dim; d++) {
    const v = BigInt.asIntN(64, BigInt("0x" + h.slice(d * 16, d * 16 + 16)));
    rawValues.push(v.toString());
    values.push(Number(v) / Q16_ONE);
  }
  const states = [];
  for (let d = 0; d < dim; d++) {
    const at = dim * 16 + d * 2;
    const code = parseInt(h.slice(at, at + 2), 16);
    const name = BELNAP_STATES[code];
    if (!name) throw new Error(`dimension ${d}: state byte ${code} is not a Belnap value`);
    states.push({ dimension: d, code, name });
  }
  return { dim, values, rawValues, states };
}

/** The `belnap_codec` agent tool: `mode` "encode" or "decode". Returns JSON text, or a plain
 *  sentence saying what was wrong (nothing is ever guessed). */
export function belnapCodecTool(args: Record<string, unknown>): string {
  try {
    if (args.mode === "encode") {
      return JSON.stringify(
        encodeBelnapInput({
          participants: args.participants as BelnapParticipant[],
          thresholdPos: args.thresholdPos as number,
          thresholdNeg: args.thresholdNeg as number,
        }),
      );
    }
    if (args.mode === "decode") {
      if (typeof args.outputHex !== "string") throw new Error("decode needs outputHex");
      const dim = typeof args.dim === "string" ? Number(args.dim) : (args.dim as number);
      return JSON.stringify(decodeBelnapOutput(args.outputHex, dim));
    }
    return 'belnap_codec needs mode "encode" or "decode". Nothing was computed.';
  } catch (e) {
    return `belnap_codec: ${e instanceof Error ? e.message : String(e)}. Nothing was computed.`;
  }
}
