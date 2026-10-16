// HUP-S6.7 end to end: the hello-mint contract deployed to a local anvil is read back with the
// Contract reader's own logic (forge's ABI → parseAbi → encodeCall → eth_call → decodeResult).
// Skipped unless scripts/e2e-postdeploy-reader.sh set CITRATE_E2E_RPC and friends.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { decodeResult, encodeCall, parseAbi, parseArg } from "./abi";

const RPC = process.env.CITRATE_E2E_RPC;
const PROJECT = process.env.CITRATE_E2E_PROJECT;
const ADDRESS = process.env.CITRATE_E2E_ADDRESS;
const CONTRACT = process.env.CITRATE_E2E_CONTRACT ?? "LemonDrops";

async function ethCall(to: string, data: string): Promise<string> {
  const r = await fetch(RPC as string, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_call", params: [{ to, data }, "latest"] }),
  });
  const j = (await r.json()) as { result?: string; error?: { message: string } };
  if (j.error) throw new Error(j.error.message);
  return j.result as string;
}

describe.skipIf(!RPC || !PROJECT || !ADDRESS)("HUP-S6.7 e2e: read hello-mint back from anvil", () => {
  it("reads name, supply cap, price and a balance through the reader logic", async () => {
    const artifact = JSON.parse(readFileSync(join(PROJECT as string, "contracts", "out", "Token.sol", `${CONTRACT}.json`), "utf8")) as { abi: unknown[] };
    const abi = parseAbi(artifact.abi);
    if (!abi.ok) throw new Error(abi.error);
    const fn = (sig: string) => {
      const f = abi.functions.find((x) => x.signature === sig);
      if (!f) throw new Error(`missing ${sig}`);
      return f;
    };
    const read = async (sig: string, args: string[] = []) => {
      const f = fn(sig);
      return decodeResult(f, await ethCall(ADDRESS as string, encodeCall(f, f.inputs.map((p, i) => parseArg(p, args[i])))));
    };
    expect(await read("name()")).toEqual(["Lemon Drops"]);
    expect(await read("symbol()")).toEqual(["LEMON"]);
    expect(await read("MAX_SUPPLY()")).toEqual(["500"]);
    expect(await read("totalMinted()")).toEqual(["0"]);
    expect(await read("balanceOf(address)", ["0x70997970C51812dc3A010C7d01b50e0d17dc79C8"])).toEqual(["0"]);
    expect(fn("mint(uint256)").kind).toBe("write");
    expect(fn("mint(uint256)").payable).toBe(true);
  });
});
