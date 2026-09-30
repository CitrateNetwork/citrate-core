// =====================================================================
// citrate-core — network compatibility gate
//
// A reroll keeps chain id 40204 but starts a new chain (new genesis, new
// contract addresses). A build made for the old chain would keep talking to
// rpc.citrate.ai with dead addresses. This compares block 0 of the live chain
// (eth_getBlockByNumber("0x0") over the same RPC the app reads) with the
// genesis hash the build's address book was generated for
// (src-tauri/addresses/40204.json → genesisHash). A mismatch means this build
// targets a retired network and must update before doing anything on-chain.
//
// Honest by construction (Rule 1): "unknown" while the RPC hasn't answered or
// errored. Only a definite, different block-0 hash is "retired".
// =====================================================================
import { useBlock } from "wagmi";
import book from "../../src-tauri/addresses/40204.json";
import { citrate } from "../chain";

export type NetworkCompat = "unknown" | "compatible" | "retired";

/** The genesis this build was generated against (lowercase 0x + 64 hex). */
export const EXPECTED_GENESIS: string = String(book.genesisHash).toLowerCase();

const HASH = /^0x[0-9a-f]{64}$/;

/** Pure decision: expected vs the live chain's block-0 hash. */
export function networkCompat(expected: string, live: string | null | undefined): NetworkCompat {
  const want = (expected || "").toLowerCase();
  const got = (live || "").toLowerCase();
  if (!HASH.test(want) || !HASH.test(got)) return "unknown";
  return want === got ? "compatible" : "retired";
}

/** Live block-0 hash of chain 40204, checked against this build's genesis. */
export function useNetworkCompat(): { status: NetworkCompat; live: string | null } {
  const { data } = useBlock({ blockNumber: 0n, chainId: citrate.id, query: { staleTime: Infinity } });
  const live = data?.hash ?? null;
  return { status: networkCompat(EXPECTED_GENESIS, live), live };
}
