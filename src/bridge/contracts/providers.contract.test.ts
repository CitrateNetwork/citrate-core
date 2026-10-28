// D-11 bridge contract — cloud model providers (status, usage, caps, halt/resume, one-way keys,
// custom endpoints). Rust side: `src-tauri/src/providers_ipc.rs`.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { bridge } from "../index";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriProviders } from "../tauri/providers";

const OPS = [
  "status",
  "usageSummary",
  "setCaps",
  "setPrice",
  "halt",
  "resume",
  "setKey",
  "testKey",
  "removeKey",
  "addCustom",
  "confirmCustom",
  "removeCustom",
  "resetUsageLog",
  "preferredGet",
  "preferredSet",
] as const;

const caps = { dailyTokens: 1000, monthlyTokens: 10000, dailyMicros: null, monthlyMicros: null };

/** Call every op once and return the `[command, args]` pairs it invoked. */
async function invokedCommands(): Promise<Array<[string, Record<string, unknown> | undefined]>> {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  await tauriProviders.status();
  await tauriProviders.usageSummary();
  await tauriProviders.setCaps("openai", caps);
  await tauriProviders.setPrice("openai", "m", { inputMicrosPerMtok: 1, outputMicrosPerMtok: 2 });
  await tauriProviders.halt("openai");
  await tauriProviders.resume("openai");
  await tauriProviders.setKey("openai", "test-only-not-a-key");
  await tauriProviders.testKey("openai");
  await tauriProviders.removeKey("openai");
  await tauriProviders.addCustom("Box", "https://llm.example.org/v1", ["m"]);
  await tauriProviders.confirmCustom("custom-1", "https://llm.example.org/v1");
  await tauriProviders.removeCustom("custom-1");
  await tauriProviders.resetUsageLog();
  await tauriProviders.preferredGet();
  await tauriProviders.preferredSet({ providerId: "openai", model: "gpt-4o" });
  return invokeMock.mock.calls.map((c) => [c[0] as string, c[1] as Record<string, unknown> | undefined]);
}

describe("bridge contract — providers (D-11)", () => {
  it("exposes the providers domain", () => {
    for (const k of OPS) expect(typeof bridge.providers[k]).toBe("function");
  });

  it("sim mode is honest (Rule 1): no providers, keys or spend in a browser preview", async () => {
    if (bridge.mode !== "sim") return;
    await expect(bridge.providers.status()).rejects.toThrow(/desktop app/);
    await expect(bridge.providers.usageSummary()).rejects.toThrow(/desktop app/);
    await expect(bridge.providers.setKey("openai", "x")).rejects.toThrow(/desktop app/);
  });
});

describe("tauri providers invokes the registered commands", () => {
  beforeEach(() => invokeMock.mockReset());

  it("maps each op to its command and argument names", async () => {
    const calls = await invokedCommands();
    expect(calls).toEqual([
      ["providers_status", undefined],
      ["providers_usage_summary", undefined],
      ["providers_caps_set", { providerId: "openai", caps }],
      ["providers_price_set", { providerId: "openai", model: "m", price: { inputMicrosPerMtok: 1, outputMicrosPerMtok: 2 } }],
      ["providers_halt", { providerId: "openai" }],
      ["providers_resume", { providerId: "openai" }],
      ["providers_key_set", { providerId: "openai", apiKey: "test-only-not-a-key" }],
      ["providers_key_test", { providerId: "openai" }],
      ["providers_key_remove", { providerId: "openai" }],
      ["providers_custom_add", { label: "Box", baseUrl: "https://llm.example.org/v1", models: ["m"] }],
      ["providers_custom_confirm", { id: "custom-1", baseUrl: "https://llm.example.org/v1" }],
      ["providers_custom_remove", { id: "custom-1" }],
      ["providers_usage_log_reset", undefined],
      ["providers_preferred_get", undefined],
      ["providers_preferred_set", { choice: { providerId: "openai", model: "gpt-4o" } }],
    ]);
  });

  it("every providers_* command is allowed for the main window, and no other", async () => {
    const toml = readFileSync(join(__dirname, "../../../src-tauri/permissions/main-window.toml"), "utf8");
    const allowed = [...toml.matchAll(/"(providers_[a-z_]+)"/g)].map((m) => m[1]).sort();
    const used = (await invokedCommands()).map(([c]) => c).sort();
    expect(allowed).toEqual(used);
  });

  it("only setKey sends a key, and nothing reads one back", async () => {
    const calls = await invokedCommands();
    const withKey = calls.filter(([, a]) => a && "apiKey" in a).map(([c]) => c);
    expect(withKey).toEqual(["providers_key_set"]);
    expect(Object.keys(tauriProviders).some((k) => /^(get|read|reveal|export)Key/i.test(k))).toBe(false);
  });
});
