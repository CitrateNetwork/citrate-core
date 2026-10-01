// =====================================================================
// citrate-core — Contract reader ABI handling (HUP-S6.7, US-6.3)
//
// The ABI comes from CitrateScan's verified source or is pasted by the member; either way it is
// untrusted input and is checked before use. This module turns it into a list of callable
// functions, turns the member's typed inputs into calldata, and decodes what a read returns. It
// never signs or sends anything: reads go through `contract_view_call` and writes through the
// SignatureCeremony, both in the main window.
// =====================================================================
import { decodeFunctionResult, encodeFunctionData, getAddress, isAddress, toFunctionSelector, type Abi, type AbiFunction } from "viem";

export interface AbiParam {
  name?: string;
  type: string;
  components?: AbiParam[];
}

export interface ReaderFunction {
  name: string;
  /** `name(type,type)`, the canonical signature. */
  signature: string;
  /** The 4-byte selector, `0x`-hex. */
  selector: string;
  /** `read` = view/pure (an eth_call, changes nothing); `write` = needs a signed transaction. */
  kind: "read" | "write";
  payable: boolean;
  stateMutability: string;
  inputs: AbiParam[];
  outputs: AbiParam[];
  /** The ABI item itself, for viem. */
  item: AbiFunction;
}

export type AbiParseResult = { ok: true; functions: ReaderFunction[] } | { ok: false; error: string };

const IDENT = /^[A-Za-z_$][A-Za-z0-9_$]{0,127}$/;
const TYPE = /^(?:u?int(?:8|16|24|32|40|48|56|64|72|80|88|96|104|112|120|128|136|144|152|160|168|176|184|192|200|208|216|224|232|240|248|256)?|address|bool|string|bytes(?:[1-9]|[12][0-9]|3[0-2])?|tuple)(?:\[\d{0,6}\])*$/;
const MAX_FUNCTIONS = 500;

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function checkParam(p: unknown, depth = 0): AbiParam | null {
  if (!isObj(p) || typeof p.type !== "string" || !TYPE.test(p.type) || depth > 8) return null;
  if (p.name !== undefined && p.name !== "" && (typeof p.name !== "string" || !IDENT.test(p.name))) return null;
  const out: AbiParam = { type: p.type, ...(typeof p.name === "string" && p.name !== "" ? { name: p.name } : {}) };
  if (p.type.startsWith("tuple")) {
    if (!Array.isArray(p.components)) return null;
    const comps = p.components.map((c) => checkParam(c, depth + 1));
    if (comps.some((c) => c === null)) return null;
    out.components = comps as AbiParam[];
  }
  return out;
}

/** The canonical type of a parameter (tuples expand to `(a,b)`). */
function canonicalType(p: AbiParam): string {
  if (!p.type.startsWith("tuple")) return p.type;
  return "(" + (p.components ?? []).map(canonicalType).join(",") + ")" + p.type.slice("tuple".length);
}

/** Parse an ABI (an array, or JSON text of one). Only functions are kept. */
export function parseAbi(raw: unknown): AbiParseResult {
  let value = raw;
  if (typeof raw === "string") {
    try {
      value = JSON.parse(raw);
    } catch {
      return { ok: false, error: "The ABI is not valid JSON." };
    }
  }
  if (!Array.isArray(value)) return { ok: false, error: "An ABI is a JSON array of entries." };
  const functions: ReaderFunction[] = [];
  for (const entry of value) {
    if (!isObj(entry)) return { ok: false, error: "Every ABI entry must be an object." };
    if (entry.type !== "function") continue;
    if (typeof entry.name !== "string" || !IDENT.test(entry.name)) return { ok: false, error: "An ABI function has an invalid name." };
    const inputs = (Array.isArray(entry.inputs) ? entry.inputs : []).map((p) => checkParam(p));
    const outputs = (Array.isArray(entry.outputs) ? entry.outputs : []).map((p) => checkParam(p));
    if (inputs.some((p) => p === null) || outputs.some((p) => p === null)) {
      return { ok: false, error: `The ABI entry for ${entry.name} has an invalid parameter.` };
    }
    const mut =
      typeof entry.stateMutability === "string"
        ? entry.stateMutability
        : entry.constant === true
          ? "view"
          : entry.payable === true
            ? "payable"
            : "nonpayable";
    if (!["view", "pure", "nonpayable", "payable"].includes(mut)) {
      return { ok: false, error: `The ABI entry for ${entry.name} has an unknown state mutability.` };
    }
    const ins = inputs as AbiParam[];
    const outs = outputs as AbiParam[];
    const signature = `${entry.name}(${ins.map(canonicalType).join(",")})`;
    const item = { type: "function", name: entry.name, inputs: ins, outputs: outs, stateMutability: mut } as unknown as AbiFunction;
    functions.push({
      name: entry.name,
      signature,
      selector: toFunctionSelector(signature),
      kind: mut === "view" || mut === "pure" ? "read" : "write",
      payable: mut === "payable",
      stateMutability: mut,
      inputs: ins,
      outputs: outs,
      item,
    });
    if (functions.length > MAX_FUNCTIONS) return { ok: false, error: "The ABI has too many functions." };
  }
  if (functions.length === 0) return { ok: false, error: "The ABI has no functions." };
  const bySig = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  functions.sort((a, b) => (a.kind === b.kind ? bySig(a.signature, b.signature) : a.kind === "read" ? -1 : 1));
  return { ok: true, functions };
}

