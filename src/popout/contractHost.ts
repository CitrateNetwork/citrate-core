// =====================================================================
// citrate-core — the main window's side of the Contract reader (HUP-S6.7)
//
// Answers the reader pop-out's requests with the real seams: CitrateScan source and read-only RPC
// through bridge.contracts (Rust), writes through store.proposeContractCall (the SignatureCeremony
// review in this window), explanations through store.explainContract (a tool-less Hermes turn).
// Desktop app only; the web preview has no windows to open.
// =====================================================================
import { bridge } from "../bridge";
import { store } from "../shell/store";
import { parseAbi } from "../contractReader/abi";
import { explainPrompt, type ExplainInput } from "../contractReader/explain";
import { createContractHost, type ContractInbox, type ContractOps } from "./contractChannel";
import type { BridgeTransport } from "./bridge";

/**
 * Build the explanation prompt here, in the main window, from the function's ABI entry (checked by
 * the same strict parser the reader uses) and CitrateScan's source as this window reads it. The
 * reader only names the function; nothing it sends becomes prompt text unchecked.
 */
export async function explainFromReader(address: string, target: string, fnEntry: Record<string, unknown>): Promise<string> {
  const parsed = parseAbi([fnEntry]);
  if (!parsed.ok || parsed.functions.length !== 1) throw new Error("the function to explain could not be read");
  const fn = parsed.functions[0];
  let verified: ExplainInput["verified"] = "pasted";
  let source: string | null = null;
  let contractName: string | null = null;
  if (target === "citrate") {
    const s = await bridge.contracts.source(address).catch(() => null);
    if (s && (s.status === "verified" || s.status === "partial") && s.abi) {
      const all = parseAbi(s.abi);
      if (all.ok && all.functions.some((g) => g.signature === fn.signature)) {
        verified = s.status;
        source = s.source;
        contractName = s.contractName;
      }
    }
  }
  return explainPrompt({ address, contractName, verified, fn, source });
}

let pendingFocus: { address: string; target: string } | null = null;
let hostPromise: Promise<{ focus(address: string, target?: string): Promise<void>; close(): void }> | null = null;

/** The reader's operations, wired to the real app. */
export function readerOps(): ContractOps {
  return {
    initial: async () => {
      const f = pendingFocus;
      pendingFocus = null;
      return f;
    },
    source: ({ address }) => bridge.contracts.source(address),
    codeSize: ({ target, address }) => bridge.contracts.codeSize(target, address),
    view: ({ target, address, calldata }) => bridge.contracts.viewCall(target, address, calldata),
    write: (a) => store.proposeContractCall(a),
    explain: async ({ address, target, fn }) => store.explainContract(await explainFromReader(address, target, fn)),
  };
}

/** Start answering the reader (once). Its requests come from Rust's queue (`inbox`). */
export function startContractHost(transport: () => Promise<BridgeTransport>, inbox: () => Promise<ContractInbox>): void {
  hostPromise ??= Promise.all([transport(), inbox()]).then(([t, i]) => createContractHost(t, readerOps(), i));
  hostPromise.catch(() => {
    hostPromise = null;
  });
}

/** Remember what the reader should open; tell an already-open reader right away. */
export async function focusContractReader(address: string, target = "citrate"): Promise<void> {
  pendingFocus = { address, target };
  const host = hostPromise ? await hostPromise.catch(() => null) : null;
  await host?.focus(address, target).catch(() => undefined);
}
