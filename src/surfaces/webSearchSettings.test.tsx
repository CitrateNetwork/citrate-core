// HUP-S5.2 / S5.3 (Rule 1 display honesty): the "Web search & decisions" Settings card.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus, type WebSettingsIo } from "../agent/webSearch";
import { WebSearchSettings } from "./WebSearchSettings";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function status(over: Partial<HermesWebStatus> = {}): HermesWebStatus {
  return {
    settings: DEFAULT_WEB_SETTINGS,
    managedChromium: null,
    searxngFound: false,
    jinaKeyFileFound: false,
    jevKeyFileFound: false,
    notices: [],
    appliesOnRestart: true,
    ...over,
  };
}

async function mount(io: WebSettingsIo): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<WebSearchSettings io={io} />);
  });
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
}
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const node = el as HTMLInputElement | HTMLTextAreaElement;
  const proto = node instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(node, value);
    node.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function tauriIo(initial: HermesWebStatus) {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const invoke = vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    calls.push({ cmd, args });
    if (cmd === "hermes_web_settings_get") return initial;
    const settings = (args?.settings ?? DEFAULT_WEB_SETTINGS) as HermesWebStatus["settings"];
    return status({ settings, notices: settings.jevEnabled ? ["Jev (TypeSafe) is on, but no key file was found, so it stays off and every decision stays local."] : [] });
  });
  return { io: { mode: "tauri", invoke: invoke as WebSettingsIo["invoke"] } as WebSettingsIo, calls };
}

describe("WebSearchSettings", () => {
  it("in the browser preview it says it is desktop-only, shows defaults, and calls nothing", async () => {
    const invoke = vi.fn();
    const { host, root } = await mount({ mode: "sim", invoke: invoke as WebSettingsIo["invoke"] });
    expect(q(host, "web-settings-preview")?.textContent).toContain("desktop-only");
    expect(q<HTMLInputElement>(host, "web-search-toggle")?.checked).toBe(false);
    expect(q<HTMLInputElement>(host, "web-search-toggle")?.disabled).toBe(true);
    expect(q<HTMLInputElement>(host, "reader-local")?.checked).toBe(true);
    expect(q<HTMLInputElement>(host, "jev-toggle")?.checked).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
    act(() => root.unmount());
  });

  it("defaults are off and local, and say nothing leaves the machine", async () => {
    const { io } = tauriIo(status());
    const { host, root } = await mount(io);
    expect(q<HTMLInputElement>(host, "web-search-toggle")?.checked).toBe(false);
    expect(q<HTMLInputElement>(host, "browser-toggle")?.checked).toBe(false);
    expect(q(host, "browser-state")?.textContent).toContain("not installed yet");
    expect(q(host, "web-local-only")).toBeTruthy();
    expect(q(host, "searxng-state")?.textContent).toContain("not installed");
    expect(q(host, "jina-notice")).toBeNull();
    expect(q(host, "jev-notice")).toBeNull();
    act(() => root.unmount());
  });

  it("choosing the Jina reader shows the third-party notice before anything is saved", async () => {
    const { io, calls } = tauriIo(status());
    const { host, root } = await mount(io);
    await click(q(host, "web-search-toggle"));
    await click(q(host, "reader-jina"));
    expect(q(host, "jina-notice")?.textContent).toContain("r.jina.ai");
    expect(q(host, "web-local-only")).toBeNull();
    expect(calls.filter((c) => c.cmd === "hermes_web_settings_set")).toHaveLength(0);
    act(() => root.unmount());
  });

  it("Jev shows its egress notice and the pending custody note, refuses bad sites, and saves good ones", async () => {
    const { io, calls } = tauriIo(status());
    const { host, root } = await mount(io);
    await click(q(host, "jev-toggle"));
    expect(q(host, "jev-notice")?.textContent).toContain("TypeSafe");
    expect(q(host, "jev-key-custody")?.textContent).toContain("pending owner sign-off");
    await type(q(host, "jev-origins"), "http://shop.example");
    expect(q(host, "jev-origins-bad")?.textContent).toContain("http://shop.example");
    expect(q<HTMLButtonElement>(host, "web-save")?.disabled).toBe(true);
    await type(q(host, "jev-origins"), "https://Shop.Example/\nhttps://docs.example");
    expect(q(host, "jev-origins-bad")).toBeNull();
    await click(q(host, "web-save"));
    const set = calls.filter((c) => c.cmd === "hermes_web_settings_set");
    expect(set).toHaveLength(1);
    const sent = set[0].args?.settings as HermesWebStatus["settings"];
    expect(sent.jevEnabled).toBe(true);
    expect(sent.jevOrigins).toEqual(["https://shop.example", "https://docs.example"]);
    expect(sent.searchEnabled).toBe(false);
    expect(q(host, "web-notices")?.textContent).toContain("stays off");
    act(() => root.unmount());
  });

  it("the browser switch is saved on its own and says where the browser comes from", async () => {
    const { io, calls } = tauriIo(status());
    const { host, root } = await mount(io);
    const toggle = q<HTMLInputElement>(host, "browser-toggle");
    expect(toggle?.disabled).toBe(false);
    expect(toggle?.getAttribute("type")).toBe("checkbox");
    expect(toggle?.closest("label")?.textContent).toContain("its own browser");
    await click(toggle);
    expect(calls.filter((c) => c.cmd === "hermes_web_settings_set")).toHaveLength(0);
    expect(q(host, "web-local-only")).toBeTruthy();
    await click(q(host, "web-save"));
    const set = calls.filter((c) => c.cmd === "hermes_web_settings_set");
    expect(set).toHaveLength(1);
    const sent = set[0].args?.settings as HermesWebStatus["settings"];
    expect(sent.browserEnabled).toBe(true);
    expect(sent.searchEnabled).toBe(false);
    expect(sent.jevEnabled).toBe(false);
    act(() => root.unmount());
    const installed = tauriIo(status({ managedChromium: "/data/components/chromium/154/chrome" }));
    const m = await mount(installed.io);
    expect(q(m.host, "browser-state")?.textContent).toContain("installed and will be used");
    act(() => m.root.unmount());
  });

  it("the browser switch is disabled in the browser preview", async () => {
    const invoke = vi.fn();
    const { host, root } = await mount({ mode: "sim", invoke: invoke as WebSettingsIo["invoke"] });
    expect(q<HTMLInputElement>(host, "browser-toggle")?.disabled).toBe(true);
    expect(q<HTMLInputElement>(host, "browser-toggle")?.checked).toBe(false);
    act(() => root.unmount());
  });

  it("flags a relative SearXNG path and reports a load error honestly", async () => {
    const { io } = tauriIo(status({ loadError: "the web settings file is not valid; defaults are in use" }));
    const { host, root } = await mount(io);
    expect(q(host, "web-settings-load-error")?.textContent).toContain("defaults are in use");
    await type(q(host, "searxng-path"), "searxng-run");
    expect(q(host, "searxng-path-bad")).toBeTruthy();
    act(() => root.unmount());
  });

  it("uses no em-dashes in member-facing text", async () => {
    const { io } = tauriIo(status());
    const { host, root } = await mount(io);
    await click(q(host, "browser-toggle"));
    await click(q(host, "web-search-toggle"));
    await click(q(host, "reader-jina"));
    await click(q(host, "jev-toggle"));
    expect(host.textContent ?? "").not.toContain("—");
    act(() => root.unmount());
  });
});
