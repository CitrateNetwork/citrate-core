// =====================================================================
// HUP-S6.7 / US-6.3 AC2 — contract_view: Hermes runs a read-only view call itself.
//
// The model names a contract and a function; this module finds the function in the contract's
// ABI (CitrateScan's verified ABI, or a one-function fragment the model supplies, for example
// for a contract on the member's local fork), encodes the arguments, and asks core for an
// `eth_call` through the same `contract_view_call` command the Contract reader uses
// (src-tauri contract_reader.rs `view_call`). Only `view` and `pure` functions are called: an
// eth_call changes nothing on chain, so the tool has no effect and needs no approval. Writes
// stay in the Contract reader, where they open the Signature Ceremony.
//
// Reads go to chain 40204 or to a loopback fork URL; core refuses any other target. What comes
// back (return values, strings a contract stores) was written by whoever controls the contract,
// so it reaches the model only inside an untrusted-data fence.
// =====================================================================
import { parseAbiItem, type AbiFunction } from "viem";
import { decodeResult, encodeCall, parseAbi, parseArg, type ReaderFunction } from "../contractReader/abi";
import { fenceUntrusted } from "./untrusted";
import { isAddress, type VerifiedSourceView } from "./verifiedSource";

/** What the tool needs from core (the bridge's contracts domain). */
export interface ContractViewDeps {
  verifiedSource(address: string): Promise<VerifiedSourceView>;
  viewCall(target: string, address: string, calldata: string): Promise<string>;
}

/** The most arguments a call may carry. */
const MAX_ARGS = 16;
/** The most characters of a fragment or a function name accepted. */
const MAX_FRAGMENT = 400;

const LOOPBACK = /^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d{1,5})?(\/.*)?$/;

function str(v: unknown): string {
  return typeof v === "string" ? v.trim() : "";
}

/** The read target: "citrate" (chain 40204) unless the model names a loopback fork URL. */
export function viewTarget(raw: unknown): { ok: true; target: string } | { ok: false; error: string } {
  const t = str(raw);
  if (t === "" || t === "citrate" || t === "40204") return { ok: true, target: "citrate" };
  if (LOOPBACK.test(t)) return { ok: true, target: t };
  return { ok: false, error: "target must be citrate (chain 40204) or a local fork at an http:// loopback address" };
}

/** A one-function ABI from a fragment such as `function balanceOf(address) view returns (uint256)`. */
export function fragmentFunction(fragment: string): ReaderFunction | string {
  if (fragment.length > MAX_FRAGMENT) return "the abi_fragment is too long";
  const text = fragment.trim().startsWith("function ") ? fragment.trim() : `function ${fragment.trim()}`;
  let item: AbiFunction;
  try {
    const parsed = parseAbiItem(text);
    if (parsed.type !== "function") return "the abi_fragment must describe a function";
    item = parsed as AbiFunction;
  } catch {
    return "the abi_fragment is not a Solidity function signature, e.g. function balanceOf(address) view returns (uint256)";
  }
  const parsed = parseAbi([item]);
  if (!parsed.ok) return parsed.error;
  return parsed.functions[0];
}

/** Pick the function named `want` (a name, or a full signature like `balanceOf(address)`). */
export function pickFunction(functions: ReaderFunction[], want: string): ReaderFunction | string {
  const w = want.replace(/\s+/g, "");
  if (w === "") return "function is required (a name such as totalSupply, or a signature such as balanceOf(address))";
  const bySig = functions.find((f) => f.signature === w);
  if (bySig) return bySig;
  const byName = functions.filter((f) => f.name === w);
  if (byName.length === 1) return byName[0];
  if (byName.length > 1) {
    return `${w} is overloaded; name one signature: ${byName.map((f) => f.signature).join(", ")}`;
  }
  const reads = functions.filter((f) => f.kind === "read").map((f) => f.signature);
  return `the contract has no function ${w}. Its view functions: ${reads.slice(0, 20).join(", ") || "none"}`;
}

