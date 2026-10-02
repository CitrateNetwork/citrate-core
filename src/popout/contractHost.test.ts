// HUP-S6.7 — the main window's answers to the Contract reader come from the real seams, and the
// address the main window asked for is handed over once.
import { describe, it, expect, vi, afterEach } from "vitest";
import { bridge } from "../bridge";
import { store } from "../shell/store";
import { focusContractReader, readerOps } from "./contractHost";

const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
afterEach(() => vi.restoreAllMocks());

describe("HUP-S6.7 contract host", () => {
  it("hands the requested address to the reader once", async () => {
    const ops = readerOps();
    await focusContractReader(ADDR, "http://127.0.0.1:8545");
    await expect(ops.initial({})).resolves.toEqual({ address: ADDR, target: "http://127.0.0.1:8545" });
    await expect(ops.initial({})).resolves.toBeNull();
  });

  it("reads go to bridge.contracts; writes and explanations to the store", async () => {
    const view = vi.spyOn(bridge.contracts, "viewCall").mockResolvedValue("0x01");
    const write = vi.spyOn(store, "proposeContractCall").mockResolvedValue({ proposed: true });
    const explain = vi.spyOn(store, "explainContract").mockResolvedValue({ text: "t", by: "m" });
    const ops = readerOps();
    await ops.view({ target: "citrate", address: ADDR, calldata: "0x06fdde03" });
    expect(view).toHaveBeenCalledWith("citrate", ADDR, "0x06fdde03");
    await ops.write({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" });
    expect(write).toHaveBeenCalledWith({ address: ADDR, calldata: "0xa0712d68", valueWei: "0", label: "mint" });
    await ops.explain({ prompt: "p" });
    expect(explain).toHaveBeenCalledWith("p");
  });
});
