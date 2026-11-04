// =====================================================================
// CORE-A1 / CORE-A2 — Hermes finds, reads, identifies and decodes contracts.
//
// Six read-only agent tools. Core does every network read (src-tauri/src/contract_intel.rs); this
// module validates the model's arguments, decodes with viem, and formats the answer. Nothing here
// signs, sends or changes anything, so none of these tools needs an approval.
//
// Data sources (Rule 7), per tool:
// - address_book_lookup: the canonical 40204 address book embedded in this build, plus eth_call
//   reads of the ModelRegistry, SkillRegistry and AgentSBT named in it (core `contract_address_book_lookup`).
// - get_code: eth_getCode / eth_getStorageAt on the 40204 RPC and CitrateScan's verified-source
//   lookup (core `contract_get_code`).
// - identify_contract: get_code's reads plus ERC-165 and ERC-20 eth_calls and the code's PUSH4
//   constants matched against the bundled signature subset (core `contract_identify`).
// - decode_calldata / decode_log: the ABI from core `contract_abi_resolve` (CitrateScan verified,
//   then the bundled system ABI of a book contract, then a partial match), then the bundled
//   selector/topic subset (core `abi_signature_lookup`). No signature service is asked.
// - explain_tx: eth_getTransactionByHash + eth_getTransactionReceipt (core `chain_tx_fetch`), then
//   the decoders above.
//
// What a contract returns or emits, the names people registered, and verified source text are
// chosen by whoever deployed or called the contract, so they reach the model only inside an
// untrusted-data fence. What could not be read or decoded is said to be unknown.
// =====================================================================
import { decodeEventLog, decodeFunctionData, formatEther, parseAbiItem, type Abi, type AbiEvent, type AbiFunction, type Hex } from "viem";
import { fenceUntrusted } from "./untrusted";
import { isAddress } from "./verifiedSource";

// ---- views of core's answers (mirror the Rust structs, serde camelCase) ----

export interface BookEntryView {
  name: string;
  address: string;
  section: string;
  abiContract: string | null;
  proxy: boolean;
}
export interface RegistryResultView {
  registry: string;
  address: string | null;
  status: "read" | "unavailable" | "not-searchable" | "not-in-book";
  matches: Record<string, unknown>[];
  note: string;
}
export interface BookLookupView {
  query: string;
  kind: "address" | "name";
  chainId: number;
  genesisHash: string;
  book: BookEntryView[];
  registries: RegistryResultView[];
  note: string;
}
export interface ProxyInfoView {
  kind: string;
  implementation: string;
  beacon: string | null;
  implementationBookNames: string[];
  implementationCodeSize: number | null;
}
export interface VerifiedStatusView {
  status: "verified" | "partial-match" | "unverified" | "unavailable";
  contractName: string | null;
  note: string;
}
export interface CodeInfoView {
  address: string;
  bookNames: string[];
  isContract: boolean;
  codeSize: number;
  codeHash: string | null;
  proxy: ProxyInfoView | null;
  verified: VerifiedStatusView;
  note: string;
}
export interface IdentityView extends CodeInfoView {
  abiSource: "verified" | "partial-match" | "book" | "none";
  abiContract: string | null;
  erc165: "yes" | "no" | "unknown";
  interfaces: Record<string, string>;
  erc20: {
    verdict: "likely" | "no" | "unknown";
    selectorsPresent: string[];
    selectorsMissing: string[];
    totalSupply: string | null;
    decimals: number | null;
    symbol: string | null;
    name: string | null;
  };
  knownSelectors: { selector: string; signatures: string[] }[];
  unknownPush4Count: number;
}
export interface AbiResolutionView {
  address: string;
  source: "verified" | "book" | "partial-match" | "none";
  abiAddress: string | null;
  contractName: string | null;
  abi: unknown[] | null;
  proxy: ProxyInfoView | null;
  bookNames: string[];
  note: string;
}
export interface SignatureMatchesView {
  functions: Record<string, string[]>;
  events: Record<string, string[]>;
  errors: Record<string, string[]>;
}
export interface TxLogView {
  address: string;
  topics: string[];
  data: string;
  logIndex: string | null;
}
export interface TxFetchView {
  hash: string;
  found: boolean;
  from: string | null;
  to: string | null;
  valueWei: string | null;
  input: string | null;
  inputTruncated: boolean;
  nonce: string | null;
  blockNumber: string | null;
  status: "success" | "reverted" | "pending";
  gasUsed: string | null;
  contractAddress: string | null;
  logs: TxLogView[];
  logsTruncated: boolean;
  names: Record<string, string[]>;
}

