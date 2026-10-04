// HUP-S6.2 / US-6.1 — the hello-mint page's fork guard (templates/hello-mint/files/app/src/
// forkGuard.ts). A local fork keeps chain id 40204, so in fork mode the page must refuse to ask the
// wallet to sign a mint unless the wallet's own RPC is this fork. The template file has no
// placeholders, so it is imported directly.
import { describe, expect, it, vi } from "vitest";
import { httpRpc, walletIsOnFork, type Rpc } from "../../templates/hello-mint/files/app/src/forkGuard";

const FORKED_AT = 61_978;
const h = (n: number) => "0x" + n.toString(16);
const hash = (tag: string, n: number) => "0x" + tag.repeat(2) + n.toString(16).padStart(62, "0");

/** A chain: blocks up to `head`, hashes from `tag` above the fork point and shared below it. */
function chain(head: number, tag: string, opts: { anvil?: boolean } = {}): Rpc {
  return async (method, params) => {
    if (method === "anvil_nodeInfo") {
      if (!opts.anvil) throw new Error("method not found");
      return { forkConfig: { forkUrl: "https://rpc.citrate.ai", forkBlockNumber: FORKED_AT } };
    }
    if (method === "eth_getBlockByNumber") {
      const p = (params ?? [])[0] as string;
      const n = p === "latest" ? head : Number.parseInt(p, 16);
      if (n > head) return null;
      return { number: h(n), hash: n <= FORKED_AT ? hash("aa", n) : hash(tag, n) };
    }
    throw new Error("unexpected " + method);
  };
}

describe("hello-mint fork guard", () => {
  it("accepts a wallet whose RPC is this fork (a block the fork mined, same hash)", async () => {
    const fork = chain(FORKED_AT + 3, "f0", { anvil: true });
    expect(await walletIsOnFork(fork, fork)).toEqual({ ok: true });
  });

  it("refuses a wallet on chain 40204 itself: ahead of the fork with other hashes", async () => {
    const live = chain(FORKED_AT + 500, "1e");
    const fork = chain(FORKED_AT + 3, "f0", { anvil: true });
    const r = await walletIsOnFork(live, fork);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/chain 40204 itself/);
  });

  it("refuses when the live head is a height the fork also mined (same height, different hash)", async () => {
    const live = chain(FORKED_AT + 2, "1e");
    const fork = chain(FORKED_AT + 3, "f0", { anvil: true });
    expect((await walletIsOnFork(live, fork)).ok).toBe(false);
  });

  it("refuses at or below the fork point, where both chains share every block", async () => {
    const atFork = chain(FORKED_AT, "1e");
    const fork = chain(FORKED_AT, "f0", { anvil: true });
    expect((await walletIsOnFork(atFork, fork)).ok).toBe(false);
  });

  it("refuses when the fork RPC is not an anvil fork or a read fails", async () => {
    const notAnvil = chain(FORKED_AT + 3, "f0");
    expect((await walletIsOnFork(notAnvil, notAnvil)).ok).toBe(false);
    const noFork: Rpc = async (m) => (m === "anvil_nodeInfo" ? { forkConfig: {} } : null);
    expect((await walletIsOnFork(noFork, noFork)).ok).toBe(false);
    const fork = chain(FORKED_AT + 3, "f0", { anvil: true });
    const broken: Rpc = async () => { throw new Error("user rejected"); };
    const r = await walletIsOnFork(broken, fork);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toMatch(/wallet did not answer/);
    const empty: Rpc = async () => null;
    expect((await walletIsOnFork(empty, fork)).ok).toBe(false);
  });

  it("httpRpc posts JSON-RPC to the fork URL and surfaces RPC errors", async () => {
    const fetcher = vi.fn(async (_url: string, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body));
      const payload = body.method === "boom" ? { jsonrpc: "2.0", id: body.id, error: { message: "nope" } } : { jsonrpc: "2.0", id: body.id, result: "0x1" };
      return new Response(JSON.stringify(payload));
    });
    const rpc = httpRpc("http://127.0.0.1:8545", fetcher as unknown as typeof fetch);
    expect(await rpc("eth_chainId")).toBe("0x1");
    await expect(rpc("boom")).rejects.toThrow("nope");
    expect(fetcher.mock.calls[0][0]).toBe("http://127.0.0.1:8545");
  });
});
