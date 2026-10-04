// Owner decision 2026-10-01: the Hermes sidecar agent loop is on by default for members. Installs
// that saved the old default (off) are switched on once; a member's later choice is kept.
import { beforeEach, describe, expect, it } from "vitest";
import { loadState, STORAGE_KEY } from "./state";

describe("sidecar agent loop default", () => {
  beforeEach(() => localStorage.clear());

  it("is on for a fresh install", () => {
    const s = loadState();
    expect(s.hermesSidecarLoop).toBe(true);
    expect(s.hermesSidecarLoopDefaultApplied).toBe(true);
  });

  it("switches an install that saved the old default (off) on, once", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ hermesSidecarLoop: false }));
    const s = loadState();
    expect(s.hermesSidecarLoop).toBe(true);
    expect(s.hermesSidecarLoopDefaultApplied).toBe(true);
  });

  it("keeps a member's later choice to turn it off", () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ hermesSidecarLoop: false, hermesSidecarLoopDefaultApplied: true }),
    );
    expect(loadState().hermesSidecarLoop).toBe(false);
  });
});