/** What the tools need from core (the bridge's contracts domain). */
export interface ContractIntelDeps {
  addressBookLookup(query: string): Promise<BookLookupView>;
  getCode(address: string): Promise<CodeInfoView>;
  identify(address: string): Promise<IdentityView>;
  abiResolve(address: string): Promise<AbiResolutionView>;
  signatureLookup(selectors: string[], topics: string[]): Promise<SignatureMatchesView>;
  txFetch(hash: string): Promise<TxFetchView>;
}

const HEX = /^0x(?:[0-9a-fA-F]{2})*$/;
const TX_HASH = /^0x[0-9a-fA-F]{64}$/;
/** The most logs explain_tx decodes; the rest are counted. */
export const MAX_EXPLAINED_LOGS = 20;
/** The most distinct log emitters whose ABI explain_tx resolves (each costs explorer + RPC reads). */
export const MAX_RESOLVED_EMITTERS = 6;
/** The longest calldata decode_calldata accepts (hex chars, after 0x). */
const MAX_CALLDATA_HEX = 2 * 64 * 1024;

function str(v: unknown): string {
  return typeof v === "string" ? v.trim() : "";
}
function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
function json(v: unknown): unknown {
  return JSON.parse(JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? x.toString() : x)));
}

// ---- address_book_lookup ----

export async function runAddressBookLookup(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const query = str(args.query);
  if (query === "") return "address_book_lookup needs a contract name (for example WrappedSALT or skill registry) or a 0x address. Nothing was looked up.";
  let r: BookLookupView;
  try {
    r = await deps.addressBookLookup(query);
  } catch (e) {
    return `address_book_lookup failed: ${errText(e)}. Nothing was found, and nothing is known about "${query}".`;
  }
  const lines = [
    `address_book_lookup "${r.query}" (${r.kind}) on chain ${r.chainId}, genesis ${r.genesisHash.slice(0, 10)}…`,
    `Source: this build's embedded 40204 address book, and live reads of the on-chain registries it names. ${r.note}`,
  ];
  for (const e of r.book) {
    const abi = e.section === "precompiles" ? "precompile" : e.abiContract ? `bundled ABI ${e.abiContract.split(":").pop()}${e.proxy ? ", proxy" : ""}` : "no bundled ABI";
    lines.push(`- ${e.name} = ${e.address} (${e.section}; ${abi})`);
  }
  const regLines = r.registries.map((g) => `${g.registry}: ${g.status}. ${g.note}`);
  lines.push(...regLines);
  const found = r.registries.filter((g) => g.matches.length > 0).map((g) => ({ registry: g.registry, address: g.address, matches: g.matches }));
  return lines.join("\n") + (found.length ? "\n" + fenceUntrusted("on-chain registry entries; names and descriptions are written by whoever registered them", found) : "");
}

// ---- get_code ----

function proxyLine(p: ProxyInfoView | null): string {
  if (!p) return "Proxy: none detected (EIP-1167 code, EIP-1967 implementation/beacon slots and the EIP-1822 slot were checked).";
  const names = p.implementationBookNames.length ? ` (${p.implementationBookNames.join(", ")} in the address book)` : " (not in the address book)";
  const size = p.implementationCodeSize == null ? "its code size could not be read" : `${p.implementationCodeSize} bytes of code`;
  return `Proxy: ${p.kind}${p.beacon ? ` via beacon ${p.beacon}` : ""}, implementation ${p.implementation}${names}, ${size}.`;
}

function verifiedLine(v: VerifiedStatusView): string {
  const name = v.contractName ? ` Contract name on CitrateScan: ${JSON.stringify(v.contractName.slice(0, 80))}.` : "";
  return `CitrateScan verification: ${v.status}.${name} ${v.note}`;
}

export async function runGetCode(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const address = str(args.address);
  if (!isAddress(address)) return "get_code needs an address (0x + 40 hex). Nothing was read.";
  let c: CodeInfoView;
  try {
    c = await deps.getCode(address);
  } catch (e) {
    return `get_code: the code of ${address} could not be read (${errText(e)}). It is unknown whether a contract is there.`;
  }
  const lines = [`get_code ${c.address} (source: eth_getCode / eth_getStorageAt on chain 40204, CitrateScan for verification).`];
  lines.push(c.bookNames.length ? `Address book: ${c.bookNames.join(", ")}.` : "Address book: not in this build's address book.");
  if (!c.isContract) {
    lines.push(c.note);
    return lines.join("\n");
  }
  lines.push(`Code: ${c.codeSize} bytes, keccak256 ${c.codeHash}.`, proxyLine(c.proxy), verifiedLine(c.verified));
  return lines.join("\n");
}

