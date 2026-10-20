import { describe, expect, it, vi } from "vitest";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus } from "./webSearch";
import { publishWebSettings, readWebSettingsUnlessSuperseded, subscribeWebSettings, withWebSettingsLock } from "./webSettingsSync";

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

  it("a read with no save published while it was in flight resolves to its answer", async () => {
    await expect(readWebSettingsUnlessSuperseded(async () => st(true))).resolves.toEqual(st(true));
  });

  it("a read that answers after another view published a save resolves to null (the answer is older)", async () => {
    let answer: (v: HermesWebStatus) => void = () => {};
    const read = readWebSettingsUnlessSuperseded(() => new Promise<HermesWebStatus>((r) => (answer = r)));
    publishWebSettings(st(true), Symbol("other view"));
    answer(st(false));
    await expect(read).resolves.toBeNull();
  });

  it("a save published before the read began does not drop it", async () => {
    publishWebSettings(st(true), Symbol("other view"));
    await expect(readWebSettingsUnlessSuperseded(async () => st(true))).resolves.toEqual(st(true));
  });

  it("only saves during the read count: two reads, a save between them, only the first is dropped", async () => {
    let answerFirst: (v: HermesWebStatus) => void = () => {};
    const first = readWebSettingsUnlessSuperseded(() => new Promise<HermesWebStatus>((r) => (answerFirst = r)));
    publishWebSettings(st(true), Symbol("other view"));
    const second = readWebSettingsUnlessSuperseded(async () => st(true));
    answerFirst(st(false));
    await expect(first).resolves.toBeNull();
    await expect(second).resolves.toEqual(st(true));
  });

  it("a failed read still rejects to its caller", async () => {
    await expect(readWebSettingsUnlessSuperseded(async () => Promise.reject(new Error("core went away")))).rejects.toThrow("core went away");
  });
});
