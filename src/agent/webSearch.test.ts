// HUP-S5.2 / S5.3: the pure helpers behind the "Web search & decisions" Settings card.
import { describe, it, expect, vi } from "vitest";
import {
  DEFAULT_WEB_SETTINGS,
  cleanSettings,
  isAbsolutePath,
  loadWebSettings,
  normalizeOrigin,
  parseOrigins,
  saveWebSettings,
  sendsToThirdParty,
  type WebSettingsIo,
} from "./webSearch";

describe("web opt-ins: defaults", () => {
  it("everything is off or local by default", () => {
    expect(DEFAULT_WEB_SETTINGS.searchEnabled).toBe(false);
    expect(DEFAULT_WEB_SETTINGS.reader).toBe("local");
    expect(DEFAULT_WEB_SETTINGS.jevEnabled).toBe(false);
    expect(DEFAULT_WEB_SETTINGS.jevOrigins).toEqual([]);
    expect(sendsToThirdParty(DEFAULT_WEB_SETTINGS)).toBe(false);
  });

  it("only the Jina reader with search on, or Jev, sends data to a third party", () => {
    expect(sendsToThirdParty({ ...DEFAULT_WEB_SETTINGS, reader: "jina" })).toBe(false);
    expect(sendsToThirdParty({ ...DEFAULT_WEB_SETTINGS, searchEnabled: true })).toBe(false);
    expect(sendsToThirdParty({ ...DEFAULT_WEB_SETTINGS, searchEnabled: true, reader: "jina" })).toBe(true);
    expect(sendsToThirdParty({ ...DEFAULT_WEB_SETTINGS, jevEnabled: true })).toBe(true);
  });
});

describe("web opt-ins: origins", () => {
  it("normalizes https origins and refuses everything else", () => {
    expect(normalizeOrigin(" https://Shop.Example/ ")).toBe("https://shop.example");
    expect(normalizeOrigin("https://docs.example:8443")).toBe("https://docs.example:8443");
    for (const bad of ["http://shop.example", "https://shop.example/path", "https://u@shop.example", "shop.example", "https://", "https://a b"]) {
      expect(normalizeOrigin(bad)).toBeNull();
    }
  });

  it("parses lines and commas, dedupes, and reports bad entries", () => {
    const r = parseOrigins("https://a.example\nhttps://A.example/, http://b.example\n\n https://c.example:9 ");
    expect(r.origins).toEqual(["https://a.example", "https://c.example:9"]);
    expect(r.bad).toEqual(["http://b.example"]);
  });
});

describe("web opt-ins: paths and the wire", () => {
  it("recognizes absolute paths", () => {
    expect(isAbsolutePath("/opt/searxng/bin/searxng-run")).toBe(true);
    expect(isAbsolutePath("C:\\searxng\\searxng-run.exe")).toBe(true);
    expect(isAbsolutePath("searxng-run")).toBe(false);
  });

  it("blank paths are sent as null, and the right commands are called", async () => {
    const invoke = vi.fn(async (_cmd: string, _args?: Record<string, unknown>) => ({}) as never);
    const io: WebSettingsIo = { mode: "tauri", invoke: invoke as WebSettingsIo["invoke"] };
    await loadWebSettings(io);
    expect(invoke).toHaveBeenLastCalledWith("hermes_web_settings_get");
    await saveWebSettings(io, { ...DEFAULT_WEB_SETTINGS, searchEnabled: true, searxngPath: "   ", jevKeyFile: " /k " });
    expect(invoke).toHaveBeenLastCalledWith("hermes_web_settings_set", {
      settings: { ...DEFAULT_WEB_SETTINGS, searchEnabled: true, searxngPath: null, jevKeyFile: "/k" },
    });
    expect(cleanSettings(DEFAULT_WEB_SETTINGS)).toEqual(DEFAULT_WEB_SETTINGS);
  });
});
