import { http, createConfig } from "wagmi";
import { citrate } from "./chain";

/**
 * wagmi + viem scaffold (D-13).
 *
 * Reads go straight to the live 40204 RPC over http transport. There are
 * deliberately NO connectors yet: signing must route through the Rust
 * SignatureCeremony via a custom wagmi connector over the internal EIP-1193
 * provider (planset 02 §1b). That connector lands in CORE-S2 — anything that
 * pretended to sign before then would violate Rule 1 and I-2 (custody).
 */
export const wagmiConfig = createConfig({
  chains: [citrate],
  connectors: [],
  transports: {
    [citrate.id]: http(),
  },
});
