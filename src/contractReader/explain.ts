// =====================================================================
// citrate-core — Contract reader explanations (HUP-S6.7, US-6.3 AC2)
//
// Two layers. `describeFunction` is what the reader can say for certain from the ABI alone (read
// or write, gas, ceremony, payable) plus a short list of function names that usually move funds
// or control. `explainPrompt` is what Hermes is asked when the member wants more: the function
// and the verified source near it, fenced as untrusted data, with the instruction to explain and
// name risks and to call no tool. The answer is shown as Hermes's explanation, not as fact.
// =====================================================================
import { fenceUntrusted } from "../agent/untrusted";
import type { ReaderFunction } from "./abi";

export interface FunctionDescription {
  summary: string;
  facts: string[];
  cautions: string[];
}

const CONTROL = /^(transferOwnership|renounceOwnership|grantRole|revokeRole|renounceRole|upgradeTo|upgradeToAndCall|setApprovalForAll|approve|setOwner|changeAdmin|setAdmin|pause|unpause)$/;
const FUNDS = /^(withdraw|withdrawAll|transfer|transferFrom|safeTransferFrom|sweep|rescue|drain|burn|burnFrom)/;
const MAX_EXCERPT = 12_000;

export function describeFunction(fn: ReaderFunction): FunctionDescription {
  const facts: string[] = [];
  const cautions: string[] = [];
  if (fn.kind === "read") {
    facts.push("A read: the node answers it directly. It is free and changes nothing on chain.");
  } else {
    facts.push("A write: it changes chain state, so it needs a transaction you approve in the Signature Ceremony.");
    facts.push("It costs gas, paid in SALT from your wallet.");
    if (fn.payable) facts.push("It is payable: the call can also send SALT to the contract.");
  }
  facts.push(fn.inputs.length === 0 ? "It takes no inputs." : `Inputs: ${fn.inputs.map((p) => `${p.name || "(unnamed)"} ${p.type}`).join(", ")}.`);
  if (fn.outputs.length > 0) facts.push(`Returns: ${fn.outputs.map((p) => `${p.name ? p.name + " " : ""}${p.type}`).join(", ")}.`);
  if (CONTROL.test(fn.name)) cautions.push("Functions with this name usually change who controls the contract or who may move tokens.");
  if (FUNDS.test(fn.name) && fn.kind === "write") cautions.push("Functions with this name usually move funds or tokens.");
  const summary = fn.kind === "read" ? `Read ${fn.signature}` : `Write ${fn.signature}${fn.payable ? " (payable)" : ""}`;
  return { summary, facts, cautions };
}

/** The lines of `source` around the first definition of `name`, bounded. */
export function sourceExcerpt(source: string | null, name: string): string | null {
  if (!source) return null;
  if (source.length <= MAX_EXCERPT) return source;
  const lines = source.split("\n");
  const at = lines.findIndex((l) => new RegExp(`\\bfunction\\s+${name}\\s*\\(`).test(l));
  const start = Math.max(0, (at < 0 ? 0 : at) - 40);
  let out = "";
  for (let i = start; i < lines.length && out.length + lines[i].length + 1 <= MAX_EXCERPT; i++) out += lines[i] + "\n";
  return out;
}

export interface ExplainInput {
  address: string;
  contractName: string | null;
  /** Where the ABI came from. */
  verified: "verified" | "partial" | "pasted";
  fn: ReaderFunction;
  source: string | null;
}

export function explainPrompt(x: ExplainInput): string {
  const origin =
    x.verified === "verified"
      ? "The ABI and source below are CitrateScan's verified source for this address."
      : x.verified === "partial"
        ? "The ABI and source below are a partial match on CitrateScan (not verified: the metadata does not match)."
        : "The ABI below was pasted by the member; there is no verified source for this address.";
  const excerpt = sourceExcerpt(x.source, x.fn.name);
  const data = {
    address: x.address,
    contractName: x.contractName,
    function: { signature: x.fn.signature, stateMutability: x.fn.stateMutability, inputs: x.fn.inputs, outputs: x.fn.outputs },
    sourceExcerpt: excerpt ?? "(no verified source)",
  };
  return [
    `Explain the function ${x.fn.signature} of the contract at ${x.address} on Citrate chain 40204 for the member, in plain words.`,
    "Say what it does, who can call it, what it changes, and its risks (funds, control, reentrancy, unbounded loops, missing checks). If the source does not show something, say you cannot tell.",
    "Do not call any tool. Do not propose any transaction. This is an explanation only.",
    origin,
    fenceUntrusted("contract ABI and source", JSON.stringify(data, null, 2)),
  ].join("\n\n");
}
