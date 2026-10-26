// D-11 (RC52) — the Settings › Models container talks only to `bridge.providers`: the key goes in
// once and leaves the input, "remove here and revoke" removes before it opens the vendor page, and a
// failed connect says the key was not stored.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { vendor, usage, NOW } from "./fixtures/providerStates.fixture";

const providers = {
  status: vi.fn(),
  usageSummary: vi.fn(),
  setKey: vi.fn(),
  testKey: vi.fn(),
  removeKey: vi.fn(),
  setCaps: vi.fn(),
  setPrice: vi.fn(),
  halt: vi.fn(),
  resume: vi.fn(),
  addCustom: vi.fn(),
  confirmCustom: vi.fn(),
  removeCustom: vi.fn(),
  resetUsageLog: vi.fn(),
};
vi.mock("../../bridge", () => ({ bridge: { providers } }));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri" }));

const { ModelsSettings } = await import("./ModelsSettings");

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
const opened: string[] = [];

beforeEach(() => {
  for (const f of Object.values(providers)) f.mockReset();
  opened.length = 0;
  providers.usageSummary.mockResolvedValue({ logIntact: true, logProblem: null, records: 0, asOfMs: NOW, providers: [usage()], recentCalls: [], clockProblem: null });
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<ModelsSettings onOpen={(u) => opened.push(u)} />);
  });
}
const q = <T extends Element>(sel: string) => host.querySelector(sel) as T | null;
const button = (text: string) => Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
async function type(input: HTMLInputElement | null, value: string) {
  expect(input, "the input is on the page").toBeInstanceOf(HTMLInputElement);
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("Settings › Models container", () => {
  it("connect hands the key to core once and clears the input", async () => {
    providers.status.mockResolvedValue([vendor()]);
    providers.setKey.mockResolvedValue({ ok: true, listedModels: 3 });
    await mount();
    const input = q<HTMLInputElement>('input[aria-label="OpenAI API key"]');
    expect(input).toBeTruthy();
    await type(input, "test-key-not-real-0123456789");
    providers.status.mockResolvedValue([vendor({ keyConfigured: true, keySetMs: NOW })]);
    await act(async () => {
      button("Connect")?.click();
    });
    expect(providers.setKey).toHaveBeenCalledWith("openai", "test-key-not-real-0123456789");
    expect(host.innerHTML).not.toContain("test-key-not-real");
    expect(host.textContent).toContain("Connected. The key passed a live test (3 models listed).");
  });

  it("a failed connect says the key was not stored and keeps no copy", async () => {
    providers.status.mockResolvedValue([vendor()]);
    providers.setKey.mockRejectedValue(new Error("the vendor rejected the key"));
    await mount();
    await type(q<HTMLInputElement>('input[aria-label="OpenAI API key"]'), "test-key-not-real-0123456789");
    await act(async () => {
      button("Connect")?.click();
    });
    expect(host.textContent).toContain("The key was not stored: the vendor rejected the key");
    expect((q<HTMLInputElement>('input[aria-label="OpenAI API key"]') as HTMLInputElement).value).toBe("");
  });

  it("a failed test is shown on the card", async () => {
    providers.status.mockResolvedValue([vendor({ keyConfigured: true })]);
    providers.testKey.mockRejectedValue(new Error("rate limited"));
    await mount();
    await act(async () => {
      button("Test key")?.click();
    });
    expect(providers.testKey).toHaveBeenCalledWith("openai");
    expect(host.textContent).toContain("The key test failed: rate limited");
  });

  it("remove here and revoke: removes the key first, then opens the vendor's revoke page", async () => {
    providers.status.mockResolvedValue([vendor({ keyConfigured: true })]);
    const order: string[] = [];
    providers.removeKey.mockImplementation(async () => {
      order.push("remove");
    });
    await mount();
    await act(async () => {
      button("Remove here and open the OpenAI revoke page")?.click();
    });
    expect(providers.removeKey).toHaveBeenCalledWith("openai");
    expect(order).toEqual(["remove"]);
    expect(opened).toEqual(["https://platform.openai.com/api-keys"]);
  });

  it("a failed remove does not open the revoke page as if it had worked", async () => {
    providers.status.mockResolvedValue([vendor({ keyConfigured: true })]);
    providers.removeKey.mockRejectedValue(new Error("keychain locked"));
    await mount();
    await act(async () => {
      button("Remove here and open the OpenAI revoke page")?.click();
    });
    expect(opened).toEqual([]);
    expect(host.textContent).toContain("keychain locked");
  });

  it("resume calls core's resume for that provider", async () => {
    providers.status.mockResolvedValue([vendor({ keyConfigured: true, halt: { kind: "unknownModel", message: "paused", atMs: NOW } })]);
    providers.resume.mockResolvedValue(vendor({ keyConfigured: true }));
    await mount();
    await act(async () => {
      button("Resume and approve this model")?.click();
    });
    expect(providers.resume).toHaveBeenCalledWith("openai");
  });

  it("saving caps sends both token and spending caps; a bad form never reaches core", async () => {
    providers.status.mockResolvedValue([vendor({ keyConfigured: true })]);
    await mount();
    await act(async () => {
      button("Edit caps")?.click();
    });
    await type(q<HTMLInputElement>('input[aria-label="daily spending cap"]'), "60");
    await act(async () => {
      button("Save caps")?.click();
    });
    expect(providers.setCaps).not.toHaveBeenCalled();
    expect(host.textContent).toContain("The daily spending cap is above the monthly one.");
    await type(q<HTMLInputElement>('input[aria-label="daily spending cap"]'), "2.50");
    await act(async () => {
      button("Save caps")?.click();
    });
    expect(providers.setCaps).toHaveBeenCalledWith("openai", { dailyTokens: 1_000_000, monthlyTokens: 10_000_000, dailyMicros: 2_500_000, monthlyMicros: 50_000_000 });
  });

  it("a confirmed custom endpoint is confirmed with exactly what the member typed", async () => {
    const c = vendor({ id: "custom-ab12", name: "Box", kind: "custom", verified: false, baseUrl: "https://llm.example.org/v1", confirmed: false, keyPage: null, revokePage: null });
    providers.status.mockResolvedValue([c]);
    await mount();
    await type(q<HTMLInputElement>('input[aria-label="confirm the URL for Box"]'), "https://llm.example.org/v1");
    await act(async () => {
      button("Confirm endpoint")?.click();
    });
    expect(providers.confirmCustom).toHaveBeenCalledWith("custom-ab12", "https://llm.example.org/v1");
  });
});
