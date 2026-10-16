// HUP-S2.6 (US-7.2 AC1) — every answer on an approval card or a wallet review becomes a local
// decision record (core's HIC outbox, then the records the nightly anchor covers).
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { CeremonyView } from "../bridge/types";

const ticks = async (n = 10) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

const view = { id: "cer-1", requiresRawAck: false } as unknown as CeremonyView;

beforeEach(() => {
  store.setState({ queue: [], cerPhase: "review", walletReview: null, toast: null });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ queue: [], walletReview: null, toast: null });
});

describe("Feature: decisions leave a trail", () => {
  it("Given an agent tool card, when the member declines, then a denied tool approval is recorded", async () => {
    const rec = vi.spyOn(bridge.agentHarness, "recordDecision").mockResolvedValue(7);
    const done = store.requestSig({
      origin: "chat agent",
      requester: "dashboard agent · tool journal_append",
      title: "Write to your journal",
      chainless: true,
      rows: [],
      cost: "none",
      sponsor: "no chain transaction",
      sponsorColor: "",
      card: { kind: "fields", tool: "journal_append", summary: "Hermes wants to write to your journal", rows: [] },
      hic: { reason: "this session read untrusted content" },
    });
    store.finishCer("declined");
    expect(await done).toBe("declined");
    expect(rec).toHaveBeenCalledWith(
      "agent.tool_approval",
      "denied",
      "Write to your journal · dashboard agent · tool journal_append",
      "this session read untrusted content",
    );
  });

  it("Given a ceremony without a tool card, when the member approves, then an approved ceremony is recorded", async () => {
    const rec = vi.spyOn(bridge.agentHarness, "recordDecision").mockResolvedValue(8);
    const done = store.requestSig({
      origin: "wallet",
      requester: "Send",
      title: "Send 1 SALT",
      rows: [],
      cost: "0.0001 SALT",
      sponsor: "you",
      sponsorColor: "",
    });
    store.finishCer("approved");
    await done;
    expect(rec).toHaveBeenCalledWith("ceremony.approval", "approved", "Send 1 SALT · Send", "the member's answer on the approval card");
  });

  it("Given a wallet review, then both an approval and a rejection are recorded", async () => {
    const rec = vi.spyOn(bridge.agentHarness, "recordDecision").mockResolvedValue(9);
    vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined as never);
    store.openWalletReview("send", "Send SALT", view, "1 SALT");
    await store.rejectWalletReview();
    expect(rec).toHaveBeenLastCalledWith("ceremony.approval", "denied", "send: Send SALT", "1 SALT");

    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(new Error("offline"));
    store.openWalletReview("send", "Send SALT", view, "1 SALT");
    await store.approveWalletReview(true);
    expect(rec).toHaveBeenLastCalledWith("ceremony.approval", "approved", "send: Send SALT", "1 SALT");
  });

  it("Given the record cannot be written, then the member is told, and the decision itself stands", async () => {
    vi.spyOn(bridge.agentHarness, "recordDecision").mockRejectedValue(new Error("the outbox is full"));
    const done = store.requestSig({ origin: "x", requester: "y", title: "z", rows: [], cost: "", sponsor: "", sponsorColor: "" });
    store.finishCer("approved");
    expect(await done).toBe("approved");
    await ticks();
    expect(store.state.toast).toContain("not recorded");
    expect(store.state.toast).toContain("the outbox is full");
  });

  it("Given a build that keeps no decision records, then nothing is claimed and nothing is shown", async () => {
    vi.spyOn(bridge.agentHarness, "recordDecision").mockResolvedValue(null);
    const done = store.requestSig({ origin: "x", requester: "y", title: "z", rows: [], cost: "", sponsor: "", sponsorColor: "" });
    store.finishCer("declined");
    await done;
    await ticks();
    expect(store.state.toast).toBeNull();
  });
});
