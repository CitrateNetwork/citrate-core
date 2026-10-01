// HUP-S5.4 — outside the desktop app there are no windows to open, and the app says so instead of
// pretending a pop-out opened.
import { describe, it, expect, vi } from "vitest";
import { openPopout, startPopoutHost } from "./appHost";
import { store } from "../shell/store";

describe("HUP-S5.4 pop-outs in the web preview", () => {
  it("no host starts and opening tells the member it needs the desktop app", async () => {
    expect(startPopoutHost()).toBeNull();
    const toast = vi.spyOn(store, "toast");
    await openPopout("monitor");
    expect(toast).toHaveBeenCalledWith(expect.stringMatching(/desktop app/i));
  });
});
