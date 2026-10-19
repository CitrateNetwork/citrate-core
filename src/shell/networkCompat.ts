// =====================================================================
// citrate-core — network compatibility gate
//
// A reroll keeps chain id 40204 but starts a new chain (new genesis, new
// contract addresses). A build made for the old chain would keep talking to
// rpc.citrate.ai with dead addresses. This compares block 0 of the live chain
// (eth_getBlockByNumber("0x0") over the same RPC the app reads) with the
// genesis hash of the build's active network profile (which the address book
// src-tauri/addresses/40204.json is checked equal to). A mismatch means this build
// targets a retired network and must update before doing anything on-chain.
//
// Honest by construction (Rule 1): "unknown" while the RPC hasn't answered or
// errored. Only a definite, different block-0 hash is "retired".
//
// K6: the expected genesis comes from the active network profile
// (src/network/profile.ts), the same file the Rust network gate reads. When the
// profile has no genesis set the status is "unset": there is nothing to compare
// against, and the gate blocks rather than letting the app read or sign.
// =====================================================================
import { useBlock } from "wagmi";
import { citrate } from "../chain";
import { expectedGenesis } from "../network/profile";

export type NetworkCompat = "unknown" | "compatible" | "retired" | "unset";

/** The genesis this build targets (lowercase 0x + 64 hex), or null when not set. */
export const EXPECTED_GENESIS: string | null = expectedGenesis();

const HASH = /^0x[0-9a-f]{64}$/;

/** Pure decision: expected vs the live chain's block-0 hash. */
export function networkCompat(expected: string | null, live: string | null | undefined): NetworkCompat {
  if (expected === null) return "unset";
  const want = (expected || "").toLowerCase();
  const got = (live || "").toLowerCase();
  if (!HASH.test(want) || !HASH.test(got)) return "unknown";
  return want === got ? "compatible" : "retired";
}

/** Live block-0 hash of the target chain, checked against this build's genesis. */
export function useNetworkCompat(): { status: NetworkCompat; live: string | null } {
  const { data } = useBlock({
    blockNumber: 0n,
    chainId: citrate.id,
    query: { staleTime: Infinity, enabled: EXPECTED_GENESIS !== null },
  });
  const live = data?.hash ?? null;
  return { status: networkCompat(EXPECTED_GENESIS, live), live };
}
