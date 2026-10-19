// HUP-S4.3 — the "Connected tools (MCP)" card on the Agent surface.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { HermesMcpCard, HermesMcpPanel } from "./HermesMcpCard";
import type { HermesMcpView } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function view(over: Partial<HermesMcpView> = {}): HermesMcpView {
  return {
    settings: { mem: false, scan: false, node: false },
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
    act(() => root.render(<HermesMcpCard view={view({ settings: { mem: true, scan: false, node: false } })} onToggle={onToggle} />));
    const scan = host.querySelector('[aria-label="CitrateScan explorer"]') as HTMLButtonElement;
    act(() => scan.click());
    expect(onToggle).toHaveBeenCalledWith({ mem: true, scan: true, node: false });
    act(() => root.unmount());
  });
});

describe("HermesMcpCard: HUP-S4.1 node entry and live state", () => {
  const withNode = (): HermesMcpView => {
    const v = view({ settings: { mem: false, scan: true, node: true } });
    v.servers.push({ name: "node", label: "This node", transport: "stdio", enabled: true, available: true, detail: "Writes wait for your approval in the app." });
    return v;
  };

  it("lists the node entry as its own switch", () => {
    const html = renderToStaticMarkup(<HermesMcpCard view={withNode()} onToggle={() => {}} />);
    expect(html).toContain("This node");
    expect(html).toMatch(/aria-label="This node"[^>]*aria-checked="true"|aria-checked="true"[^>]*aria-label="This node"/);
  });

  it("shows each enabled server's live state from the running Hermes, and none for a server that is off", () => {
    const html = renderToStaticMarkup(
      <HermesMcpCard
        view={withNode()}
        onToggle={() => {}}
        runtime={{
          running: true,
          configured: true,
          servers: [
            { name: "node", transport: "stdio", state: "ready", era: "legacy", protocolVersion: "2025-06-18", tools: 21, skipped: [] },
            { name: "scan", transport: "http", state: "exited", tools: 0, skipped: [], nextRetryMs: 4000 },
          ],
        }}
      />,
    );
    expect(html).toContain("connected · 21 tools · MCP 2025-06-18");
    expect(html).toContain("reconnecting · retrying in 4 s");
    expect(html).not.toContain('data-testid="mcp-runtime-mem"');
  });

  it("says Hermes is not running instead of guessing", () => {
    const html = renderToStaticMarkup(<HermesMcpCard view={withNode()} onToggle={() => {}} runtime={{ running: false }} />);
    expect(html).toContain("Hermes is not running");
  });
});

describe("HermesMcpCard — the node row (HUP-S4.2 / S8.5)", () => {
  it("is off, and disabled with its reason while the node MCP server is off", () => {
    const v = view();
    v.servers.push({ name: "node", label: "Your node", transport: "stdio", enabled: false, available: false, detail: "Turn on the Node MCP server (Settings, API endpoints & keys) to offer this to Hermes." });
    const html = renderToStaticMarkup(<HermesMcpCard view={v} onToggle={() => {}} />);
    expect(html).toContain("Your node");
    expect(html).toContain("Turn on the Node MCP server");
    expect(html).toMatch(/aria-label="Your node"[^>]*disabled=""|disabled=""[^>]*aria-label="Your node"/);
    expect(html).toMatch(/All start off/);
  });

  it("flips node on and leaves the other switches as they were", () => {
    const onToggle = vi.fn();
    const v = view({ settings: { mem: true, scan: false, node: false } });
    v.servers.push({ name: "node", label: "Your node", transport: "stdio", enabled: false, available: true, detail: "Read-only tools from this node's MCP server." });
    const host = document.createElement("div");
    const root = createRoot(host);
    act(() => root.render(<HermesMcpCard view={v} onToggle={onToggle} />));
    const node = host.querySelector('[aria-label="Your node"]') as HTMLButtonElement;
    act(() => node.click());
    expect(onToggle).toHaveBeenCalledWith({ mem: true, scan: false, node: true });
    act(() => root.unmount());
  });
});

// K2 (0.5.2): the card moved to Hermes › Controls › Details › MCP servers. Its save follows the
// read-modify-write rule from #341/#343: re-read what core has saved, change only the switch the
// member pressed, write that. A change saved elsewhere since the card loaded is kept.
describe("HermesMcpPanel — read-modify-write", () => {
  async function mountPanel(api: Parameters<typeof HermesMcpPanel>[0]["api"]) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => {
      root.render(<HermesMcpPanel api={api} />);
    });
    await act(async () => await new Promise((r) => setTimeout(r, 0)));
    return { host, root };
  }
  const press = async (host: HTMLElement, label: string) => {
    const el = host.querySelector(`[aria-label="${label}"]`) as HTMLButtonElement | null;
    expect(el).toBeTruthy();
    await act(async () => {
      el?.click();
    });
    await act(async () => await new Promise((r) => setTimeout(r, 0)));
  };

  it("keeps a switch another writer turned on after the card loaded", async () => {
    let saved = { mem: false, scan: false, node: false };
    const api = {
      mcpSettings: vi.fn(async () => view({ settings: { ...saved } })),
      mcpSet: vi.fn(async (next: typeof saved) => {
        saved = { ...next };
        return view({ settings: { ...saved } });
      }),
      mcpRuntime: vi.fn(async () => Promise.reject(new Error("not running"))),
    };
    const { host, root } = await mountPanel(api);
    // Another view turns CitrateScan on after this card loaded.
    saved = { ...saved, scan: true };
    await press(host, "Your memory graph");
    expect(api.mcpSet).toHaveBeenCalledTimes(1);
    expect(api.mcpSet).toHaveBeenCalledWith({ mem: true, scan: true, node: false });
    expect(saved).toEqual({ mem: true, scan: true, node: false });
    // The card now shows what core saved, including the other writer's change.
    expect((host.querySelector('[aria-label="CitrateScan explorer"]') as HTMLElement).getAttribute("aria-checked")).toBe("true");
    act(() => root.unmount());
    host.remove();
  });

  it("a failed re-read writes nothing and says why", async () => {
    let reads = 0;
    const api = {
      mcpSettings: vi.fn(async () => {
        reads++;
        if (reads > 1) throw new Error("settings file locked");
        return view();
      }),
      mcpSet: vi.fn(async () => view()),
      mcpRuntime: vi.fn(async () => Promise.reject(new Error("not running"))),
    };
    const { host, root } = await mountPanel(api);
    await press(host, "Your memory graph");
    expect(api.mcpSet).not.toHaveBeenCalled();
    expect(host.textContent).toContain("settings file locked");
    act(() => root.unmount());
    host.remove();
  });
});
