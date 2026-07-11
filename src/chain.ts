import { defineChain } from "viem";

/**
 * Citrate testnet-beta (chain id 40204). Live RPC — no mocked data (Rule 1).
 * Everything in this repo targets 40204 until the launch gates (planset 00).
 */
export const citrate = defineChain({
  id: 40204,
  name: "Citrate",
  nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
  rpcUrls: {
    default: { http: ["https://rpc.citrate.ai"] },
  },
});
