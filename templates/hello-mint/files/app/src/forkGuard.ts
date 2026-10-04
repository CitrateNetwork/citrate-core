// The fork guard. A local anvil fork keeps chain id 40204, so a wallet cannot tell the fork and
// chain 40204 apart by chain id. In fork mode the page refuses to ask the wallet to sign a mint
// until it has confirmed that the wallet's own RPC is this fork, not chain 40204 itself: the
// wallet's latest block must be one the fork mined after it forked, with the same hash on the
// fork. Anything else (the live chain, another fork, an RPC that cannot be read) is a refusal.

/** One JSON-RPC call: method and params in, the result out (throws on an RPC error). */
export type Rpc = (method: string, params?: unknown[]) => Promise<unknown>;

export type GuardResult = { ok: true } | { ok: false; reason: string };

const LIVE =
  "Your wallet's Citrate network points at chain 40204 itself, not at this fork, so the mint would be sent to the live chain. Point the wallet's Citrate network at the fork RPC, then try again.";

function blockOf(v: unknown): { number: bigint; hash: string } | null {
  if (typeof v !== "object" || v === null) return null;
  const b = v as { number?: unknown; hash?: unknown };
  if (typeof b.number !== "string" || typeof b.hash !== "string") return null;
  try {
    return { number: BigInt(b.number), hash: b.hash.toLowerCase() };
  } catch {
    return null;
  }
}

function forkBlockNumber(info: unknown): bigint | null {
  if (typeof info !== "object" || info === null) return null;
  const n = (info as { forkConfig?: { forkBlockNumber?: unknown } }).forkConfig?.forkBlockNumber;
  if (typeof n === "number" && Number.isSafeInteger(n) && n >= 0) return BigInt(n);
  if (typeof n === "string") {
    try {
      return BigInt(n);
    } catch {
      return null;
    }
  }
  return null;
}

/** Whether the wallet's RPC is this fork. `wallet` goes through the wallet; `fork` is the page's
 *  own fork RPC. */
export async function walletIsOnFork(wallet: Rpc, fork: Rpc): Promise<GuardResult> {
  let info: unknown;
  try {
    info = await fork("anvil_nodeInfo");
  } catch {
    return { ok: false, reason: "The fork RPC did not say which block it forked from, so the page cannot confirm where the mint would go." };
  }
  const forkedAt = forkBlockNumber(info);
  if (forkedAt === null) {
    return { ok: false, reason: "The fork RPC is not an anvil fork, so the page cannot confirm where the mint would go." };
  }
  let head: { number: bigint; hash: string } | null;
  try {
    head = blockOf(await wallet("eth_getBlockByNumber", ["latest", false]));
  } catch {
    return { ok: false, reason: "The wallet did not answer a block read, so the page cannot confirm where the mint would go." };
  }
  if (!head) return { ok: false, reason: "The wallet returned no latest block, so the page cannot confirm where the mint would go." };
  // At or below the fork point the fork and chain 40204 share every block: not proof of the fork.
  if (head.number <= forkedAt) {
    return { ok: false, reason: LIVE };
  }
  let same: { number: bigint; hash: string } | null;
  try {
    same = blockOf(await fork("eth_getBlockByNumber", ["0x" + head.number.toString(16), false]));
  } catch {
    return { ok: false, reason: "The fork RPC did not answer a block read." };
  }
  if (!same || same.hash !== head.hash) return { ok: false, reason: LIVE };
  return { ok: true };
}

/** A JSON-RPC client for the page's fork URL (loopback only, checked by the config). */
export function httpRpc(url: string, fetcher: typeof fetch = fetch): Rpc {
  let id = 0;
  return async (method, params = []) => {
    id += 1;
    const res = await fetcher(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
    });
    const body = (await res.json()) as { result?: unknown; error?: { message?: string } };
    if (body.error) throw new Error(body.error.message ?? "RPC error");
    return body.result;
  };
}