// ---- identify_contract ----

export async function runIdentifyContract(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const address = str(args.address);
  if (!isAddress(address)) return "identify_contract needs an address (0x + 40 hex). Nothing was read.";
  let id: IdentityView;
  try {
    id = await deps.identify(address);
  } catch (e) {
    return `identify_contract: ${address} could not be read (${errText(e)}). What it is stays unknown.`;
  }
  const lines = [
    `identify_contract ${id.address} (sources: this build's address book, chain 40204 code and eth_calls, CitrateScan, the bundled signature subset).`,
    id.bookNames.length ? `Address book name: ${id.bookNames.join(", ")}.` : "Address book name: none (not in this build's book).",
  ];
  if (!id.isContract) {
    lines.push(id.note);
    return lines.join("\n");
  }
  const abiWord: Record<IdentityView["abiSource"], string> = {
    verified: "CitrateScan verified ABI",
    "partial-match": "CitrateScan partial match only (not verified)",
    book: `bundled system ABI${id.abiContract ? ` (${id.abiContract.split(":").pop()})` : ""}`,
    none: "none (no verified or bundled ABI; calls decode only by the signature subset)",
  };
  lines.push(`Code: ${id.codeSize} bytes, keccak256 ${id.codeHash}.`, proxyLine(id.proxy), verifiedLine(id.verified), `ABI source: ${abiWord[id.abiSource] ?? id.abiSource}.`);
  const ifaces = Object.entries(id.interfaces)
    .map(([k, v]) => `${k} ${v}`)
    .join(", ");
  lines.push(`ERC-165: ${id.erc165}${ifaces ? ` (${ifaces})` : ""}.`);
  const e20 = id.erc20;
  lines.push(
    `ERC-20 heuristic: ${e20.verdict} (ERC-20 has no interface id, so this is never proof; selectors in code: ${e20.selectorsPresent.length}/6${e20.selectorsMissing.length ? `, missing ${e20.selectorsMissing.join(", ")}` : ""}${e20.decimals != null ? `, decimals ${e20.decimals}` : ""}${e20.totalSupply != null ? `, totalSupply ${e20.totalSupply}` : ""}).`,
  );
  lines.push(
    `Known function selectors in the code: ${id.knownSelectors.length}${id.unknownPush4Count ? `; ${id.unknownPush4Count} other 4-byte constants match no known signature (not all are selectors)` : ""}.`,
  );
  lines.push(id.note);
  const untrusted: Record<string, unknown> = {};
  if (e20.symbol != null) untrusted.symbol = e20.symbol;
  if (e20.name != null) untrusted.name = e20.name;
  if (id.knownSelectors.length) untrusted.knownSelectors = id.knownSelectors.map((s) => `${s.selector} ${s.signatures[0]}${s.signatures.length > 1 ? ` (+${s.signatures.length - 1} more)` : ""}`);
  return lines.join("\n") + (Object.keys(untrusted).length ? "\n" + fenceUntrusted("values read from the contract and selectors found in its code", untrusted) : "");
}

// ---- decoding ----

export interface DecodedCall {
  ok: true;
  source: string;
  signature: string;
  functionName: string;
  args: { name: string; type: string; value: unknown }[];
  alternatives: string[];
}
export interface DecodedEvent {
  ok: true;
  source: string;
  signature: string;
  eventName: string;
  args: { name: string; type: string; indexed: boolean; value: unknown }[];
  alternatives: string[];
}
export type DecodeFailure = { ok: false; reason: string };

function abiFunctions(abi: unknown[]): AbiFunction[] {
  return abi.filter((i): i is AbiFunction => typeof i === "object" && i !== null && (i as { type?: string }).type === "function");
}
function abiEvents(abi: unknown[]): AbiEvent[] {
  return abi.filter((i): i is AbiEvent => typeof i === "object" && i !== null && (i as { type?: string }).type === "event");
}
function sigOf(item: AbiFunction | AbiEvent): string {
  return `${item.name}(${item.inputs.map((p) => canonicalType(p)).join(",")})`;
}
function canonicalType(p: { type: string; components?: readonly { type: string; components?: unknown }[] }): string {
  if (p.type.startsWith("tuple")) {
    const comps = (p.components ?? []) as { type: string; components?: readonly { type: string }[] }[];
    return `(${comps.map((c) => canonicalType(c)).join(",")})${p.type.slice(5)}`;
  }
  return p.type;
}

