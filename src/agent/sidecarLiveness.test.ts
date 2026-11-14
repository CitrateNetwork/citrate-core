// 0.5.4 (owner 2026-11-14): the "Hermes is not running" notice must reflect the real state.
import { describe, it, expect, vi } from "vitest";
import {
  SIDECAR_DOWN_NOTICE,
  SIDECAR_LOOP_OFF_NOTICE,
  downReason,
  hermesRowRunning,
  labelFor,
  noticeFor,
  onHermesFlip,
  readSidecar,
  sameReason,
  sidecarUnreadableNotice,
} from "./sidecarLiveness";
import { createSlice } from "../shell/slices/createSlice";

describe("readSidecar: one fresh read, never a guess", () => {
  it("running is up, not running is stopped", async () => {
    expect(await readSidecar(async () => ({ running: true }))).toEqual({ up: true });
    expect(await readSidecar(async () => ({ running: false }))).toEqual({ up: false, why: "stopped" });
  });

  it("a failed status call is unreadable with its error, not 'not running'", async () => {
    const r = await readSidecar(async () => {
      throw new Error("hermes control: connection refused");
    });
    expect(r).toEqual({ up: false, why: "unreadable", error: "hermes control: connection refused" });
  });

  it("a very long error is cut so the toast stays readable", async () => {
    const r = await readSidecar(async () => {
      throw new Error("x".repeat(500));
    });
    expect(r.up).toBe(false);
    if (!r.up && r.why === "unreadable") expect(r.error.length).toBeLessThanOrEqual(160);
  });
});

describe("downReason and the notice it gives", () => {
  it("the loop switched off says so, whatever Hermes's state", () => {
    expect(downReason(false, { up: true })).toEqual({ why: "loop_off" });
    expect(downReason(false, null)).toEqual({ why: "loop_off" });
    expect(noticeFor({ why: "loop_off" })).toBe(SIDECAR_LOOP_OFF_NOTICE);
    expect(SIDECAR_LOOP_OFF_NOTICE).not.toMatch(/not running/);
  });

  it("only a read that says not running gives the not-running notice", () => {
    expect(noticeFor(downReason(true, { up: false, why: "stopped" }))).toBe(SIDECAR_DOWN_NOTICE);
    const unreadable = downReason(true, { up: false, why: "unreadable", error: "timed out" });
    expect(noticeFor(unreadable)).toBe(sidecarUnreadableNotice("timed out"));
    expect(noticeFor(unreadable)).not.toMatch(/not running/);
  });

  it("every label still says no tools", () => {
    for (const r of [{ why: "stopped" }, { why: "loop_off" }, { why: "unreadable", error: "e" }] as const) {
      expect(labelFor(r)).toMatch(/no tools/);
    }
  });

  it("sameReason compares the case and the error", () => {
    expect(sameReason({ why: "stopped" }, { why: "stopped" })).toBe(true);
    expect(sameReason({ why: "stopped" }, { why: "loop_off" })).toBe(false);
    expect(sameReason({ why: "unreadable", error: "a" }, { why: "unreadable", error: "a" })).toBe(true);
    expect(sameReason({ why: "unreadable", error: "a" }, { why: "unreadable", error: "b" })).toBe(false);
  });
});

describe("onHermesFlip", () => {
  it("fires only when the Hermes row flips between running and not running", () => {
    const slice = createSlice({ attached: [{ id: "hermes", state: "off" }], other: 0 });
    const flips: boolean[] = [];
    const unsub = onHermesFlip(slice.subscribe, slice.get, (r) => flips.push(r));
    slice.set({ other: 1 });
    slice.set({ attached: [{ id: "hermes", state: "starting" }] });
    expect(flips).toEqual([]);
    slice.set({ attached: [{ id: "hermes", state: "running" }] });
    slice.set({ attached: [{ id: "hermes", state: "running" }] });
    slice.set({ attached: [{ id: "hermes", state: "off" }] });
    expect(flips).toEqual([true, false]);
    unsub();
    slice.set({ attached: [{ id: "hermes", state: "running" }] });
    expect(flips).toEqual([true, false]);
  });

  it("another runtime's row never counts", () => {
    expect(hermesRowRunning([{ id: "openclaw", state: "running" }])).toBe(false);
    const fn = vi.fn();
    const slice = createSlice({ attached: [{ id: "openclaw", state: "off" }] });
    onHermesFlip(slice.subscribe, slice.get, fn);
    slice.set({ attached: [{ id: "openclaw", state: "running" }] });
    expect(fn).not.toHaveBeenCalled();
  });
});