/** The model's arguments as the values viem encodes, checked against the inputs. */
export function encodeArgs(fn: ReaderFunction, raw: unknown): unknown[] | string {
  const list = raw === undefined || raw === null ? [] : raw;
  if (!Array.isArray(list)) return "args must be a JSON array, one value per input";
  if (list.length > MAX_ARGS) return `at most ${MAX_ARGS} arguments`;
  if (list.length !== fn.inputs.length) {
    return `${fn.signature} takes ${fn.inputs.length} argument${fn.inputs.length === 1 ? "" : "s"}, got ${list.length}`;
  }
  try {
    return fn.inputs.map((p, i) => {
      const v = list[i];
      const text = typeof v === "string" ? v : Array.isArray(v) || (typeof v === "object" && v !== null) ? JSON.stringify(v) : String(v);
      return parseArg(p, text);
    });
  } catch (e) {
    return `argument problem: ${e instanceof Error ? e.message : String(e)}`;
  }
}

/** Run one `contract_view` call. Every refusal is a plain sentence that says nothing was read. */
export async function runContractView(deps: ContractViewDeps, args: Record<string, unknown>): Promise<string> {
  const refuse = (why: string) => `contract_view: ${why}. Nothing was read.`;
  const address = str(args.address);
  if (!isAddress(address)) return refuse("address must be a contract address (0x + 40 hex)");
  const t = viewTarget(args.target);
  if (!t.ok) return refuse(t.error);
  const want = str(args.function);
  const fragment = str(args.abi_fragment);

  let fn: ReaderFunction | string;
  let abiSource: string;
  if (fragment !== "") {
    fn = fragmentFunction(fragment);
    abiSource = "the abi_fragment you gave";
    if (typeof fn !== "string" && want !== "" && want.replace(/\s+/g, "") !== fn.name && want.replace(/\s+/g, "") !== fn.signature) {
      return refuse(`function ${want} does not match the abi_fragment (${fn.signature})`);
    }
  } else {
    let src: VerifiedSourceView;
    try {
      src = await deps.verifiedSource(address);
    } catch (e) {
      return refuse(`the verified ABI lookup failed (${e instanceof Error ? e.message : String(e)}); pass abi_fragment to call it anyway`);
    }
    if (!src.abi || (src.status !== "verified" && src.status !== "partial-match")) {
      return refuse(
        `CitrateScan has no verified ABI for ${address} (status: ${src.status}); pass abi_fragment, for example "function totalSupply() view returns (uint256)"`,
      );
    }
    const parsed = parseAbi(src.abi);
    if (!parsed.ok) return refuse(`the verified ABI could not be read (${parsed.error})`);
    fn = pickFunction(parsed.functions, want);
    abiSource = "CitrateScan's verified ABI";
  }
  if (typeof fn === "string") return refuse(fn);
  if (fn.kind !== "read") {
    return refuse(
      `${fn.signature} is ${fn.stateMutability}, not a view function. contract_view only reads; a write goes through the Contract reader, where the member approves it in the Signature Ceremony`,
    );
  }
  const encoded = encodeArgs(fn, args.args);
  if (typeof encoded === "string") return refuse(encoded);
  let calldata: string;
  try {
    calldata = encodeCall(fn, encoded);
  } catch (e) {
    return refuse(`the arguments could not be encoded (${e instanceof Error ? e.message : String(e)})`);
  }
  let data: string;
  try {
    data = await deps.viewCall(t.target, address, calldata);
  } catch (e) {
    return `contract_view: the call to ${fn.signature} failed or reverted: ${e instanceof Error ? e.message : String(e)}. Nothing was changed.`;
  }
  let values: string[];
  try {
    values = decodeResult(fn, data);
  } catch (e) {
    return `contract_view: ${fn.signature} answered, but the answer could not be decoded (${e instanceof Error ? e.message : String(e)}).`;
  }
  const where = t.target === "citrate" ? "chain 40204" : `the local fork at ${t.target}`;
  const outputs = fn.outputs.map((o, i) => ({ name: o.name || `out${i}`, type: o.type, value: values[i] ?? "" }));
  return (
    `contract_view: read ${fn.signature} (${fn.stateMutability}) on ${address} (${where}), using ${abiSource}. ` +
    "This was an eth_call: it changed nothing.\n" +
    fenceUntrusted("values returned by the contract; whoever controls it chose them", outputs)
  );
}