function callFromAbi(abi: unknown[], data: Hex, source: string): DecodedCall | null {
  const fns = abiFunctions(abi);
  if (fns.length === 0) return null;
  try {
    const out = decodeFunctionData({ abi: fns as Abi, data });
    const fn = fns.find((f) => f.name === out.functionName && (out.args?.length ?? 0) === f.inputs.length) ?? fns.find((f) => f.name === out.functionName);
    if (!fn) return null;
    const values = (out.args ?? []) as unknown[];
    return {
      ok: true,
      source,
      signature: sigOf(fn),
      functionName: fn.name,
      args: fn.inputs.map((p, i) => ({ name: p.name || `arg${i}`, type: canonicalType(p), value: json(values[i]) })),
      alternatives: [],
    };
  } catch {
    return null;
  }
}

function eventFromAbi(abi: unknown[], topics: Hex[], data: Hex, source: string): DecodedEvent | null {
  const evs = abiEvents(abi);
  for (const ev of evs) {
    try {
      const out = decodeEventLog({ abi: [ev] as Abi, topics: topics as [Hex, ...Hex[]], data, strict: true });
      const values = out.args as unknown;
      return {
        ok: true,
        source,
        signature: sigOf(ev),
        eventName: ev.name,
        args: ev.inputs.map((p, i) => ({
          name: p.name || `arg${i}`,
          type: canonicalType(p),
          indexed: !!p.indexed,
          value: json(Array.isArray(values) ? values[i] : (values as Record<string, unknown> | undefined)?.[p.name ?? ""]),
        })),
        alternatives: [],
      };
    } catch {
      // not this event (wrong topic or indexed layout); try the next
    }
  }
  return null;
}

const SOURCE_WORDS: Record<AbiResolutionView["source"], string> = {
  verified: "CitrateScan verified ABI",
  book: "bundled system ABI (address book)",
  "partial-match": "CitrateScan partial-match ABI (not verified)",
  none: "",
};

async function resolved(deps: ContractIntelDeps, address: string | null): Promise<{ abi: unknown[] | null; source: string; note: string }> {
  if (!address) return { abi: null, source: "", note: "" };
  try {
    const r = await deps.abiResolve(address);
    return { abi: r.source === "none" ? null : r.abi, source: `${SOURCE_WORDS[r.source]} of ${r.contractName ?? r.abiAddress ?? address}`, note: r.note };
  } catch (e) {
    return { abi: null, source: "", note: `the ABI of ${address} could not be resolved (${errText(e)})` };
  }
}

/** Decode `data` with the resolved ABI, then the signature subset. */
export async function decodeCall(deps: ContractIntelDeps, data: Hex, address: string | null): Promise<DecodedCall | DecodeFailure> {
  if (data.length < 10) return { ok: false, reason: "calldata shorter than a 4-byte selector" };
  const selector = data.slice(0, 10).toLowerCase();
  const r = await resolved(deps, address);
  if (r.abi) {
    const hit = callFromAbi(r.abi, data, r.source);
    if (hit) return hit;
  }
  let frags: string[] = [];
  try {
    frags = (await deps.signatureLookup([selector], [])).functions[selector] ?? [];
  } catch (e) {
    return { ok: false, reason: `the signature subset could not be read (${errText(e)})` };
  }
  const hits: DecodedCall[] = [];
  for (const f of frags) {
    try {
      const item = parseAbiItem(f) as AbiFunction;
      const hit = callFromAbi([item], data, "bundled signature subset (selector match; names from a known ABI, not from this contract)");
      if (hit) hits.push(hit);
    } catch {
      // a fragment that does not parse or decode is skipped
    }
  }
  if (hits.length === 0) {
    const why = r.abi ? `the selector ${selector} is not in the ${r.source}` : r.note || "no ABI";
    return { ok: false, reason: `unknown selector ${selector}: ${why}, and ${frags.length ? "no known signature decodes these bytes" : "it is not in the bundled signature subset"}` };
  }
  return { ...hits[0], alternatives: hits.slice(1).map((h) => h.signature) };
}