function parseJsonInput(text: string, what: string): unknown[] {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch {
    throw new Error(`${what} needs a JSON array, e.g. [1, 2]`);
  }
  if (!Array.isArray(v)) throw new Error(`${what} needs a JSON array, e.g. [1, 2]`);
  return v;
}

function fromJson(p: AbiParam, v: unknown): unknown {
  if (typeof v === "string") return parseArg(p, v);
  if (typeof v === "number" || typeof v === "boolean") return parseArg(p, String(v));
  if (Array.isArray(v)) return parseArg(p, JSON.stringify(v));
  throw new Error(`${p.type}: unsupported value`);
}

/** Turn the member's text for one parameter into the value viem encodes. */
export function parseArg(p: AbiParam, text: string): unknown {
  const t = text.trim();
  const arr = /^(.*)\[(\d*)\]$/.exec(p.type);
  if (arr) {
    const inner: AbiParam = { ...p, type: arr[1] };
    const items = parseJsonInput(t, p.type);
    if (arr[2] !== "" && items.length !== Number(arr[2])) throw new Error(`${p.type} needs exactly ${arr[2]} items`);
    return items.map((v) => fromJson(inner, v));
  }
  if (p.type === "tuple") {
    const comps = p.components ?? [];
    const items = parseJsonInput(t, "a tuple");
    if (items.length !== comps.length) throw new Error(`the tuple needs ${comps.length} items`);
    const named = comps.every((c) => c.name);
    const vals = comps.map((c, i) => fromJson(c, items[i]));
    return named ? Object.fromEntries(comps.map((c, i) => [c.name as string, vals[i]])) : vals;
  }
  if (/^u?int\d*$/.test(p.type)) {
    if (!/^-?(?:\d+|0x[0-9a-fA-F]+)$/.test(t)) throw new Error(`${p.type} needs a whole number`);
    const n = t.startsWith("-") ? -BigInt(t.slice(1)) : BigInt(t);
    if (p.type.startsWith("uint") && n < 0n) throw new Error(`${p.type} cannot be negative`);
    return n;
  }
  if (p.type === "bool") {
    if (t === "true") return true;
    if (t === "false") return false;
    throw new Error("bool needs true or false");
  }
  if (p.type === "address") {
    if (!isAddress(t, { strict: false })) throw new Error("not a valid address");
    return getAddress(t);
  }
  if (p.type.startsWith("bytes")) {
    if (!/^0x(?:[0-9a-fA-F]{2})*$/.test(t)) throw new Error(`${p.type} needs 0x hex`);
    const size = p.type.slice(5);
    if (size !== "" && (t.length - 2) / 2 !== Number(size)) throw new Error(`${p.type} needs exactly ${size} bytes`);
    return t.toLowerCase();
  }
  if (p.type === "string") return text;
  throw new Error(`${p.type} is not supported`);
}

/** The calldata for a call of `fn` with already-parsed arguments. */
export function encodeCall(fn: ReaderFunction, args: unknown[]): `0x${string}` {
  if (args.length !== fn.inputs.length) {
    throw new Error(`${fn.signature} takes ${fn.inputs.length} input${fn.inputs.length === 1 ? "" : "s"}`);
  }
  return encodeFunctionData({ abi: [fn.item] as Abi, functionName: fn.name, args } as Parameters<typeof encodeFunctionData>[0]);
}

function display(v: unknown): string {
  if (typeof v === "bigint") return v.toString();
  if (typeof v === "string") return v;
  if (typeof v === "boolean") return String(v);
  return JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? x.toString() : x));
}

/** Decode a read's return data into one display string per output. */
export function decodeResult(fn: ReaderFunction, data: string): string[] {
  if (!/^0x(?:[0-9a-fA-F]{2})*$/.test(data)) throw new Error("the node returned something that is not hex");
  if (fn.outputs.length === 0) return [];
  if (data === "0x") throw new Error("the call returned nothing (is there a contract at this address on this network?)");
  const out = decodeFunctionResult({ abi: [fn.item] as Abi, functionName: fn.name, data: data as `0x${string}` } as Parameters<typeof decodeFunctionResult>[0]);
  const vals = fn.outputs.length === 1 ? [out] : Array.isArray(out) ? out : Object.values(out as object);
  return vals.map(display);
}
