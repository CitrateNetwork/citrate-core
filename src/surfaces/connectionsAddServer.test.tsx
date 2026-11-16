// 0.5.4 — Connections › MCP servers: "Add server" was a text box whose button toasted and saved
// nothing. The member's own servers are saved by the one editor (Hermes › Controls › MCP,
// mcp_server_save), so the row opens it. The SaaS rows have no connector in this build: they
// are disabled and say so, instead of toasting "not wired yet".
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Connections } from "./Connections";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
});
async function mount() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<Connections store={store} s={freshState("p1")} />);
  });
}

describe("Connections › Add server", () => {
  it("opens the MCP editor in Hermes › Controls instead of a form that saves nothing", async () => {
    store.setState({ route: "settings/connections", sSec: "account" });
    await mount();
    expect(host!.querySelector('input[placeholder*="Custom server"]')).toBeNull();
    const a = host!.querySelector('[data-testid="mcp-add-server-link"]') as HTMLAnchorElement;
    expect(a.getAttribute("href")).toBe("#/settings/mcp");
    await act(async () => a.click());
    expect(store.state.route).toBe("hermes/controls/details");
    expect(store.state.sSec).toBe("mcp");
  });

  it("SaaS rows are disabled and say they are not available, with no toast", async () => {
    store.setState({ toast: null });
    await mount();
    for (const id of ["slack", "linear", "gmail"]) {
      expect(host!.querySelector(`[data-testid="saas-${id}-unavailable"]`)?.textContent).toMatch(/not available in this build/);
    }
    const connect = [...host!.querySelectorAll("button")].filter((b) => b.textContent === "Connect" && b.disabled);
    expect(connect.length).toBeGreaterThanOrEqual(3);
    expect(host!.textContent).not.toMatch(/on the runbook/);
  });
});
