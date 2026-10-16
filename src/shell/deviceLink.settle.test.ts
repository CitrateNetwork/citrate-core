// HUP-S8.2: the fleet wizard waits on "Link this machine". If core refuses to even open the review
// (bad name, no wallet), the caller is still told it is over, so nothing waits forever.
import { afterEach, describe, expect, it, vi } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";

describe("linkThisDevice settles its caller", () => {
  afterEach(() => vi.restoreAllMocks());
  it("calls onDone when the review cannot be opened, and opens no review", async () => {
    store.setState({ walletReview: null });
    vi.spyOn(bridge.cluster, "linkDeviceRequest").mockRejectedValueOnce(new Error("no wallet"));
    const onDone = vi.fn();
    await store.linkThisDevice("Studio Mac", onDone);
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(store.state.walletReview).toBeNull();
  });
});
