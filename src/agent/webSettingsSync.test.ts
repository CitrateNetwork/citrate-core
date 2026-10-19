import { describe, expect, it, vi } from "vitest";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus } from "./webSearch";
import { publishWebSettings, subscribeWebSettings, withWebSettingsLock } from "./webSettingsSync";

const st = (searchEnabled: boolean): HermesWebStatus => ({
  settings: { ...DEFAULT_WEB_SETTINGS, searchEnabled },
  managedChromium: null,
  searxngFound: false,
  jinaKeyFileFound: false,
  jevKeyFileFound: false,
  notices: [],
  appliesOnRestart: true,
});

describe("webSettingsSync", () => {
  it("delivers a save to every subscriber with the writer's id", () => {
    const a = vi.fn();
    const b = vi.fn();
    const ua = subscribeWebSettings(a);
    const ub = subscribeWebSettings(b);
    const writer = Symbol("writer");
    publishWebSettings(st(true), writer);
    expect(a).toHaveBeenCalledWith(st(true), writer);
    expect(b).toHaveBeenCalledWith(st(true), writer);
    ua();
    ub();
  });

  it("stops delivering after unsubscribe", () => {
    const a = vi.fn();
    const ua = subscribeWebSettings(a);
    ua();
    publishWebSettings(st(false), Symbol("w"));
    expect(a).not.toHaveBeenCalled();
  });

  it("runs writes one at a time, in the order asked, even when the first is slower", async () => {
    const log: string[] = [];
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const a = withWebSettingsLock(async () => {
      log.push("a read");
      await tick();
      await tick();
      log.push("a write");
      return "a";
    });
    const b = withWebSettingsLock(async () => {
      log.push("b read");
      await tick();
      log.push("b write");
      return "b";
    });
    expect(await Promise.all([a, b])).toEqual(["a", "b"]);
    expect(log).toEqual(["a read", "a write", "b read", "b write"]);
  });

  it("a failed write rejects to its caller and releases the lock for the next one", async () => {
    const failed = withWebSettingsLock(async () => {
      throw new Error("core went away");
    });
    const next = withWebSettingsLock(async () => "ok");
    await expect(failed).rejects.toThrow("core went away");
    await expect(next).resolves.toBe("ok");
  });
});
