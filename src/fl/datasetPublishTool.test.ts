// 0.5.4 WP-H6: publishing a dataset. Core's refusals stop it before any card; the card says a
// CID cannot be recalled; IPFS publish is core's; the DatasetRegistry record waits on the address
// book ("Not available yet on this network" on r1007) and, where a book pins it, opens the exact
// publish call in the wallet review. Hermes never signs.
import { describe, expect, it, vi } from "vitest";
import { decodeFunctionData, encodeFunctionResult, keccak256, toBytes, toFunctionSelector, type Hex } from "viem";
import realBook from "../../src-tauri/addresses/40204.json";
import { runPublishTool, proposeRegistryRecord, type PublishToolDeps } from "./datasetPublishTool";
import { DATASET_REGISTRY_ABI, PUBLISH_SIGNATURE, datasetIdFor, publishCalldata, registryState } from "./datasetRegistry";
import { NOT_AVAILABLE } from "./networkState";
import { DATASET_ID, HASH_B, ZERO, dataset, datasetList, publication, publishCheck } from "./fixtures/datasets";
import type { ApprovalCard } from "../agent/approvalCards";
import type { CerSpec } from "../shell/state";

const REGISTRY = "0x" + "77".repeat(20);
const PUBLISHER = "0x" + "ab".repeat(20);

function deps(over: Partial<PublishToolDeps> = {}, dsOver: Partial<PublishToolDeps["ds"]> = {}, answer = "approved") {
  const asked: { spec: CerSpec; card: ApprovalCard }[] = [];
  const ds = {
    list: vi.fn(async () => datasetList()),
    published: vi.fn(async () => []),
    publishCheck: vi.fn(async () => publishCheck()),
    publish: vi.fn(async () => ({ version: { ...dataset().versions[0], version: 2 }, name: "Orchard notes", publication: publication(), bundleBytes: 4096 })),
    ...dsOver,
  };
  const d: PublishToolDeps = {
    ds,
    ask: vi.fn(async (spec: CerSpec, card: ApprovalCard) => {
      asked.push({ spec, card });
      return answer;
    }),
    book: { addresses: {} },
    reads: { codeSize: vi.fn(async () => 0), viewCall: vi.fn(async () => "0x") },
    publisher: () => PUBLISHER,
    propose: vi.fn(async () => ({ proposed: true })),
    ...over,
  };
  return { d, ds, asked };
}

describe("the shipped network has no DatasetRegistry", () => {
  it("the generated book does not pin it, so the record waits (Today, 0.5.4)", async () => {
    expect((realBook.addresses as Record<string, string>).DatasetRegistry).toBeUndefined();
    expect(await registryState(realBook, { codeSize: async () => 1, viewCall: async () => "0x" })).toEqual({ kind: "not-deployed", reason: NOT_AVAILABLE });
  });
});

