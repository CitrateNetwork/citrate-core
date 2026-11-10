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
// (src/network/profile.ts), the same file the Rust network gate reads, and only
// when the WHOLE profile is ready by the Rust `ready()` rule (gateGenesis). Any
// unset or malformed value (not only the genesis: a missing addressBook, RPC,
// bootnodes ...) makes the status "unset": Rust refuses node starts and every
// signature then, chain.ts has no RPC, and the gate blocks rather than waiting
// on a block-0 read that can never answer.
// =====================================================================
import { useBlock } from "wagmi";
import { citrate } from "../chain";
import { ACTIVE_PROFILE, gateGenesis, type NetworkProfile } from "../network/profile";

export type NetworkCompat = "unknown" | "compatible" | "retired" | "unset";

/** The genesis this build targets (lowercase 0x + 64 hex), or null when the active profile is
 *  not fully ready. */
export const EXPECTED_GENESIS: string | null = gateGenesis(ACTIVE_PROFILE);

const HASH = /^0x[0-9a-f]{64}$/;

/** Pure decision: expected vs the live chain's block-0 hash. */
export function networkCompat(expected: string | null, live: string | null | undefined): NetworkCompat {
  if (expected === null) return "unset";
  const want = (expected || "").toLowerCase();
  const got = (live || "").toLowerCase();
  if (!HASH.test(want) || !HASH.test(got)) return "unknown";
  return want === got ? "compatible" : "retired";
}

/** Live block-0 hash of the target chain, checked against this build's genesis. The block-0
 *  read is made only for a fully ready profile. */
export function useNetworkCompat(profile: NetworkProfile | null = ACTIVE_PROFILE): {
  status: NetworkCompat;
  live: string | null;
  expected: string | null;
} {
  const expected = gateGenesis(profile);
  const { data } = useBlock({
    blockNumber: 0n,
    chainId: citrate.id,
    query: { staleTime: Infinity, enabled: expected !== null },
  });
  const live = expected === null ? null : (data?.hash ?? null);
  return { status: networkCompat(expected, live), live, expected };
}

/**
 * The network's display name for a compat status (UI truth contract §2 "Network", §6): the chain
 * id alone is not enough, so "Citrate betanet" only once block 0 matches this build's genesis.
 */
export function networkLabel(status: NetworkCompat): "Citrate betanet" | "Network not checked yet" | "Update required" {
  if (status === "compatible") return "Citrate betanet";
  if (status === "retired") return "Update required";
  return "Network not checked yet";
}

/** The live network label (genesis-checked; "Network not checked yet" until block 0 reads). */
export function useNetworkLabel(): string {
  return networkLabel(useNetworkCompat().status);
}
