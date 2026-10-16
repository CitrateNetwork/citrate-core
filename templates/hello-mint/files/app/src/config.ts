import { defineChain, getAddress, isAddress, type Address, type Chain } from "viem";

/** Citrate's chain id. The local fork keeps it (anvil forks inherit the chain id). */
export const CITRATE_CHAIN_ID = 40204;

export const citrate = defineChain({
  id: CITRATE_CHAIN_ID,
  name: "Citrate",
  nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
  rpcUrls: { default: { http: ["https://rpc.citrate.ai"] } },
  blockExplorers: { default: { name: "CitrateScan", url: "https://explorer.citrate.ai" } },
  testnet: true,
});

export type Target = "fork" | "citrate";

export interface Deployment {
  target: Target;
  chain: Chain;
  rpcUrl: string;
  /** null until the deploy step has written VITE_CONTRACT_ADDRESS. */
  contract: Address | null;
}

export type ConfigResult = { ok: true; deployment: Deployment } | { ok: false; error: string };

const DEFAULT_FORK_RPC = "http://127.0.0.1:8545";
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

function forkChain(rpcUrl: string): Chain {
  return defineChain({
    id: CITRATE_CHAIN_ID,
    name: "Citrate (local fork)",
    nativeCurrency: citrate.nativeCurrency,
    rpcUrls: { default: { http: [rpcUrl] } },
    testnet: true,
  });
}

/**
 * Read the page configuration. Every value is checked; a bad value is reported
 * on the page instead of being guessed around.
 */
export function readConfig(env: Record<string, string | undefined>): ConfigResult {
  const rawTarget = (env.VITE_TARGET ?? "fork").trim();
  if (rawTarget !== "fork" && rawTarget !== "citrate") {
    return { ok: false, error: `VITE_TARGET must be "fork" or "citrate", not "${rawTarget}".` };
  }
  const target: Target = rawTarget;

  const rawAddress = (env.VITE_CONTRACT_ADDRESS ?? "").trim();
  let contract: Address | null = null;
  if (rawAddress !== "") {
    if (!isAddress(rawAddress)) {
      return { ok: false, error: "VITE_CONTRACT_ADDRESS is not a valid address." };
    }
    contract = getAddress(rawAddress);
  }

  if (target === "citrate") {
    return {
      ok: true,
      deployment: { target, chain: citrate, rpcUrl: citrate.rpcUrls.default.http[0], contract },
    };
  }

  const rpcUrl = (env.VITE_FORK_RPC_URL ?? DEFAULT_FORK_RPC).trim();
  let parsed: URL;
  try {
    parsed = new URL(rpcUrl);
  } catch {
    return { ok: false, error: "VITE_FORK_RPC_URL is not a URL." };
  }
  if (parsed.protocol !== "http:" || !LOOPBACK_HOSTS.has(parsed.hostname)) {
    return { ok: false, error: "VITE_FORK_RPC_URL must be an http:// loopback address (the local fork)." };
  }
  return { ok: true, deployment: { target, chain: forkChain(rpcUrl), rpcUrl, contract } };
}
