import { describe, it, expect } from "vitest";
import { EXPECTED_GENESIS, networkCompat } from "./networkCompat";

const A = "0x0f2b567fad0a376f11d1d1d58511c089b40cceb4810f77eb7eed1e22558ebf72";
const B = "0x98e0d72f422049606a6b29ca0a9bcfd2300753fd36526d2c8e9f9a2531b70c73";

describe("networkCompat", () => {
  it("is compatible only when block 0 matches the build's genesis", () => {
    expect(networkCompat(A, A)).toBe("compatible");
    expect(networkCompat(A, A.toUpperCase().replace("0X", "0x"))).toBe("compatible");
  });

  it("flags a different block 0 as a retired network", () => {
    expect(networkCompat(A, B)).toBe("retired");
    expect(networkCompat(B, A)).toBe("retired");
  });

  it("never claims retired without a definite answer", () => {
    expect(networkCompat(A, null)).toBe("unknown");
    expect(networkCompat(A, undefined)).toBe("unknown");
    expect(networkCompat(A, "")).toBe("unknown");
    expect(networkCompat(A, "0x1234")).toBe("unknown");
    expect(networkCompat("", A)).toBe("unknown");
  });

  it("the embedded address book pins a real genesis hash", () => {
    expect(EXPECTED_GENESIS).toMatch(/^0x[0-9a-f]{64}$/);
    expect(EXPECTED_GENESIS).not.toBe("0x" + "0".repeat(64));
  });
});
