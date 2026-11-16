// V6: the web preview has no on-device scanner, so the check fails and nothing is posted.
import { describe, it, expect } from "vitest";
import { scanForDisclosure, SCAN_NEEDS_DESKTOP } from "./disclosureScan";

describe("scanForDisclosure", () => {
  it("fails closed outside the desktop app", async () => {
    await expect(scanForDisclosure("hello")).rejects.toThrow(SCAN_NEEDS_DESKTOP);
  });
});
