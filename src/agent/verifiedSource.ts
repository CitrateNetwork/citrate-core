// =====================================================================
// HUP-S4.3 — get_verified_source: the pure formatter behind the agent tool.
//
// Core's `contract_verified_source` command reads CitrateScan's verified-source lookup
// (src-tauri/src/verified_source.rs). Everything the explorer returns about a contract's code
// (source, ABI, contract name, compiler string, its note) was written by the contract's deployer
// or by a remote server, so it reaches the model only inside an untrusted-data fence. The plain
// header carries core-authored words chosen from the status alone.
// =====================================================================
import { fenceUntrusted } from "./untrusted";

export type VerifiedSourceStatus = "verified" | "partial-match" | "unverified" | "unavailable";

/** Mirrors the Rust `VerifiedSource` (serde camelCase). */
export interface VerifiedSourceView {
  address: string;
  status: VerifiedSourceStatus;
  verified: boolean;
  matchType: string | null;
  contractName: string | null;
  compilerVersion: string | null;
  sourceHash: string | null;
  source: string | null;
  sourceTruncated: boolean;
  abi: unknown[] | null;
  verifiedAt: string | null;
  note: string;
}

export function isAddress(v: unknown): v is string {
  return typeof v === "string" && /^0x[0-9a-fA-F]{40}$/.test(v);
}

const HEADER: Record<VerifiedSourceStatus, string> = {
  verified:
    "status: verified (full match). The recompiled source matches the deployed bytecode exactly, metadata included.",
  "partial-match":
    "status: partial match. The code matches only after stripping metadata, so the shown source may differ from what was deployed. This is not a verified match; say so when you describe it.",
  unverified:
    "status: not verified. CitrateScan has no verified source for this address. Do not guess what its code does; you may read its state with view calls, and the member can submit source on CitrateScan.",
  unavailable:
    "status: unavailable. CitrateScan could not read its verification records, so it is unknown whether this contract is verified. Say that plainly.",
};

/** The text handed back to the model for one lookup. */
export function formatVerifiedSourceForAgent(v: VerifiedSourceView): string {
  const header = `CitrateScan verified-source lookup for ${v.address}: ${HEADER[v.status] ?? HEADER.unavailable}`;
  const hasCode = (v.status === "verified" || v.status === "partial-match") && (v.source !== null || v.abi !== null);
  if (!hasCode) return header;
  const trunc = v.sourceTruncated ? " The source was truncated; ask for a specific function if you need more." : "";
  return (
    header +
    trunc +
    "\n" +
    fenceUntrusted("verified contract source and ABI from CitrateScan, written by the contract's deployer", {
      contractName: v.contractName,
      compilerVersion: v.compilerVersion,
      matchType: v.matchType,
      verifiedAt: v.verifiedAt,
      sourceTruncated: v.sourceTruncated,
      abi: v.abi,
      source: v.source,
    })
  );
}
