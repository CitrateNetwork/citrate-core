import { custom, defineChain, http, type Transport } from "viem";
import { ACTIVE_PROFILE, isReady, profileProblems } from "./network/profile";

/**
 * The chain this build targets, from the active network profile
 * (src-tauri/network/profiles.json, K6). Live RPC, no mocked data (Rule 1).
 *
 * When the active profile is not ready there is no RPC to read and no chain id to
 * claim: the RPC list is empty and the id is 0, which is never a valid chain id.
 * NetworkCompatGate blocks the app in that state.
 */
const ready = isReady(ACTIVE_PROFILE);

/** True when the active network profile is fully set (Rust `NetworkProfile::ready`). */
export const NETWORK_READY: boolean = ready;

export const citrate = defineChain({
  id: ready && ACTIVE_PROFILE?.chainId ? ACTIVE_PROFILE.chainId : 0,
  name: "Citrate",
  nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
  rpcUrls: {
    default: { http: ready && ACTIVE_PROFILE?.rpcUrl ? [ACTIVE_PROFILE.rpcUrl] : [] },
  },
});

/** The error every read gets while the network is not set. Nothing is sent. */
export class NetworkNotSetError extends Error {
  constructor(method: string) {
    const problems = profileProblems(ACTIVE_PROFILE);
    const why = problems.length > 0 ? ` (${problems.join("; ")})` : "";
    super(`this build has no network set${why}, so ${method} was not sent; update Citrate Core`);
    this.name = "NetworkNotSetError";
  }
}

/**
 * The read transport for the target chain (K6 invariants P1 and P3).
 *
 * - Profile ready: viem `http(url)`, or `http()` (the chain's RPC) when no url is given.
 * - Profile not ready: a transport that rejects every request without sending it. It is built
 *   without throwing, unlike `http()` on a chain with no RPC url (viem throws `UrlRequiredError`
 *   when the transport is created). That throw ran during wagmi config setup and broke the app
 *   import, so the *Update required* notice and the update card never rendered.
 *
 * `ready` is for tests; production uses the active profile.
 */
export function readTransport(url?: string, isNetworkReady: boolean = ready): Transport {
  if (isNetworkReady) return http(url);
  return custom(
    {
      async request({ method }: { method: string }) {
        throw new NetworkNotSetError(method);
      },
    },
    { key: "citrate-network-not-set", name: "Citrate (no network set)", retryCount: 0 },
  );
}
