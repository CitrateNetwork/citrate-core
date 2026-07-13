import { http, createConfig } from "wagmi";
import { citrate } from "./chain";
import { ceremonyConnector } from "./wallet/ceremonyConnector";

/**
 * wagmi + viem scaffold (D-13).
 *
 * Reads go straight to the live 40204 RPC over the http transport. Signing is
 * routed through the Rust SignatureCeremony via the custom `ceremonyConnector`
 * (B1.3): its internal EIP-1193 provider forwards reads to the transport,
 * returns the wallet ADDRESS (never a key) for eth_accounts, and marshals every
 * signing call (personal_sign / eth_signTypedData_v4 / eth_sendTransaction) to
 * `bridge.signing` → the ceremony → a human approval. Signing NEVER happens in
 * JS. The interactive approval UI is wired host-side via `approvalHook` and the
 * real 40204 broadcast for eth_sendTransaction is deferred to B1.4 (honest
 * error, no fabricated tx hash — Rule 1).
 */
export const wagmiConfig = createConfig({
  chains: [citrate],
  connectors: [ceremonyConnector()],
  transports: {
    [citrate.id]: http(),
  },
});
