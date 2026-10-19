import { describe, expect, it, vi } from "vitest";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus } from "./webSearch";
import { publishWebSettings, subscribeWebSettings } from "./webSettingsSync";

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
});
