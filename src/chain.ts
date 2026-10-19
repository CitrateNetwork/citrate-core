import { defineChain } from "viem";
import { ACTIVE_PROFILE, isReady } from "./network/profile";

/**
 * The chain this build targets, from the active network profile
 * (src-tauri/network/profiles.json, K6). Live RPC, no mocked data (Rule 1).
 *
 * When the active profile is not set there is no RPC to read and no chain id to
 * claim: the RPC list is empty (every read fails honestly) and the id is 0, which
 * is never a valid chain id. NetworkCompatGate blocks the app in that state.
 */
const ready = isReady(ACTIVE_PROFILE);

export const citrate = defineChain({
  id: ready && ACTIVE_PROFILE?.chainId ? ACTIVE_PROFILE.chainId : 0,
  name: "Citrate",
  nativeCurrency: { name: "SALT", symbol: "SALT", decimals: 18 },
  rpcUrls: {
    default: { http: ready && ACTIVE_PROFILE?.rpcUrl ? [ACTIVE_PROFILE.rpcUrl] : [] },
  },
});