describe("fl_dataset_publish_propose (HIC-1, irreversible)", () => {
  it("stops at core's refusals with no card", async () => {
    const { d, ds } = deps({}, { publishCheck: vi.fn(async () => publishCheck({ refusals: ["License unknown for 1 source", "No PII scan of these rows yet; the on-device scan must pass first"] })) });
    const out = await runPublishTool({ dataset: DATASET_ID, license: "MIT" }, null, d);
    expect(out.result).toMatch(/Core refuses publishing/);
    expect(out.result).toContain("- License unknown for 1 source");
    expect(out.result).toMatch(/you cannot override them/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(ds.publish).not.toHaveBeenCalled();
  });

  it("asks on a card that says it cannot be undone, publishes to IPFS, and says the record waits", async () => {
    const { d, ds, asked } = deps();
    const out = await runPublishTool({ dataset: DATASET_ID, license: "MIT" }, null, d);
    expect(ds.publishCheck).toHaveBeenCalledWith(DATASET_ID, 1, "MIT", "public");
    const card = asked[0].card;
    expect(card.kind === "fields" && card.rows.find((r) => r.k === "cannot be undone")?.v).toMatch(/cannot be recalled/);
    expect(asked[0].spec.title).toMatch(/cannot be undone/);
    expect(ds.publish).toHaveBeenCalledWith(DATASET_ID, 1, "MIT", "public", HASH_B);
    expect(out.status).toBe("approved");
    expect(out.result).toContain("CID bafkreiexamplecid");
    expect(out.result).toContain(`The DatasetRegistry record waits: ${NOT_AVAILABLE}`);
    expect(d.propose).not.toHaveBeenCalled();
  });

  it("publishes nothing when the member declines", async () => {
    const { d, ds } = deps({}, {}, "declined");
    const out = await runPublishTool({ dataset: DATASET_ID, license: "MIT" }, null, d);
    expect(out.result).toMatch(/nothing left this device/);
    expect(ds.publish).not.toHaveBeenCalled();
  });

  it("does not publish the same version twice; only its record can still be proposed", async () => {
    const { d, ds } = deps({}, { list: vi.fn(async () => datasetList([dataset({ versions: [dataset().versions[0], { ...dataset().versions[0], version: 2 }] })])), published: vi.fn(async () => [publication()]) });
    const out = await runPublishTool({ dataset: DATASET_ID, license: "MIT" }, null, d);
    expect(out.result).toMatch(/already on IPFS at bafkreiexamplecid/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(ds.publishCheck).not.toHaveBeenCalled();
  });

  it("checks the id first", async () => {
    const { d, ds } = deps();
    expect((await runPublishTool({ dataset: "x", license: "MIT" }, null, d)).result).toMatch(/needs the dataset id/);
    expect(ds.list).not.toHaveBeenCalled();
  });
});

describe("the DatasetRegistry record where a book pins it", () => {
  const pinned = { addresses: { DatasetRegistry: REGISTRY } };

  it("opens the exact publish call in the wallet review, with the registry's prev", async () => {
    const countZero = encodeFunctionResult({ abi: DATASET_REGISTRY_ABI, functionName: "versionCount", result: 0 });
    const { d } = deps({ book: pinned, reads: { codeSize: vi.fn(async () => 100), viewCall: vi.fn(async () => countZero) } });
    const msg = await proposeRegistryRecord("Orchard notes", publication(), d);
    expect(msg).toMatch(/waiting in the wallet review/);
    const input = (d.propose as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(input.address).toBe(REGISTRY);
    expect(input.valueWei).toBe("0");
    expect(input.label).toBe(PUBLISH_SIGNATURE);
    expect(input.calldata.slice(0, 10)).toBe(toFunctionSelector(PUBLISH_SIGNATURE));
    const dec = decodeFunctionData({ abi: DATASET_REGISTRY_ABI, data: input.calldata as Hex });
    const [id, v] = dec.args as readonly [Hex, { publisher: string; manifestHash: string; prev: string; cid: string; license: string; privacy: number; publishedAt: bigint; withdrawn: boolean }];
    expect(id).toBe(datasetIdFor(PUBLISHER, "Orchard notes"));
    expect(v.publisher).toBe("0x0000000000000000000000000000000000000000");
    expect(v.manifestHash).toBe(publication().manifestHash);
    expect(v.prev).toBe(ZERO);
    expect(v.cid).toBe("bafkreiexamplecid");
    expect(v.license).toBe("MIT");
    expect(v.privacy).toBe(0);
    expect(v.publishedAt).toBe(0n);
    expect(v.withdrawn).toBe(false);
  });

  it("refuses without a wallet address, and on an unknown chain read", async () => {
    const { d } = deps({ book: pinned, reads: { codeSize: vi.fn(async () => 100), viewCall: vi.fn(async () => "0x") }, publisher: () => "" });
    expect(await proposeRegistryRecord("n", publication(), d)).toMatch(/wallet address has not been read/);
    const down = deps({ book: pinned, reads: { codeSize: vi.fn(async () => Promise.reject(new Error("rpc down"))), viewCall: vi.fn(async () => "0x") } });
    expect(await proposeRegistryRecord("n", publication(), down.d)).toMatch(/Could not read whether this network has DatasetRegistry \(rpc down\)/);
    expect(down.d.propose).not.toHaveBeenCalled();
  });

  it("datasetIdFor matches the contract: publisher in the top 20 bytes, keccak(name) low 96 bits", () => {
    const id = datasetIdFor(PUBLISHER, "Orchard notes");
    expect(id.slice(0, 42)).toBe(PUBLISHER);
    expect(id.slice(42)).toBe(keccak256(toBytes("Orchard notes")).slice(-24));
    expect(() => datasetIdFor("nope", "n")).toThrow();
    expect(() => publishCalldata(id, publication({ privacy: "group" }), ZERO as Hex)).toThrow(/Public datasets only/);
  });
});