/** Decode one log with the emitter's resolved ABI, then the signature subset. */
export async function decodeLogEntry(
  deps: ContractIntelDeps,
  log: { address: string | null; topics: Hex[]; data: Hex },
  abiCache?: Map<string, Promise<{ abi: unknown[] | null; source: string; note: string }>>,
): Promise<DecodedEvent | DecodeFailure> {
  if (log.topics.length === 0) return { ok: false, reason: "an anonymous event (no topic0) cannot be decoded without its ABI" };
  const topic0 = log.topics[0].toLowerCase();
  let r: { abi: unknown[] | null; source: string; note: string } = { abi: null, source: "", note: "" };
  if (log.address) {
    const key = log.address.toLowerCase();
    if (abiCache && !abiCache.has(key) && abiCache.size >= MAX_RESOLVED_EMITTERS) {
      r = { abi: null, source: "", note: "this emitter's ABI was not resolved (too many emitters in one tx)" };
    } else if (abiCache) {
      if (!abiCache.has(key)) abiCache.set(key, resolved(deps, key));
      r = await (abiCache.get(key) as Promise<typeof r>);
    } else {
      r = await resolved(deps, key);
    }
  }
  if (r.abi) {
    const hit = eventFromAbi(r.abi, log.topics, log.data, r.source);
    if (hit) return hit;
  }
  let frags: string[] = [];
  try {
    frags = (await deps.signatureLookup([], [topic0])).events[topic0] ?? [];
  } catch (e) {
    return { ok: false, reason: `the signature subset could not be read (${errText(e)})` };
  }
  const hits: DecodedEvent[] = [];
  for (const f of frags) {
    try {
      const item = parseAbiItem(f) as AbiEvent;
      const hit = eventFromAbi([item], log.topics, log.data, "bundled signature subset (topic match; names from a known ABI, not from this contract)");
      if (hit) hits.push(hit);
    } catch {
      // skipped
    }
  }
  if (hits.length === 0) {
    return { ok: false, reason: `unknown event topic ${topic0}: ${frags.length ? "no known event layout decodes it" : "not in the bundled signature subset"}${r.note && !r.abi ? ` (${r.note})` : ""}` };
  }
  return { ...hits[0], alternatives: hits.slice(1).map((h) => h.signature) };
}

function parseHex(v: unknown): Hex | null {
  const s = str(v);
  return HEX.test(s) ? (s.toLowerCase() as Hex) : null;
}

export async function runDecodeCalldata(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const data = parseHex(args.data);
  if (!data || data.length < 10) return "decode_calldata needs `data`: 0x hex calldata with at least a 4-byte selector. Nothing was decoded.";
  if (data.length - 2 > MAX_CALLDATA_HEX) return "decode_calldata: the calldata is larger than 64 KiB. Nothing was decoded.";
  const rawAddr = str(args.address);
  if (rawAddr !== "" && !isAddress(rawAddr)) return "decode_calldata: `address` must be 0x + 40 hex (or omitted). Nothing was decoded.";
  const out = await decodeCall(deps, data, rawAddr === "" ? null : rawAddr.toLowerCase());
  if (!out.ok) return `decode_calldata: not decoded. ${out.reason}. The meaning of this call is unknown; do not guess it.`;
  const alt = out.alternatives.length ? ` Other signatures with the same selector also decode these bytes: ${out.alternatives.join(", ")}; the match is ambiguous.` : "";
  return `decode_calldata: ${out.signature}, decoded with the ${out.source}.${alt}\n` + fenceUntrusted("decoded call arguments; whoever built the call chose them", out.args);
}

export async function runDecodeLog(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const raw = (typeof args.log === "object" && args.log !== null ? args.log : args) as Record<string, unknown>;
  const topicsRaw = Array.isArray(raw.topics) ? raw.topics : null;
  const topics = topicsRaw?.map((t) => parseHex(t)) ?? null;
  const data = raw.data === undefined || raw.data === null || raw.data === "" ? ("0x" as Hex) : parseHex(raw.data);
  if (!topics || topics.some((t) => !t || t.length !== 66) || topics.length > 4 || !data) {
    return "decode_log needs a log: {address?, topics: up to 4 × 0x + 64 hex, data: 0x hex}. Nothing was decoded.";
  }
  const addr = str(raw.address);
  if (addr !== "" && !isAddress(addr)) return "decode_log: `address` must be 0x + 40 hex (or omitted). Nothing was decoded.";
  const out = await decodeLogEntry(deps, { address: addr === "" ? null : addr, topics: topics as Hex[], data });
  if (!out.ok) return `decode_log: not decoded. ${out.reason}. What this event means is unknown; do not guess it.`;
  const alt = out.alternatives.length ? ` Other event layouts with the same topic also decode it: ${out.alternatives.join(", ")}.` : "";
  return `decode_log: ${out.signature}, decoded with the ${out.source}.${alt}\n` + fenceUntrusted("decoded event values; the contract that emitted the log chose them", out.args);
}

