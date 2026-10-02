// HUP-S4.3 — the "Connected tools (MCP)" card on the Agent surface.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { HermesMcpCard } from "./HermesMcpCard";
import type { HermesMcpView } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function view(over: Partial<HermesMcpView> = {}): HermesMcpView {
  return {
    settings: { mem: false, scan: false },
    servers: [
      { name: "mem", label: "Your memory graph", transport: "stdio", enabled: false, available: true, detail: "Read-only memory tools." },
      { name: "scan", label: "CitrateScan explorer", transport: "http", enabled: false, available: true, detail: "Read-only public chain tools." },
    ],
    configWritten: false,
    restartRequired: false,
    ...over,
  };
}

describe("HermesMcpCard — render", () => {
  it("lists both servers, off by default, and says the default awaits sign-off", () => {
    const html = renderToStaticMarkup(<HermesMcpCard view={view()} onToggle={() => {}} />);
    expect(html).toContain("Your memory graph");
    expect(html).toContain("CitrateScan explorer");
    expect((html.match(/aria-checked="false"/g) || []).length).toBe(2);
    expect(html).toMatch(/pending owner sign-off/i);
  });

  it("an unavailable server's switch is disabled and its reason is shown", () => {
    const v = view();
    v.servers[0] = { ...v.servers[0], available: false, detail: "The memory bridge is not available on this machine right now." };
    const html = renderToStaticMarkup(<HermesMcpCard view={v} onToggle={() => {}} />);
    expect(html).toContain("not available on this machine");
    expect(html).toMatch(/aria-label="Your memory graph"[^>]*disabled=""|disabled=""[^>]*aria-label="Your memory graph"/);
  });

  it("asks for a Hermes restart while the sidecar is running", () => {
    const html = renderToStaticMarkup(<HermesMcpCard view={view({ restartRequired: true })} onToggle={() => {}} />);
    expect(html).toMatch(/restart Hermes/i);
  });

  it("shows nothing but an honest line when the settings could not be read", () => {
    const html = renderToStaticMarkup(<HermesMcpCard view={null} error="needs the desktop app" onToggle={() => {}} />);
    expect(html).toContain("needs the desktop app");
    expect(html).not.toContain("aria-checked");
  });
});

describe("HermesMcpCard — toggle", () => {
  it("flips one server and leaves the other as it was", () => {
    const onToggle = vi.fn();
    const host = document.createElement("div");
    const root = createRoot(host);
    act(() => root.render(<HermesMcpCard view={view({ settings: { mem: true, scan: false } })} onToggle={onToggle} />));
    const scan = host.querySelector('[aria-label="CitrateScan explorer"]') as HTMLButtonElement;
    act(() => scan.click());
    expect(onToggle).toHaveBeenCalledWith({ mem: true, scan: true });
    act(() => root.unmount());
  });
});
