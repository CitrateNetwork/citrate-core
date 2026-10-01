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
import { createContractHost, type ContractOps } from "./contractChannel";
import type { BridgeTransport } from "./bridge";

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
    explain: ({ prompt }) => store.explainContract(prompt),
  };
}

/** Start answering the reader (once). */
export function startContractHost(transport: () => Promise<BridgeTransport>): void {
  hostPromise ??= transport().then((t) => createContractHost(t, readerOps()));
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