// ---- explain_tx ----

function salt(wei: string | null): string {
  if (wei == null) return "unknown";
  try {
    return `${formatEther(BigInt(wei))} SALT`;
  } catch {
    return `${wei} wei`;
  }
}
function named(addr: string | null, names: Record<string, string[]>): string {
  if (!addr) return "none";
  const n = names[addr.toLowerCase()];
  return n?.length ? `${addr} (${n.join(", ")})` : addr;
}

export async function runExplainTx(deps: ContractIntelDeps, args: Record<string, unknown>): Promise<string> {
  const hash = str(args.hash ?? args.tx_hash);
  if (!TX_HASH.test(hash)) return "explain_tx needs a transaction hash (0x + 64 hex). Nothing was read.";
  let tx: TxFetchView;
  try {
    tx = await deps.txFetch(hash);
  } catch (e) {
    return `explain_tx: ${hash} could not be read from chain 40204 (${errText(e)}). Nothing is known about it.`;
  }
  if (!tx.found) return `explain_tx: chain 40204 has no transaction ${hash} (eth_getTransactionByHash returned nothing). It may be on another network, dropped, or mistyped.`;
  const lines = [
    `explain_tx ${tx.hash} (source: eth_getTransactionByHash and eth_getTransactionReceipt on chain 40204; decoding as noted per item).`,
    `Status: ${tx.status}${tx.blockNumber ? ` in block ${tx.blockNumber}` : ""}${tx.gasUsed ? `, gas used ${tx.gasUsed}` : ""}.`,
    `From ${named(tx.from, tx.names)} to ${tx.to ? named(tx.to, tx.names) : "nobody (a contract creation)"}, value ${salt(tx.valueWei)}${tx.nonce ? `, nonce ${tx.nonce}` : ""}.`,
  ];
  if (tx.contractAddress) lines.push(`Created contract ${named(tx.contractAddress, tx.names)}.`);
  const untrusted: Record<string, unknown> = {};
  const input = (tx.input ?? "0x") as Hex;
  if (!tx.to) {
    lines.push(`Input: ${Math.max(0, (input.length - 2) / 2)} bytes of init code (not decoded).`);
  } else if (input === "0x") {
    lines.push("Input: none (a plain SALT transfer).");
  } else if (tx.inputTruncated) {
    lines.push("Input: larger than 128 KiB; not decoded.");
  } else {
    const call = await decodeCall(deps, input, tx.to);
    if (call.ok) {
      lines.push(`Call: ${call.signature}, decoded with the ${call.source}${call.alternatives.length ? `; ambiguous with ${call.alternatives.join(", ")}` : ""}.`);
      untrusted.call = { signature: call.signature, args: call.args };
    } else {
      lines.push(`Call: not decoded (${call.reason}). Its meaning is unknown.`);
    }
  }
  if (tx.status === "pending") {
    lines.push("No receipt yet, so there are no logs to read.");
  } else {
    const cache = new Map<string, Promise<{ abi: unknown[] | null; source: string; note: string }>>();
    const events: unknown[] = [];
    let undecoded = 0;
    for (const log of tx.logs.slice(0, MAX_EXPLAINED_LOGS)) {
      const ev = await decodeLogEntry(deps, { address: log.address, topics: log.topics as Hex[], data: (log.data || "0x") as Hex }, cache);
      if (ev.ok) {
        events.push({ emitter: named(log.address, tx.names), event: ev.signature, decodedWith: ev.source, args: ev.args });
      } else {
        undecoded += 1;
        events.push({ emitter: named(log.address, tx.names), undecoded: ev.reason });
      }
    }
    const more = tx.logs.length > MAX_EXPLAINED_LOGS || tx.logsTruncated ? ` (only the first ${Math.min(tx.logs.length, MAX_EXPLAINED_LOGS)} are shown)` : "";
    lines.push(`Logs: ${tx.logs.length}${more}; ${tx.logs.length ? `${Math.min(tx.logs.length, MAX_EXPLAINED_LOGS) - undecoded} decoded, ${undecoded} not decoded` : "none emitted"}.`);
    if (events.length) untrusted.events = events;
  }
  return lines.join("\n") + (Object.keys(untrusted).length ? "\n" + fenceUntrusted("decoded call and events; whoever sent the tx and the contracts it touched chose these values", untrusted) : "");
}
