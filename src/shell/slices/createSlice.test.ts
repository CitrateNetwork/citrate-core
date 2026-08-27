import { describe, it, expect, vi } from "vitest";
import { createSlice } from "./createSlice";

describe("createSlice — the CX per-feature state primitive (CX-S0.1)", () => {
  it("returns the initial state and a stable reference until set", () => {
    const s = createSlice({ n: 1, label: "a" });
    const first = s.get();
    expect(first).toEqual({ n: 1, label: "a" });
    expect(s.get()).toBe(first); // stable reference between sets (getSnapshot stability)
  });

  it("merges an object patch and yields a NEW reference", () => {
    const s = createSlice({ n: 1, label: "a" });
    const before = s.get();
    s.set({ n: 2 });
    expect(s.get()).toEqual({ n: 2, label: "a" });
    expect(s.get()).not.toBe(before); // new identity so subscribers/getSnapshot fire
  });

  it("merges a functional patch computed from current state", () => {
    const s = createSlice({ n: 1 });
    s.set((cur) => ({ n: cur.n + 10 }));
    expect(s.get().n).toBe(11);
  });

  it("notifies subscribers on set and stops after unsubscribe", () => {
    const s = createSlice({ n: 0 });
    const fn = vi.fn();
    const off = s.subscribe(fn);
    s.set({ n: 1 });
    s.set({ n: 2 });
    expect(fn).toHaveBeenCalledTimes(2);
    off();
    s.set({ n: 3 });
    expect(fn).toHaveBeenCalledTimes(2); // no further calls after unsubscribe
  });

  it("isolates instances — two slices do not share state (per-feature ownership)", () => {
    const a = createSlice({ v: "models" });
    const b = createSlice({ v: "storage" });
    a.set({ v: "models-updated" });
    expect(a.get().v).toBe("models-updated");
    expect(b.get().v).toBe("storage"); // unaffected
  });
});
