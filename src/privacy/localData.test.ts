// HUP-S10.5 — "delete my local data" (webview half): dry run first, typed confirmation,
// webview storage cleared before the Rust delete. Written red-first, then implemented.
import { describe, it, expect, vi } from "vitest";
import type { PrivacyIo } from "./privacyIo";
import { CONFIRM_PHRASE, WALLET_CONFIRM_PHRASE, confirmationError, deleteLocalData, formatBytes, planLocalData, type DataPlan } from "./localData";

const plan: DataPlan = {
  options: { includeWallet: false, keepModels: false },
  entries: [{ path: "/d/ai.citrate.core/models", kind: "data", bytes: 5_000_000_000, action: "delete", reason: null }],
  keychain: [{ service: "ai.citrate.core", account: "node-storage-key", label: "Node storage key", present: true, action: "delete" }],
  deleteBytes: 5_000_000_000,
  keepBytes: 0,
  confirmPhrase: "delete my local data",
  walletConfirmPhrase: null,
  notes: [],
};

function io(over: Partial<PrivacyIo> = {}): PrivacyIo {
  return {
    mode: "tauri",
    pickSavePath: async () => null,
    pickOpenPath: async () => null,
    invoke: vi.fn(async () => plan) as PrivacyIo["invoke"],
    clearWebStorage: vi.fn(),
    ...over,
  };
}

describe("confirmation", () => {
  it("matches the Rust phrases", () => {
    expect(CONFIRM_PHRASE).toBe("delete my local data");
    expect(WALLET_CONFIRM_PHRASE).toBe("delete my wallet");
  });
  it("needs the exact phrase, and a second one for the wallet", () => {
    const o = { includeWallet: false, keepModels: false };
    expect(confirmationError(o, "delete", "")).toMatch(/Type/);
    expect(confirmationError(o, "  Delete my LOCAL data ", "")).toBeNull();
    const w = { includeWallet: true, keepModels: false };
    expect(confirmationError(w, "delete my local data", "")).toMatch(/wallet/);
    expect(confirmationError(w, "delete my local data", "delete my wallet")).toBeNull();
  });
});

describe("dry run", () => {
  it("asks Rust for the plan with the chosen options", async () => {
    const invoke = vi.fn(async () => plan);
    const r = await planLocalData(io({ invoke: invoke as PrivacyIo["invoke"] }), { includeWallet: false, keepModels: true });
    expect(r).toEqual({ ok: true, plan });
    expect(invoke).toHaveBeenCalledWith("local_data_plan", { options: { includeWallet: false, keepModels: true } });
  });
  it("the web preview says it needs the desktop app", async () => {
    const r = await planLocalData(io({ mode: "sim" }), { includeWallet: false, keepModels: false });
    expect(r.ok).toBe(false);
  });
});

describe("delete", () => {
  it("refuses without the phrase and touches nothing", async () => {
    const invoke = vi.fn();
    const clear = vi.fn();
    const r = await deleteLocalData(io({ invoke: invoke as PrivacyIo["invoke"], clearWebStorage: clear }), { includeWallet: false, keepModels: false }, "nope", "");
    expect(r.ok).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    expect(clear).not.toHaveBeenCalled();
  });

  it("clears webview storage first, then runs the Rust delete", async () => {
    const order: string[] = [];
    const report = { deleted: ["/d/ai.citrate.core/models"], failed: [], keychainDeleted: ["ai.citrate.core/node-storage-key"], keychainFailed: [] };
    const invoke = vi.fn(async () => {
      order.push("invoke");
      return report;
    });
    const clear = vi.fn(() => order.push("clear"));
    const r = await deleteLocalData(
      io({ invoke: invoke as PrivacyIo["invoke"], clearWebStorage: clear }),
      { includeWallet: true, keepModels: false },
      "delete my local data",
      "delete my wallet",
    );
    expect(r).toEqual({ ok: true, report });
    expect(order).toEqual(["clear", "invoke"]);
    expect(invoke).toHaveBeenCalledWith("local_data_delete", {
      options: { includeWallet: true, keepModels: false },
      confirm: "delete my local data",
      walletConfirm: "delete my wallet",
    });
  });
});

describe("formatBytes", () => {
  it("reads like a file manager", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(5_000_000_000)).toBe("4.7 GB");
  });
});
