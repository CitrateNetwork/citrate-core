// citrate-core: the chain's own clock for FL windows (dispute windows are in seconds on WorkMarket,
// compared against block.timestamp). Data source: the latest block's timestamp on the network
// profile's RPC (eth_getBlockByNumber "latest"). Unread is null, never the device clock.
import { createPublicClient, http } from "viem";
import { citrate } from "../chain";
import { aboutDuration } from "./format";

let client: ReturnType<typeof createPublicClient> | null = null;

export async function latestBlockTime(): Promise<number> {
  client ??= createPublicClient({ chain: citrate, transport: http() });
  const b = await client.getBlock({ blockTag: "latest" });
  return Number(b.timestamp);
}

/** "ends 2026-11-20 12:00 UTC, in about 3 h 40 min" against the chain's clock. */
export function windowWords(endsAt: number, chainNow: number | null): string {
  const at = new Date(endsAt * 1000).toISOString().slice(0, 16).replace("T", " ") + " UTC";
  if (chainNow === null) return `ends ${at} (the chain's time was not read)`;
  if (chainNow >= endsAt) return `ended ${at}`;
  return `ends ${at}, in ${aboutDuration(endsAt - chainNow)}`;
}
