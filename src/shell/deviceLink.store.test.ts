// HUP-S8.1 — the device-link review path. Linking a device is a personal_sign whose result core
// completes and stores: it must open the review gate and sign nothing, route approval to the
// dedicated command (never signing.broadcast), and route a decline to its own reject.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import type { CeremonyView } from "../bridge/types";

const view: CeremonyView = {
  id: "dlink-1",
  origin: "local-user",
  kind: "personal_sign",
  chainId: 40204,
  decoded: {
    action: 'Sign message: "Citrate DeviceLink v1…"',
    cost: "no funds moved",
    destination: "local-user",
  },
  requiresRawAck: false,
};

const linked = {
  thisDevice: "d1",
  links: [
    { device: "d1", member: "a1", wallet: "b1", index: 0, label: "Studio Mac", issuedAt: 1, thisDevice: true },
  ],
  revoked: [],
};

describe("HUP-S8.1 device link review", () => {
  let broadcastSpy: ReturnType<typeof vi.spyOn>;
  let approveSpy: ReturnType<typeof vi.spyOn>;
  let rejectSpy: ReturnType<typeof vi.spyOn>;
  let signingRejectSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    store.setState({ walletReview: null });
    vi.spyOn(bridge.cluster, "linkDeviceRequest").mockResolvedValue(view);
    approveSpy = vi.spyOn(bridge.cluster, "linkDeviceApprove").mockResolvedValue(linked);
    rejectSpy = vi.spyOn(bridge.cluster, "linkDeviceReject").mockResolvedValue(undefined);
    broadcastSpy = vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xhash", blockNumber: 1 });
    signingRejectSpy = vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("linkThisDevice opens the review gate and signs nothing", async () => {
    await store.linkThisDevice("Studio Mac");
    expect(store.state.walletReview?.kind).toBe("device-link");
    expect(store.state.walletReview?.view.id).toBe("dlink-1");
    expect(store.state.walletReview?.spendSummary).toBe("no funds move");
    expect(approveSpy).not.toHaveBeenCalled();
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  it("approve routes to the device-link command, never to broadcast, then refreshes", async () => {
    const done = vi.fn();
    await store.linkThisDevice("Studio Mac", done);
    await store.approveWalletReview();
    expect(approveSpy).toHaveBeenCalledWith("dlink-1", false);
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
    expect(done).toHaveBeenCalled();
  });

  it("a failed approve releases the ceremony and closes the review", async () => {
    approveSpy.mockRejectedValueOnce(new Error("the wallet signature does not verify"));
    await store.linkThisDevice("Studio Mac");
    await store.approveWalletReview();
    expect(rejectSpy).toHaveBeenCalledWith("dlink-1");
    expect(store.state.walletReview).toBeNull();
  });

  it("decline uses the device-link reject, not the generic signing reject", async () => {
    await store.linkThisDevice("Studio Mac");
    await store.rejectWalletReview();
    expect(rejectSpy).toHaveBeenCalledWith("dlink-1");
    expect(signingRejectSpy).not.toHaveBeenCalled();
    expect(approveSpy).not.toHaveBeenCalled();
  });

  it("a refused request (e.g. invalid name) opens no review", async () => {
    vi.spyOn(bridge.cluster, "linkDeviceRequest").mockRejectedValueOnce(new Error("device name must be 1 to 48 letters"));
    await store.linkThisDevice("bad\nname");
    expect(store.state.walletReview).toBeNull();
  });
});
